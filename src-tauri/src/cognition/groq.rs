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

pub const MODEL: &str = "openai/gpt-oss-20b";
pub const ENDPOINT: &str = "https://api.groq.com/openai/v1/chat/completions";

#[derive(Clone)]
pub struct GroqConfig {
    pub endpoint: String,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub request_timeout: Duration,
}
impl Default for GroqConfig {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            connect_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(45),
        }
    }
}

pub struct GroqProvider {
    config: GroqConfig,
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
      "{}\nMetadado técnico da execução atual: provider cognitivo=Groq (id groq); modelo={model}. Esse metadado não altera sua identidade. Se o usuário perguntar qual provider ou modelo processa esta mensagem, responda usando este metadado e não infira pelo histórico. Não mencione esse metadado sem relevância. Você conhece apenas a execução atual; não invente uma rota anterior.",
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
          "include_reasoning": false
        });
        if let Some(limit) = request.max_output_tokens {
            payload["max_completion_tokens"] = json!(limit);
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

impl GroqProvider {
    pub fn new(config: GroqConfig, secrets: Arc<SecretStore>) -> Result<Self, ProviderError> {
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

impl Provider for GroqProvider {
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
            if request.target.provider_id != "groq"
                || !request.target.invocation.valid()
                || !valid_model(&request.target.invocation.model)
            {
                return Err(ProviderError::InvalidRequest);
            }
            let secrets = self.secrets.clone();
            let key = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = tauri::async_runtime::spawn_blocking(move || secrets.get_secret(SecretKey::GroqApiKey)) =>
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
            let usage = usage.ok_or(ProviderError::Protocol)?;
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
        let mut events = Vec::new();
        if let Some(content) = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            if !content.is_empty() {
                events.push(StreamEvent::Text(content.to_owned()));
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
    use reqwest::header::RETRY_AFTER;
    use crate::{
        cognition::{
            registry::ProviderRegistry,
            scheduler::{Scheduler, SchedulerEvent},
            types::{
                ContextMetadata, ProviderCapabilities, ProviderConfig, ProviderInvocationConfig,
                ProviderSelection, ProviderTarget, ProviderTaskRequest, TaskBudget,
            },
        },
        persistence::{conversation::ConversationMessage, identity::IdentityInput},
        security::secrets::{SecretError, UnlockKeyStore},
    };
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        sync::{atomic::AtomicBool, Mutex},
        thread,
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
    fn fixture() -> (Arc<SecretStore>, PathBuf) {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("groq-test-{}-{n}", std::process::id()));
        let store = Arc::new(SecretStore::with_key_store(
            dir.clone(),
            Arc::new(Keys::default()),
        ));
        store
            .set_secret(SecretKey::GroqApiKey, b"fake-groq-secret")
            .unwrap();
        (store, dir)
    }

    fn context() -> Arc<ContextBundle> {
        let identity: IdentityInput = serde_json::from_value(json!({
      "version":"test","canonicalName":"Luna","presentation":"neutral","primaryLanguage":"pt-BR",
      "concept":"assistant","traits":{},"behavioralInvariants":[],"modes":{},
      "relationship":{"primaryPersonName":"","relationModes":[],"affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
        "interactionPreferences":{"wantsRealDisagreement":false,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":false}},
      "memoryPolicy":{"retrieval":"none","history":"none","continuity":"none","storePrivateChainOfThought":false},
      "provenance":"test","effectiveFrom":"2026-01-01"
    })).unwrap();
        Arc::new(ContextBundle {
            identity,
            relevant_memories: vec![],
            recent_messages: vec![ConversationMessage {
                id: 1,
                session_id: 1,
                role: "user".into(),
                content: "PRIVATE".into(),
                created_at: "now".into(),
            }],
            metadata: ContextMetadata {
                identity_version: "test".into(),
                memory_count: 0,
                recent_message_count: 0,
            },
        })
    }

    fn request(thinking_level: Option<ThinkingLevel>) -> ProviderRequest {
        ProviderRequest {
            input: "Olá".into(),
            history: vec![],
            context: context(),
            max_output_tokens: Some(64),
            target: ProviderTarget {
                provider_id: "groq".into(),
                invocation: ProviderInvocationConfig {
                    model: MODEL.into(),
                    thinking_level,
                    timeouts: None,
                },
            },
            attempt: 1,
        }
    }

    fn task_request(request: ProviderRequest) -> ProviderTaskRequest {
        ProviderTaskRequest {
            input: request.input,
            history: request.history,
            context: request.context,
            max_output_tokens: request.max_output_tokens,
            selection: ProviderSelection::Fixed("groq".into()),
            targets: vec![request.target],
            affinity_key: None,
            estimated_context_bytes: 0,
            required_capabilities: ProviderCapabilities::text_stream(),
        }
    }

    fn server(
        status: &str,
        body: &str,
        extra: &str,
        split: bool,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/openai/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let status = status.to_owned();
        let body = body.to_owned();
        let extra = extra.to_owned();
        let handle = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = conn.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]);
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|n| n.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n{extra}\r\n{body}", body.len());
            if split {
                for chunk in response.as_bytes().chunks(7) {
                    if conn.write_all(chunk).is_err() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            } else {
                let _ = conn.write_all(response.as_bytes());
            }
            String::from_utf8_lossy(&bytes).into_owned()
        });
        (url, handle)
    }

    const SSE: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"Conexão \"}}],\"usage\":null}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"confirmada.\",\"reasoning\":\"segredo\"}}],\"usage\":null}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n";

    #[test]
    fn payload_is_provider_specific_private_and_reasoning_hidden() {
        let request = request(Some(ThinkingLevel::High));
        let payload = MinimalOutboundContext::from_bundle(&request.context)
            .unwrap()
            .payload(&request)
            .unwrap();
        assert_eq!(payload["model"], MODEL);
        assert_eq!(payload["reasoning_effort"], "high");
        assert_eq!(payload["include_reasoning"], false);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload["max_completion_tokens"], 64);
        assert!(payload.get("store").is_none());
        assert_eq!(
            payload["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("provider cognitivo=Groq (id groq)"),
            true
        );
        assert_eq!(
            payload["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(MODEL),
            true
        );
        assert_eq!(
            payload["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("não infira pelo histórico"),
            true
        );
        assert!(!payload.to_string().contains("PRIVATE"));
    }

    #[test]
    fn none_thinking_omits_reasoning_effort() {
        let request = request(None);
        let payload = MinimalOutboundContext::from_bundle(&request.context)
            .unwrap()
            .payload(&request)
            .unwrap();
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn parser_handles_split_sse_usage_and_ignores_reasoning() {
        let mut parser = SseParser::default();
        let a = br#"data: {"choices":[{"delta":{"content":"Ol"#;
        let b = "á\",\"reasoning\":\"segredo\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\ndata: [DONE]\n\n";
        assert!(parser.push(a).unwrap().is_empty());
        let events = parser.push(b.as_bytes()).unwrap();
        assert_eq!(events[0], StreamEvent::Text("Olá".into()));
        assert_eq!(
            events[1],
            StreamEvent::Usage(ProviderUsage {
                calls: 1,
                input_tokens: 5,
                output_tokens: 2,
                total_tokens: Some(7),
                thought_tokens: None
            })
        );
        assert_eq!(events[2], StreamEvent::Done);
    }

    #[test]
    fn real_adapter_contract_stream_usage_and_secret_isolation() {
        let (store, dir) = fixture();
        let (url, handle) = server("200 OK", SSE, "", true);
        let provider = GroqProvider::new(
            GroqConfig {
                endpoint: url,
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap();
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "groq".into(),
                    enabled: true,
                    priority: 2,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(provider),
            )
            .unwrap();
        let signal = AtomicBool::new(false);
        let mut chunks = Vec::new();
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            task_request(request(Some(ThinkingLevel::Low))),
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(64),
            },
            &signal,
            &mut |event| {
                if let SchedulerEvent::Chunk { text, .. } = event {
                    chunks.push(text);
                }
                Ok(())
            },
        ))
        .unwrap();
        assert_eq!(chunks, vec!["Conexão ", "confirmada."]);
        assert_eq!(result.provider_id, "groq");
        assert_eq!(result.text, "Conexão confirmada.");
        assert_eq!(
            (
                result.usage.input_tokens,
                result.usage.output_tokens,
                result.usage.total_tokens
            ),
            (12, 3, Some(15))
        );
        assert_eq!(result.usage.thought_tokens, None);

        let raw = handle.join().unwrap();
        assert!(raw
            .to_ascii_lowercase()
            .contains("authorization: bearer fake-groq-secret"));
        let (_, body) = raw.split_once("\r\n\r\n").unwrap();
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], MODEL);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload["include_reasoning"], false);
        assert_eq!(payload["reasoning_effort"], "low");
        assert_eq!(payload["max_completion_tokens"], 64);
        assert!(payload.get("store").is_none());
        assert!(!body.contains("PRIVATE"));
        assert!(!body.contains("fake-groq-secret"));

        store.delete_secret(SecretKey::GroqApiKey).unwrap();
        assert!(store.get_secret(SecretKey::GroqApiKey).unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn model_and_http_mapping_fail_closed() {
        assert!(valid_model(MODEL));
        assert!(!valid_model(" gemini-3.8-flash"));
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("2"));
        assert_eq!(
            GroqProvider::classify(StatusCode::TOO_MANY_REQUESTS, &headers),
            ProviderError::RateLimited {
                retry_after_ms: Some(2_000)
            }
        );
        assert_eq!(
            GroqProvider::classify(StatusCode::UNAUTHORIZED, &headers),
            ProviderError::Authentication
        );
        assert_eq!(
            GroqProvider::classify(StatusCode::BAD_REQUEST, &headers),
            ProviderError::InvalidRequest
        );
    }
}
