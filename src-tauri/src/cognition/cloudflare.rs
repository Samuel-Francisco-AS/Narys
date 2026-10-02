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
    provider::{Provider, ProviderFuture},
    transport::{cancellation, diagnosed_network_error, timeout_error, TimeoutPhase, retry_after_ms},
    types::{
        ContextBundle, ProviderChunk, ProviderError, ProviderRequest, ProviderResponse,
        ProviderRole, ProviderTimeouts, ProviderUsage,
    },
};
use crate::security::secrets::{SecretKey, SecretStore};

pub const MODEL: &str = "@cf/zai-org/glm-4.7-flash";
pub const ENDPOINT: &str = "https://api.cloudflare.com/client/v4/accounts";

#[derive(Clone)]
pub struct CloudflareConfig {
    pub endpoint: String,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub request_timeout: Duration,
}
impl Default for CloudflareConfig {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            connect_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(45),
        }
    }
}

pub struct CloudflareProvider {
    config: CloudflareConfig,
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
        let mut execution_instruction = format!(
      "{}\nMetadado técnico da execução atual: provider cognitivo=Cloudflare (id cloudflare); modelo={model}. Esse metadado não altera sua identidade. Se o usuário perguntar qual provider ou modelo processa esta mensagem, responda usando este metadado e não infira pelo histórico. Não mencione esse metadado sem relevância. Você conhece apenas a execução atual; não invente uma rota anterior.",
      self.system_instruction
    );
        if let Some(internal) = request.internal_system_instruction.as_deref() {
            execution_instruction.push_str("\nInstrução técnica interna do Luna Core (prioritária):\n");
            execution_instruction.push_str(internal);
        }
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
          "stream_options": {"include_usage": true}
        });
        if let Some(limit) = request.max_output_tokens {
            payload["max_completion_tokens"] = json!(limit);
        }
        Ok(payload)
    }
}

impl CloudflareProvider {
    pub fn new(config: CloudflareConfig, secrets: Arc<SecretStore>) -> Result<Self, ProviderError> {
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

    fn classify(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> ProviderError {
        let code = serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/errors/0/code")
                    .or_else(|| value.pointer("/error/code"))
                    .and_then(|code| {
                        code.as_u64()
                            .map(|value| value.to_string())
                            .or_else(|| code.as_str().map(str::to_owned))
                    })
            });
        match code.as_deref() {
            Some("3036") | Some("5035") => return ProviderError::QuotaExceeded,
            Some("3040") => {
                return ProviderError::Unavailable {
                    retry_after_ms: retry_after_ms(headers),
                }
            }
            _ => {}
        }
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
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b'@'))
}

impl Provider for CloudflareProvider {
    fn supports_invocation(&self, invocation: &super::types::ProviderInvocationConfig, mode: &super::types::InvocationMode) -> bool {
        invocation.valid() && valid_model(&invocation.model) && invocation.thinking_level.is_none()
            && mode.valid() && mode.text_stream()
    }

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
            if request.target.provider_id != "cloudflare"
                || !request.target.invocation.valid()
                || !valid_model(&request.target.invocation.model)
            {
                return Err(ProviderError::InvalidRequest);
            }
            if !self.supports_invocation(&request.target.invocation, &request.mode) {
                return Err(ProviderError::UnsupportedMode);
            }
            let secrets = self.secrets.clone();
            let credentials = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = tauri::async_runtime::spawn_blocking(move || secrets.get_secrets(&[
                  SecretKey::CloudflareApiToken,
                  SecretKey::CloudflareAccountId,
              ])) =>
                result.map_err(|_| ProviderError::Unavailable { retry_after_ms: None })?
                  .map_err(|_| ProviderError::Unavailable { retry_after_ms: None })?
            };
            let key = credentials
                .get(&SecretKey::CloudflareApiToken)
                .and_then(Option::as_ref)
                .ok_or(ProviderError::Authentication)?;
            let account_id = credentials
                .get(&SecretKey::CloudflareAccountId)
                .and_then(Option::as_ref)
                .ok_or(ProviderError::Authentication)?;
            let account_id =
                std::str::from_utf8(&account_id).map_err(|_| ProviderError::Authentication)?;
            if account_id.is_empty()
                || account_id.len() > 64
                || !account_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(ProviderError::Authentication);
            }
            let endpoint = format!(
                "{}/{}/ai/v1/chat/completions",
                self.config.endpoint.trim_end_matches('/'),
                account_id
            );
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
            let started = std::time::Instant::now();
            let connect_ms = self.config.connect_timeout.as_millis().min(u64::MAX as u128) as u64;
            let send = self
                .client
                .post(&endpoint)
                .timeout(Duration::from_millis(timeouts.request_timeout_ms as u64))
                .header(AUTHORIZATION, auth)
                .json(&payload)
                .send();
            let mut response = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = send => result.map_err(|error| diagnosed_network_error(&error, "cloudflare", request, timeouts, connect_ms, started))?,
            };
            if matches!(response.status().as_u16(), 408 | 504) {
                let phase = if response.status().as_u16() == 408 { TimeoutPhase::Http408 } else { TimeoutPhase::Http504 };
                return Err(timeout_error("cloudflare", request, phase, timeouts, connect_ms, started));
            }
            if !response.status().is_success() {
                const MAX_ERROR_BODY: usize = 64 * 1024;
                let status = response.status();
                let headers = response.headers().clone();
                let mut body = Vec::new();
                loop {
                    let chunk = tokio::select! {
                        _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
                        result = response.chunk() => result.map_err(|error| {
                            match diagnosed_network_error(&error, "cloudflare", request, timeouts, connect_ms, started) {
                                ProviderError::Unavailable { .. } => ProviderError::Unavailable { retry_after_ms: retry_after_ms(&headers) },
                                error => error,
                            }
                        })?,
                    };
                    let Some(chunk) = chunk else { break };
                    if body.len().saturating_add(chunk.len()) > MAX_ERROR_BODY { break; }
                    body.extend_from_slice(&chunk);
                }
                return Err(Self::classify(status, &headers, &body));
            }

            let mut parser = SseParser::default();
            let mut text = String::new();
            let mut usage = None;
            let mut done = false;
            let mut finish_reason = None;
            'stream: loop {
                let next = tokio::select! {
                  _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
                  result = tokio::time::timeout(Duration::from_millis(timeouts.stream_idle_timeout_ms as u64), response.chunk()) =>
                    result.map_err(|_| timeout_error("cloudflare", request, TimeoutPhase::StreamIdle, timeouts, connect_ms, started))?.map_err(|error| diagnosed_network_error(&error, "cloudflare", request, timeouts, connect_ms, started))?,
                };
                let Some(bytes) = next else { break };
                let parsed = parser.push(&bytes);
                let parsed = match parsed {
                    Ok(events) => events,
                    Err(error) => {
                        diagnostic(
                            &request.target.invocation.model,
                            "sse_decode",
                            done,
                            finish_label(finish_reason.as_ref()),
                            usage.is_some() || String::from_utf8_lossy(&bytes).contains("\"usage\""),
                            false,
                            text.len(),
                            "malformed_sse_or_usage",
                        );
                        return Err(error);
                    }
                };
                for event in parsed {
                    match event {
                        StreamEvent::Text(piece) => {
                            text.push_str(&piece);
                            on_chunk(ProviderChunk { text: piece })?;
                        }
                        StreamEvent::Usage(value) => usage = Some(value),
                        StreamEvent::Finish(reason) => {
                            match record_finish_reason(&mut finish_reason, reason) {
                                Ok(false) => {}
                                Ok(true) => {
                                    // Cloudflare's OpenAI-compatible stream has been observed
                                    // repeating the same terminal reason on a later usage chunk.
                                    // Identical repetition is idempotent; it changes no outcome.
                                }
                                Err(error) => {
                                    diagnostic(
                                        &request.target.invocation.model,
                                        "terminal",
                                        done,
                                        Some("conflict"),
                                        usage.is_some(),
                                        usage.is_some(),
                                        text.len(),
                                        "conflicting_finish",
                                    );
                                    return Err(error);
                                }
                            }
                        }
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
            if !done {
                diagnostic(&request.target.invocation.model, "terminal", done, finish_label(finish_reason.as_ref()), usage.is_some(), usage.is_some(), text.len(), "missing_done");
                return Err(ProviderError::Protocol);
            }
            match finish_reason.as_ref() {
                Some(FinishReason::Stop) if !text.trim().is_empty() => {}
                Some(FinishReason::Length) => {
                    diagnostic(&request.target.invocation.model, "terminal", done, Some("length"), usage.is_some(), usage.is_some(), text.len(), "incomplete");
                    return Err(ProviderError::Incomplete);
                }
                Some(FinishReason::ToolCalls) => {
                    diagnostic(&request.target.invocation.model, "terminal", done, Some("tool_calls"), usage.is_some(), usage.is_some(), text.len(), "requires_action");
                    return Err(ProviderError::RequiresAction);
                }
                Some(FinishReason::Unknown) | None => {
                    diagnostic(&request.target.invocation.model, "terminal", done, finish_label(finish_reason.as_ref()), usage.is_some(), usage.is_some(), text.len(), "invalid_finish_reason");
                    return Err(ProviderError::Protocol);
                }
                Some(FinishReason::Stop) => {
                    diagnostic(&request.target.invocation.model, "terminal", done, Some("stop"), usage.is_some(), usage.is_some(), text.len(), "empty_content");
                    return Err(ProviderError::Protocol);
                }
            }
            let usage_present = usage.is_some();
            let usage = usage.unwrap_or_default();
            diagnostic(&request.target.invocation.model, "complete", done, Some("stop"), usage_present, usage.output_tokens_measured, text.len(), "none");
            Ok(ProviderResponse { text, usage })
        })
    }
}

#[derive(Debug, PartialEq)]
enum StreamEvent {
    Text(String),
    Usage(ProviderUsage),
    Finish(FinishReason),
    Done,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    Unknown,
}

/// Record a terminal reason exactly once. Some OpenAI-compatible Cloudflare
/// streams can repeat the same finish_reason on a later usage chunk. Repeating
/// the same terminal state is idempotent; conflicting terminal states remain a
/// protocol violation and fail closed.
fn record_finish_reason(
    slot: &mut Option<FinishReason>,
    incoming: FinishReason,
) -> Result<bool, ProviderError> {
    match slot {
        None => {
            *slot = Some(incoming);
            Ok(false)
        }
        Some(existing) if *existing == incoming => Ok(true),
        Some(_) => Err(ProviderError::Protocol),
    }
}

fn finish_label(reason: Option<&FinishReason>) -> Option<&'static str> {
    match reason {
        Some(FinishReason::Stop) => Some("stop"),
        Some(FinishReason::Length) => Some("length"),
        Some(FinishReason::ToolCalls) => Some("tool_calls"),
        Some(FinishReason::Unknown) => Some("unknown"),
        None => None,
    }
}

#[cfg(debug_assertions)]
fn diagnostic(model: &str, phase: &str, done: bool, finish: Option<&str>, usage_present: bool, usage_valid: bool, content_bytes: usize, error: &str) {
    let safe_model: String = model.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-')).take(128).collect();
    eprintln!("[Cloudflare][diag] provider=cloudflare model={safe_model} phase={phase} done={done} finish={} usage_present={usage_present} usage_valid={usage_valid} content_bytes={content_bytes} error={error}", finish.unwrap_or("absent"));
}

#[cfg(not(debug_assertions))]
fn diagnostic(_model: &str, _phase: &str, _done: bool, _finish: Option<&str>, _usage_present: bool, _usage_valid: bool, _content_bytes: usize, _error: &str) {}

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
        if let Some(content) = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            if !content.is_empty() {
                events.push(StreamEvent::Text(content.to_owned()));
            }
        }
        if let Some(raw_reason) = value.pointer("/choices/0/finish_reason") {
            if !raw_reason.is_null() {
                let reason = match raw_reason.as_str() {
                    Some("stop") => FinishReason::Stop,
                    Some("length") => FinishReason::Length,
                    Some("tool_calls") | Some("function_call") => FinishReason::ToolCalls,
                    Some(_) => FinishReason::Unknown,
                    None => return Err(ProviderError::Protocol),
                };
                events.push(StreamEvent::Finish(reason));
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
                output_tokens_measured: true,
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
            "cloudflare-lifecycle-{}-{}",
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
            .set_secrets(&[
                (SecretKey::CloudflareApiToken, b"synthetic-token".to_vec()),
                (
                    SecretKey::CloudflareAccountId,
                    b"synthetic-account".to_vec(),
                ),
            ])
            .unwrap();
        (store, directory)
    }

    fn http_request(timeouts: ProviderTimeouts) -> ProviderRequest {
        ProviderRequest {
            mode: crate::cognition::types::InvocationMode::default(),
            internal_system_instruction: None,
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
                provider_id: "cloudflare".into(),
                invocation: super::super::types::ProviderInvocationConfig {
                    model: MODEL.into(),
                    thinking_level: None,
                    timeouts: Some(timeouts),
                },
            },
            attempt: 0,
        }
    }

    #[test]
    fn payload_has_minimal_context_and_hides_reasoning() {
        assert!(valid_model(MODEL));
        assert!(!valid_model("cloudflare small"));
        assert!(
            !CloudflareProvider::classify(StatusCode::UNAUTHORIZED, &HeaderMap::new(), b"")
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
        let mut rest = "á\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning\":\"secret\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
            .as_bytes()
            .to_vec();
        let events = parser.push(&mut rest).unwrap();
        assert_eq!(events[0], StreamEvent::Text("Olá".into()));
        assert_eq!(events[1], StreamEvent::Finish(FinishReason::Stop));
        assert_eq!(events[2], StreamEvent::Done);
    }

    #[test]
    fn usage_is_optional_but_valid_usage_is_parsed() {
        let mut parser = SseParser::default();
        let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":null}\n\ndata: [DONE]\n\n")
            .unwrap();
        assert_eq!(
            events,
            vec![
                StreamEvent::Text("ok".into()),
                StreamEvent::Finish(FinishReason::Stop),
                StreamEvent::Done
            ]
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
                thought_tokens: None,
                output_tokens_measured: true,
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
            CloudflareProvider::classify(StatusCode::TOO_MANY_REQUESTS, &HeaderMap::new(), b""),
            ProviderError::RateLimited {
                retry_after_ms: None
            }
        );
        assert_eq!(
            CloudflareProvider::classify(StatusCode::INTERNAL_SERVER_ERROR, &HeaderMap::new(), b""),
            ProviderError::Unavailable {
                retry_after_ms: None
            }
        );
        assert_eq!(
            CloudflareProvider::classify(StatusCode::FORBIDDEN, &HeaderMap::new(), b""),
            ProviderError::Authentication
        );
        assert_eq!(
            CloudflareProvider::classify(
                StatusCode::TOO_MANY_REQUESTS,
                &HeaderMap::new(),
                br#"{"errors":[{"code":3036}]}"#,
            ),
            ProviderError::QuotaExceeded
        );
        assert_eq!(
            CloudflareProvider::classify(
                StatusCode::TOO_MANY_REQUESTS,
                &HeaderMap::new(),
                br#"{"errors":[{"code":3040}]}"#,
            ),
            ProviderError::Unavailable {
                retry_after_ms: None
            }
        );
        assert_eq!(
            CloudflareProvider::classify(
                StatusCode::FORBIDDEN,
                &HeaderMap::new(),
                br#"{"errors":[{"code":5035}]}"#,
            ),
            ProviderError::QuotaExceeded
        );
        assert_eq!(
            CloudflareProvider::classify(StatusCode::REQUEST_TIMEOUT, &HeaderMap::new(), b""),
            ProviderError::Timeout
        );
    }

    #[test]
    fn finish_reason_is_required_and_classified_without_exposing_reasoning() {
        let mut parser = SseParser::default();
        let events = parser
            .push(
                br#"data: {"choices":[{"delta":{"content":"{\"title\":\"T\u00edtulo\",\"summary\":\"texto incom"},"finish_reason":"length"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}

data: [DONE]

"#,
            )
            .unwrap();
        assert_eq!(
            events,
            vec![
                StreamEvent::Text("{\"title\":\"Título\",\"summary\":\"texto incom".into()),
                StreamEvent::Finish(FinishReason::Length),
                StreamEvent::Usage(ProviderUsage {
                    calls: 1,
                    input_tokens: 1,
                    output_tokens: 2,
                    total_tokens: Some(3),
                    thought_tokens: None,
                    output_tokens_measured: true,
                }),
                StreamEvent::Done
            ]
        );

        for (reason, expected) in [
            ("tool_calls", FinishReason::ToolCalls),
            ("function_call", FinishReason::ToolCalls),
            ("mystery", FinishReason::Unknown),
        ] {
            let mut parser = SseParser::default();
            let events = parser
                .push(
                    format!(
                        "data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"{reason}\"}}]}}\n\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
            assert_eq!(events, vec![StreamEvent::Finish(expected)]);
        }

        let mut parser = SseParser::default();
        assert_eq!(
            parser
                .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n"),
            Ok(vec![StreamEvent::Text("x".into()), StreamEvent::Done])
        );
    }

    #[test]
    fn duplicate_finish_reason_is_idempotent_but_conflict_fails_closed() {
        let duplicate = "data: {\"choices\":[{\"delta\":{\"content\":\"visible\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\ndata: [DONE]\n\n";
        let (store, directory) = http_fixture();
        let (endpoint, handle) =
            super::super::transport::test_support::server("200 OK", duplicate, false, Duration::ZERO);
        let provider = CloudflareProvider::new(
            CloudflareConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 500,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ))
        .unwrap();
        handle.join().unwrap();
        assert_eq!(result.text, "visible");
        assert!(result.usage.output_tokens_measured);
        assert_eq!(result.usage.output_tokens, 1);
        std::fs::remove_dir_all(directory).unwrap();

        let conflicting = "data: {\"choices\":[{\"delta\":{\"content\":\"visible\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\ndata: [DONE]\n\n";
        let (store, directory) = http_fixture();
        let (endpoint, handle) =
            super::super::transport::test_support::server("200 OK", conflicting, false, Duration::ZERO);
        let provider = CloudflareProvider::new(
            CloudflareConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 500,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ));
        handle.join().unwrap();
        assert_eq!(result, Err(ProviderError::Protocol));
        std::fs::remove_dir_all(directory).unwrap();

        let mut finish = None;
        assert_eq!(record_finish_reason(&mut finish, FinishReason::Stop), Ok(false));
        assert_eq!(record_finish_reason(&mut finish, FinishReason::Stop), Ok(true));
        assert_eq!(
            record_finish_reason(&mut finish, FinishReason::ToolCalls),
            Err(ProviderError::Protocol)
        );
    }

    #[test]
    fn local_http_lifecycle_covers_split_valid_eof_timeout_and_internal_code() {
        let (store, directory) = http_fixture();
        let provider = CloudflareProvider::new(CloudflareConfig::default(), store).unwrap();
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

        let valid = "data: {\"choices\":[{\"delta\":{\"content\":\"vis\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"ible\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\ndata: [DONE]\n\n";
        let (store, directory) = http_fixture();
        let (endpoint, handle) =
            super::super::transport::test_support::server("200 OK", valid, false, Duration::ZERO);
        let provider = CloudflareProvider::new(
            CloudflareConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 500,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ))
        .unwrap();
        handle.join().unwrap();
        assert_eq!(result.text, "visible");
        std::fs::remove_dir_all(directory).unwrap();

        let (store, directory) = http_fixture();
        let (endpoint, handle) = super::super::transport::test_support::server(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"title\\\":\\\"T\\\",\\\"summary\\\":\\\"incom\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":null},\"finish_reason\":\"length\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":256,\"total_tokens\":257}}\n\ndata: [DONE]\n\n",
            false,
            Duration::ZERO,
        );
        let provider = CloudflareProvider::new(
            CloudflareConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 500,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ));
        handle.join().unwrap();
        assert!(matches!(result, Err(ProviderError::Incomplete)));
        std::fs::remove_dir_all(directory).unwrap();

        let (store, directory) = http_fixture();
        let (endpoint, handle) = super::super::transport::test_support::server(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
            false,
            Duration::ZERO,
        );
        let provider = CloudflareProvider::new(
            CloudflareConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts {
                request_timeout_ms: 500,
                stream_idle_timeout_ms: 500,
            }),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ));
        handle.join().unwrap();
        assert!(matches!(result, Err(ProviderError::Incomplete)));
        std::fs::remove_dir_all(directory).unwrap();

        let (store, directory) = http_fixture();
        let (endpoint, handle) = super::super::transport::test_support::server(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"visible\"}}]}\n\n",
            false,
            Duration::ZERO,
        );
        let provider = CloudflareProvider::new(
            CloudflareConfig {
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
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":\"bad\",\"completion_tokens\":2,\"total_tokens\":2}}\n\ndata: [DONE]\n\n",
            false,
            Duration::ZERO,
        );
        let provider = CloudflareProvider::new(CloudflareConfig { endpoint, ..Default::default() }, store).unwrap();
        let result = tauri::async_runtime::block_on(provider.execute(
            &http_request(ProviderTimeouts { request_timeout_ms: 500, stream_idle_timeout_ms: 500 }),
            &AtomicBool::new(false), &mut |_| Ok(()),
        ));
        handle.join().unwrap();
        assert!(matches!(result, Err(ProviderError::Protocol)));
        std::fs::remove_dir_all(directory).unwrap();

        let (store, directory) = http_fixture();
        let (endpoint, handle) = super::super::transport::test_support::server(
            "429 Too Many Requests",
            r#"{"errors":[{"code":3036}]}"#,
            false,
            Duration::ZERO,
        );
        let provider = CloudflareProvider::new(
            CloudflareConfig {
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
        assert!(matches!(result, Err(ProviderError::QuotaExceeded)));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
