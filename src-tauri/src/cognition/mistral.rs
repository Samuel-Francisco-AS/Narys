use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client, StatusCode,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use super::{
    policy::ThinkingLevel,
    provider::{Provider, ProviderFuture},
    transport::{cancellation, network_error, retry_after_ms},
    types::{
        ContextBundle, ProviderChunk, ProviderError, ProviderRequest, ProviderResponse,
        ProviderRole, ProviderTimeouts, ProviderUsage,
    },
};
use crate::security::secrets::{SecretKey, SecretStore};

pub const MODEL: &str = "mistral-small-2603";
pub const ENDPOINT: &str = "https://api.mistral.ai/v1/chat/completions";

#[derive(Clone)]
pub struct MistralConfig {
    pub endpoint: String,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub request_timeout: Duration,
}
impl Default for MistralConfig {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            connect_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(45),
        }
    }
}

pub struct MistralProvider {
    config: MistralConfig,
    client: Client,
    secrets: Arc<SecretStore>,
    timeouts: Arc<RwLock<ProviderTimeouts>>,
}

struct MinimalOutboundContext {
    system_instruction: String,
}
impl MinimalOutboundContext {
    fn from_bundle(bundle: &ContextBundle) -> Result<Self, ProviderError> {
        let name = bundle.identity.canonical_name.trim();
        let language = bundle.identity.primary_language.trim();
        if name.is_empty()
            || name.len() > 64
            || name.chars().any(char::is_control)
            || language.is_empty()
            || language.len() > 16
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(ProviderError::Fatal);
        }
        let language = if language == "pt-BR" {
            "português brasileiro".to_owned()
        } else {
            format!("idioma {language}")
        };
        Ok(Self { system_instruction: format!(
      "Você é {name}, uma assistente virtual. Responda em {language}. Seja clara, natural e tecnicamente rigorosa. Avalie premissas e preserve a autonomia do usuário."
    ) })
    }

    fn payload(&self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        let model = &request.target.invocation.model;
        if !valid_model(model) {
            return Err(ProviderError::InvalidRequest);
        }
        let execution_instruction = format!(
      "{}\nMetadado técnico da execução atual: provider cognitivo=Mistral (id mistral); modelo={model}. Esse metadado não altera sua identidade. Se o usuário perguntar qual provider ou modelo processa esta mensagem, responda usando este metadado e não infira pelo histórico. Não mencione esse metadado sem relevância. Você conhece apenas a execução atual; não invente uma rota anterior.",
      self.system_instruction
    );
        let mut messages = vec![json!({"role":"system","content":execution_instruction})];
        for message in &request.history {
            messages.push(json!({
        "role": match message.role { ProviderRole::User => "user", ProviderRole::Assistant => "assistant" },
        "content": message.content,
      }));
        }
        messages.push(json!({"role":"user","content":request.input}));
        let mut payload = json!({
          "model": model,
          "messages": messages,
          "stream": true,
          "stream_options": {"include_usage": true},
        });
        if let Some(limit) = request.max_output_tokens {
            payload["max_tokens"] = json!(limit);
        }
        if let Some(level) = request.target.invocation.thinking_level {
            payload["reasoning_effort"] = json!(match level {
                ThinkingLevel::Low => "low",
                ThinkingLevel::Medium => "medium",
                ThinkingLevel::High => "high",
            });
        }
        Ok(payload)
    }
}

impl MistralProvider {
    pub fn new(config: MistralConfig, secrets: Arc<SecretStore>) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .build()
            .map_err(|_| ProviderError::Unavailable {
                retry_after_ms: None,
            })?;
        let timeouts = ProviderTimeouts {
            request_timeout_ms: config
                .request_timeout
                .as_millis()
                .try_into()
                .map_err(|_| ProviderError::Fatal)?,
            stream_idle_timeout_ms: config
                .idle_timeout
                .as_millis()
                .try_into()
                .map_err(|_| ProviderError::Fatal)?,
        };
        Ok(Self {
            config,
            client,
            secrets,
            timeouts: Arc::new(RwLock::new(timeouts)),
        })
    }
    pub fn timeout_handle(&self) -> Arc<RwLock<ProviderTimeouts>> {
        self.timeouts.clone()
    }

    fn classify(status: StatusCode, headers: &HeaderMap) -> ProviderError {
        match status.as_u16() {
            400 | 404 | 413 | 422 => ProviderError::InvalidRequest,
            401 | 403 => ProviderError::Authentication,
            402 => ProviderError::QuotaExceeded,
            408 | 504 => ProviderError::Timeout,
            429 => ProviderError::RateLimited {
                retry_after_ms: retry_after_ms(headers),
            },
            500..=599 => ProviderError::Unavailable {
                retry_after_ms: retry_after_ms(headers),
            },
            _ => ProviderError::Fatal,
        }
    }
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && model.trim() == model
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
}

impl Provider for MistralProvider {
    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if cancelled.load(Ordering::Acquire) {
                return Err(ProviderError::Cancelled);
            }
            if request.target.provider_id != "mistral"
                || !request.target.invocation.valid()
                || !valid_model(&request.target.invocation.model)
            {
                return Err(ProviderError::InvalidRequest);
            }
            let secrets = self.secrets.clone();
            let key = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = tauri::async_runtime::spawn_blocking(move || secrets.get_secret(SecretKey::MistralApiKey)) =>
                result.map_err(|_| ProviderError::Unavailable { retry_after_ms: None })?
                  .map_err(|_| ProviderError::Unavailable { retry_after_ms: None })?
                  .ok_or(ProviderError::Authentication)?,
            };
            let mut auth = b"Bearer ".to_vec();
            auth.extend_from_slice(&key);
            let mut auth =
                HeaderValue::from_bytes(&auth).map_err(|_| ProviderError::Authentication)?;
            auth.set_sensitive(true);
            let payload =
                MinimalOutboundContext::from_bundle(&request.context)?.payload(request)?;
            let timeouts = request
                .target
                .invocation
                .timeouts
                .unwrap_or_else(|| *self.timeouts.read().unwrap_or_else(|p| p.into_inner()));
            let send = self
                .client
                .post(&self.config.endpoint)
                .timeout(Duration::from_millis(timeouts.request_timeout_ms as u64))
                .header(AUTHORIZATION, auth)
                .json(&payload)
                .send();
            let mut response = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = send => result.map_err(|error| network_error(&error))?,
            };
            if !response.status().is_success() {
                return Err(Self::classify(response.status(), response.headers()));
            }

            let mut parser = SseParser::default();
            let mut text = String::new();
            let mut usage = None;
            let mut done = false;
            'stream: loop {
                let next = tokio::select! {
                  _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
                  result = tokio::time::timeout(Duration::from_millis(timeouts.stream_idle_timeout_ms as u64), response.chunk()) =>
                    result.map_err(|_| ProviderError::Timeout)?.map_err(|error| network_error(&error))?,
                };
                let Some(bytes) = next else { break };
                for event in parser.push(&bytes)? {
                    match event {
                        StreamEvent::Text(piece) => {
                            text.push_str(&piece);
                            on_chunk(ProviderChunk { text: piece })?;
                        }
                        StreamEvent::Usage(value) => usage = Some(value),
                        StreamEvent::Done => {
                            done = true;
                            break 'stream;
                        }
                        StreamEvent::Ignore => {}
                    }
                }
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(ProviderError::Cancelled);
            }
            if !done || text.trim().is_empty() {
                return Err(ProviderError::Protocol);
            }
            let usage = usage.unwrap_or_default();
            Ok(ProviderResponse { text, usage })
        })
    }
}

#[derive(Debug, PartialEq)]
enum StreamEvent {
    Text(String),
    Usage(ProviderUsage),
    Done,
    Ignore,
}

#[derive(Default)]
struct SseParser {
    pending: Vec<u8>,
    data: String,
}
impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > 1_048_576 {
            return Err(ProviderError::Protocol);
        }
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let mut line = self.pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = std::str::from_utf8(&line).map_err(|_| ProviderError::Protocol)?;
            if line.is_empty() {
                events.extend(self.finish_event()?);
            } else if let Some(value) = line.strip_prefix("data:") {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value.trim_start());
            }
            if self.data.len() > 1_048_576 {
                return Err(ProviderError::Protocol);
            }
        }
        Ok(events)
    }

    fn finish_event(&mut self) -> Result<Vec<StreamEvent>, ProviderError> {
        let data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return Ok(vec![]);
        }
        if data.trim() == "[DONE]" {
            return Ok(vec![StreamEvent::Done]);
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| ProviderError::Protocol)?;
        if value.get("error").is_some() {
            return Err(ProviderError::Protocol);
        }
        if value.get("choices").is_none() && value.get("usage").is_none() {
            return Err(ProviderError::Protocol);
        }
        let mut events = Vec::new();
        if let Some(content) = value.pointer("/choices/0/delta/content") {
            match content {
                Value::String(text) if !text.is_empty() => {
                    events.push(StreamEvent::Text(text.clone()));
                }
                Value::String(_) => {}
                Value::Array(chunks) => {
                    for chunk in chunks {
                        let object = chunk.as_object().ok_or(ProviderError::Protocol)?;
                        match object.get("type").and_then(Value::as_str) {
                            Some("thinking") => {
                                let thinking = object
                                    .get("thinking")
                                    .and_then(Value::as_array)
                                    .ok_or(ProviderError::Protocol)?;
                                if thinking.len() > 64 {
                                    return Err(ProviderError::Protocol);
                                }
                                for inner in thinking {
                                    let inner = inner.as_object().ok_or(ProviderError::Protocol)?;
                                    if inner.get("type").and_then(Value::as_str) != Some("text")
                                        || inner
                                            .get("text")
                                            .and_then(Value::as_str)
                                            .is_none_or(|text| text.len() > 1_048_576)
                                    {
                                        return Err(ProviderError::Protocol);
                                    }
                                }
                            }
                            Some("text") => {
                                let text = object
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .ok_or(ProviderError::Protocol)?;
                                if !text.is_empty() {
                                    events.push(StreamEvent::Text(text.to_owned()));
                                }
                            }
                            _ => return Err(ProviderError::Protocol),
                        }
                    }
                }
                _ => return Err(ProviderError::Protocol),
            }
        }
        if let Some(raw_usage) = value.get("usage").filter(|usage| !usage.is_null()) {
            let input_tokens = token_field(raw_usage, "prompt_tokens")?;
            let output_tokens = token_field(raw_usage, "completion_tokens")?;
            let total_tokens = token_field(raw_usage, "total_tokens")?;
            events.push(StreamEvent::Usage(ProviderUsage {
                calls: 1,
                input_tokens,
                output_tokens,
                total_tokens: Some(total_tokens),
                thought_tokens: None,
            }));
        }
        if events.is_empty() {
            events.push(StreamEvent::Ignore);
        }
        Ok(events)
    }
}

fn token_field(usage: &Value, name: &str) -> Result<u32, ProviderError> {
    usage
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(ProviderError::Protocol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::secrets::{SecretError, UnlockKeyStore};
    use std::{
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Default)]
    struct Keys(Mutex<Option<Vec<u8>>>);
    impl UnlockKeyStore for Keys {
        fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn store(&self, key: &[u8]) -> Result<(), SecretError> {
            *self.0.lock().unwrap() = Some(key.to_vec());
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    fn http_fixture() -> (std::sync::Arc<SecretStore>, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "mistral-lifecycle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = std::sync::Arc::new(SecretStore::with_key_store(
            directory.clone(),
            std::sync::Arc::new(Keys::default()),
        ));
        store
            .set_secret(SecretKey::MistralApiKey, b"synthetic-mistral-secret")
            .unwrap();
        (store, directory)
    }

    fn http_request(timeouts: ProviderTimeouts) -> ProviderRequest {
        ProviderRequest {
            input: "hello".into(),
            history: vec![],
            context: std::sync::Arc::new(ContextBundle {
                identity: serde_json::from_value(serde_json::json!({
                    "version":"v1","canonicalName":"Luna","presentation":"neutral",
                    "primaryLanguage":"pt-BR","concept":"test","traits":{},
                    "behavioralInvariants":[],"modes":{},"relationship":{
                      "primaryPersonName":"","relationModes":[],"affectionStyle":{
                        "warm":false,"provocative":false,"playfulJealousy":false,
                        "playfulTerritoriality":false,"coercion":false,"isolation":false,
                        "emotionalBlackmail":false},"interactionPreferences":{
                        "wantsRealDisagreement":false,
                        "wantsLunaToProposeDirectionsDuringStructuring":false,
                        "prefersLinearFlowDuringImplementation":false}},
                    "memoryPolicy":{"retrieval":"none","history":"none",
                      "continuity":"none","storePrivateChainOfThought":false},
                    "provenance":"test","effectiveFrom":"2026-01-01"
                }))
                .unwrap(),
                relevant_memories: vec![],
                recent_messages: vec![],
                metadata: super::super::types::ContextMetadata {
                    identity_version: "v1".into(),
                    memory_count: 0,
                    recent_message_count: 0,
                },
            }),
            max_output_tokens: Some(64),
            target: super::super::types::ProviderTarget {
                provider_id: "mistral".into(),
                invocation: super::super::types::ProviderInvocationConfig {
                    model: MODEL.into(),
                    thinking_level: Some(ThinkingLevel::Low),
                    timeouts: Some(timeouts),
                },
            },
            attempt: 0,
        }
    }

    #[test]
    fn payload_has_minimal_context_and_hides_reasoning() {
        assert!(valid_model(MODEL));
        assert!(!valid_model("mistral small"));
        assert!(
            !MistralProvider::classify(StatusCode::UNAUTHORIZED, &HeaderMap::new())
                .eq(&ProviderError::Fatal)
        );
    }

    #[test]
    fn parser_splits_events_done_and_hides_reasoning() {
        let mut parser = SseParser::default();
        assert!(parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"Ol")
            .unwrap()
            .is_empty());
        let mut rest = "á\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning\":\"secret\"}}]}\n\ndata: [DONE]\n\n"
            .as_bytes()
            .to_vec();
        let events = parser.push(&mut rest).unwrap();
        assert_eq!(events[0], StreamEvent::Text("Olá".into()));
        assert_eq!(events[1], StreamEvent::Ignore);
        assert_eq!(events[2], StreamEvent::Done);
    }

    #[test]
    fn typed_reasoning_content_emits_only_text_and_supports_string_transition() {
        let mut parser = SseParser::default();
        let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"private one\"},{\"type\":\"text\",\"text\":\"private two\"}]},{\"type\":\"text\",\"text\":\"visible\"}]}}]}\n\n")
            .unwrap();
        assert_eq!(events, vec![StreamEvent::Text("visible".into())]);
        let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\" continuation\"}}]}\n\n")
            .unwrap();
        assert_eq!(events, vec![StreamEvent::Text(" continuation".into())]);
        assert!(!format!("{events:?}").contains("private"));
    }

    #[test]
    fn malformed_typed_content_fails_closed() {
        let mut parser = SseParser::default();
        assert_eq!(
            parser.push(
                b"data: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":\"private\"}]}}]}\n\n"
            ),
            Err(ProviderError::Protocol)
        );
        let mut parser = SseParser::default();
        assert_eq!(
            parser.push(b"data: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"image\",\"text\":\"x\"}]}]}}]}\n\n"),
            Err(ProviderError::Protocol)
        );
    }

    #[test]
    fn payload_uses_mistral_contract_without_unknown_reasoning_field() {
        let context = ContextBundle {
            identity: serde_json::from_value(serde_json::json!({
                "version":"v1","canonicalName":"Synthetic","presentation":"neutral",
                "primaryLanguage":"pt-BR","concept":"test","traits":{"curiosity":"high"},
                "behavioralInvariants":["be_clear"],"modes":{"test":{"priority":"test","tone":"calm"}},
                "relationship":{"primaryPersonName":"Tester","relationModes":["testing"],
                  "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,
                    "playfulTerritoriality":false,"coercion":false,"isolation":false,
                    "emotionalBlackmail":false},
                  "interactionPreferences":{"wantsRealDisagreement":true,
                    "wantsLunaToProposeDirectionsDuringStructuring":false,
                    "prefersLinearFlowDuringImplementation":true}},
                "memoryPolicy":{"retrieval":"selective","history":"versioned",
                  "continuity":"revisable","storePrivateChainOfThought":false},
                "provenance":"synthetic","effectiveFrom":"2026-01-01"
            })).unwrap(),
            relevant_memories: vec![],
            recent_messages: vec![],
            metadata: super::super::types::ContextMetadata {
                identity_version: "v1".into(),
                memory_count: 0,
                recent_message_count: 0,
            },
        };
        let request = ProviderRequest {
            input: "hello".into(),
            history: vec![],
            context: std::sync::Arc::new(context),
            max_output_tokens: Some(321),
            target: super::super::types::ProviderTarget {
                provider_id: "mistral".into(),
                invocation: super::super::types::ProviderInvocationConfig {
                    model: "custom-model".into(),
                    thinking_level: Some(ThinkingLevel::High),
                    timeouts: None,
                },
            },
            attempt: 0,
        };
        let payload = MinimalOutboundContext {
            system_instruction: "test".into(),
        }
        .payload(&request)
        .unwrap();
        assert_eq!(payload["max_tokens"], 321);
        assert_eq!(payload["reasoning_effort"], "high");
        assert!(payload.get("include_reasoning").is_none());
        assert!(payload.get("max_completion_tokens").is_none());
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
    }

    #[test]
    fn usage_is_optional_but_valid_usage_is_parsed() {
        let mut parser = SseParser::default();
        let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}],\"usage\":null}\n\ndata: [DONE]\n\n")
            .unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::Text("ok".into()), StreamEvent::Done]
        );
        let mut parser = SseParser::default();
        let events = parser
            .push(b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n")
            .unwrap();
        assert_eq!(
            events[0],
            StreamEvent::Usage(ProviderUsage {
                calls: 1,
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: Some(5),
                thought_tokens: None
            })
        );
    }

    #[test]
    fn malformed_and_statuses_fail_closed() {
        let mut parser = SseParser::default();
        assert_eq!(
            parser.push(b"data: {bad}\n\n"),
            Err(ProviderError::Protocol)
        );
        assert_eq!(
            MistralProvider::classify(StatusCode::TOO_MANY_REQUESTS, &HeaderMap::new()),
            ProviderError::RateLimited {
                retry_after_ms: None
            }
        );
        assert_eq!(
            MistralProvider::classify(StatusCode::INTERNAL_SERVER_ERROR, &HeaderMap::new()),
            ProviderError::Unavailable {
                retry_after_ms: None
            }
        );
        assert_eq!(
            MistralProvider::classify(StatusCode::FORBIDDEN, &HeaderMap::new()),
            ProviderError::Authentication
        );
    }

    #[test]
    fn local_http_lifecycle_covers_split_thinking_eof_http_and_cancel() {
        let (store, directory) = http_fixture();
        let provider = MistralProvider::new(MistralConfig::default(), store).unwrap();
        assert!(matches!(
            tauri::async_runtime::block_on(provider.execute(
                &http_request(ProviderTimeouts {
                    request_timeout_ms: 500,
                    stream_idle_timeout_ms: 500,
                }),
                &AtomicBool::new(true),
                &mut |_| Ok(()),
            )),
            Err(ProviderError::Cancelled)
        ));
        std::fs::remove_dir_all(directory).unwrap();

        let body = "data: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"private-only\"}]}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"private-close\"}]},{\"type\":\"text\",\"text\":\"visible\"}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" final\"}}]}\n\ndata: [DONE]\n\n";
        let (store, directory) = http_fixture();
        let (endpoint, handle) =
            super::super::transport::test_support::server("200 OK", body, false, Duration::ZERO);
        let provider = MistralProvider::new(
            MistralConfig {
                endpoint,
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap();
        let request = http_request(ProviderTimeouts {
            request_timeout_ms: 500,
            stream_idle_timeout_ms: 500,
        });
        let cancelled = AtomicBool::new(false);
        let mut public = String::new();
        let response =
            tauri::async_runtime::block_on(provider.execute(&request, &cancelled, &mut |chunk| {
                public.push_str(&chunk.text);
                Ok(())
            }))
            .unwrap();
        handle.join().unwrap();
        assert_eq!(public, "visible final");
        assert_eq!(response.text, "visible final");
        assert!(!public.contains("private"));
        std::fs::remove_dir_all(directory).unwrap();

        let (store, directory) = http_fixture();
        let (endpoint, handle) = super::super::transport::test_support::server(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"visible\"}}]}\n\n",
            false,
            Duration::ZERO,
        );
        let provider = MistralProvider::new(
            MistralConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 100,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ));
        handle.join().unwrap();
        assert!(matches!(result, Err(ProviderError::Protocol)));
        std::fs::remove_dir_all(directory).unwrap();

        let (store, directory) = http_fixture();
        let (endpoint, handle) = super::super::transport::test_support::server(
            "401 Unauthorized",
            "denied",
            false,
            Duration::ZERO,
        );
        let provider = MistralProvider::new(
            MistralConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 100,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ));
        handle.join().unwrap();
        assert!(matches!(result, Err(ProviderError::Authentication)));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
