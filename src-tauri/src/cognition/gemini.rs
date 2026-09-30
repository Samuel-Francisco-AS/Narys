use crate::security::secrets::{SecretKey, SecretStore};
use reqwest::{
    header::{HeaderMap, HeaderValue, RETRY_AFTER},
    Client, StatusCode,
};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::{Duration, SystemTime},
};

use super::{
    policy::ThinkingLevel,
    provider::{Provider, ProviderFuture},
    types::{
        ContextBundle, ProviderChunk, ProviderError, ProviderMessage, ProviderRequest,
        ProviderResponse, ProviderRole, ProviderTimeouts, ProviderUsage,
    },
};

pub const MODEL: &str = "gemini-3.8-flash";
pub const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
#[cfg(test)]
pub const PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS: u32 = 4096;

#[derive(Clone)]
pub struct GeminiConfig {
    pub endpoint: String,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub request_timeout: Duration,
}
impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            connect_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(45),
        }
    }
}

// Explicit allowlist: the local bundle can contain intimate relationship, memories and history.
// The cloud receives these identity fields, explicit-session text history, and the current message.
pub struct MinimalOutboundContext {
    system_instruction: String,
}
impl MinimalOutboundContext {
    pub fn from_bundle(bundle: &ContextBundle) -> Result<Self, ProviderError> {
        let name = bundle.identity.canonical_name.trim();
        let language = bundle.identity.primary_language.trim();
        if name.is_empty()
            || name.len() > 64
            || name.chars().any(|c| c.is_control())
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
        Ok(Self { system_instruction: format!("Você é {name}, uma assistente virtual. Responda em {language}. Seja clara, natural e tecnicamente rigorosa. Avalie premissas e preserve a autonomia do usuário.") })
    }
    fn payload(
        &self,
        model: &str,
        input: &str,
        history: &[ProviderMessage],
        max_output_tokens: Option<u32>,
        thinking_level: Option<ThinkingLevel>,
    ) -> Value {
        let input = if history.is_empty() {
            json!(input)
        } else {
            let mut steps: Vec<Value> = history.iter().map(|message| json!({
        "type": match message.role { ProviderRole::User => "user_input", ProviderRole::Assistant => "model_output" },
        "content": [{"type":"text","text":message.content}]
      })).collect();
            steps.push(json!({"type":"user_input","content":[{"type":"text","text":input}]}));
            Value::Array(steps)
        };
        let mut generation_config = json!({"thinking_summaries":"none"});
        if let Some(limit) = max_output_tokens {
            generation_config["max_output_tokens"] = json!(limit);
        }
        if let Some(level) = thinking_level {
            generation_config["thinking_level"] = json!(level.as_str());
        }
        let execution_instruction = format!(
      "{}\nMetadado técnico da execução atual: provider cognitivo=Gemini (id gemini); modelo={model}. Esse metadado não altera sua identidade. Se o usuário perguntar qual provider ou modelo processa esta mensagem, responda usando este metadado e não infira pelo histórico. Não mencione esse metadado sem relevância. Você conhece apenas a execução atual; não invente uma rota anterior.",
      self.system_instruction
    );
        json!({"model":model,"store":false,"stream":true,"system_instruction":execution_instruction,
      "input":input,"generation_config":generation_config})
    }
}

pub struct GeminiProvider {
    config: GeminiConfig,
    client: Client,
    secrets: Arc<SecretStore>,
    timeouts: Arc<RwLock<ProviderTimeouts>>,
}
impl GeminiProvider {
    pub fn new(config: GeminiConfig, secrets: Arc<SecretStore>) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .build()
            .map_err(|_| {
                #[cfg(debug_assertions)]
                eprintln!("[Gemini][diag] unavailable source=client_init");
                ProviderError::Unavailable {
                    retry_after_ms: None,
                }
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
            429 => ProviderError::RateLimited {
                retry_after_ms: retry_after_ms(headers),
            },
            400 | 404 => ProviderError::InvalidRequest,
            401 | 403 => ProviderError::Authentication,
            408 | 504 => ProviderError::Timeout,
            500..=599 => ProviderError::Unavailable {
                retry_after_ms: retry_after_ms(headers),
            },
            _ => ProviderError::Fatal,
        }
    }
}
fn classify_error_code(code: &str, retry_after_ms: Option<u64>) -> ProviderError {
    match code {
        "authentication" | "permission_denied" => ProviderError::Authentication,
        "rate_limit_exceeded" | "too_many_requests" => {
            ProviderError::RateLimited { retry_after_ms }
        }
        "quota_exceeded" | "payment_required" => ProviderError::QuotaExceeded,
        "deadline_exceeded" | "gateway_timeout" => ProviderError::Timeout,
        "invalid_argument" | "invalid_request" | "not_found" | "model_not_found" => {
            ProviderError::InvalidRequest
        }
        "api_error" | "service_unavailable" => ProviderError::Unavailable { retry_after_ms },
        "cancelled" => ProviderError::RemoteCancelled,
        // All request, generation and unknown errors fail closed; no message is exposed.
        _ => ProviderError::Fatal,
    }
}
fn error_code(value: &Value) -> Option<&str> {
    value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .filter(|code| !code.is_empty())
}
#[cfg(debug_assertions)]
fn diag_http_unavailable(
    status: StatusCode,
    headers: &HeaderMap,
    code: Option<&str>,
    result: &ProviderError,
) {
    if !matches!(result, ProviderError::Unavailable { .. }) {
        return;
    }
    // Only these known codes can produce Unavailable. Never print arbitrary response strings.
    let code = match code {
        Some("api_error") => " code=api_error",
        Some("service_unavailable") => " code=service_unavailable",
        _ => "",
    };
    if let Some(ms) = retry_after_ms(headers) {
        eprintln!(
            "[Gemini][diag] unavailable source=http status={}{} retry_after_ms={ms}",
            status.as_u16(),
            code
        );
    } else {
        eprintln!(
            "[Gemini][diag] unavailable source=http status={}{}",
            status.as_u16(),
            code
        );
    }
}
#[cfg(debug_assertions)]
fn rate_limit_diagnostic(
    status: StatusCode,
    code: Option<&str>,
    retry_after_ms: Option<u64>,
) -> String {
    // Keep response-provided strings out of diagnostics unless explicitly allowlisted.
    let code = match code {
        Some("rate_limit_exceeded") => "rate_limit_exceeded",
        Some("too_many_requests") => "too_many_requests",
        _ => "none",
    };
    let retry_after_ms = retry_after_ms.map_or_else(|| "none".to_owned(), |ms| ms.to_string());
    format!("[Gemini][diag] rate_limited source=http status={} code={code} retry_after_ms={retry_after_ms}", status.as_u16())
}
#[cfg(debug_assertions)]
fn diag_http_rate_limited(status: StatusCode, code: Option<&str>, result: &ProviderError) {
    if let ProviderError::RateLimited { retry_after_ms } = result {
        eprintln!("{}", rate_limit_diagnostic(status, code, *retry_after_ms));
    }
}
#[cfg(debug_assertions)]
fn diag_stream_unavailable(code: &str, result: &ProviderError) {
    if matches!(result, ProviderError::Unavailable { .. }) {
        // Keep the diagnostic allowlisted even if the mapper gains new codes later.
        let code = match code {
            "api_error" => "api_error",
            "service_unavailable" => "service_unavailable",
            _ => "other",
        };
        eprintln!("[Gemini][diag] unavailable source=stream code={code}");
    }
}
async fn http_error(response: &mut reqwest::Response, cancelled: &AtomicBool) -> ProviderError {
    const MAX_ERROR_BODY: usize = 64 * 1024;
    let status = response.status();
    let headers = response.headers().clone();
    let fallback = || GeminiProvider::classify(status, &headers);
    let mut body = Vec::new();
    loop {
        let next = tokio::select! {
          _ = cancellation(cancelled) => return ProviderError::Cancelled,
          result = response.chunk() => result,
        };
        match next {
            Ok(Some(chunk)) if body.len().saturating_add(chunk.len()) <= MAX_ERROR_BODY => {
                body.extend_from_slice(&chunk)
            }
            Ok(None) => break,
            _ => {
                let result = fallback();
                #[cfg(debug_assertions)]
                diag_http_unavailable(status, &headers, None, &result);
                #[cfg(debug_assertions)]
                diag_http_rate_limited(status, None, &result);
                return result;
            }
        }
    }
    let parsed = serde_json::from_slice::<Value>(&body).ok();
    let code = parsed.as_ref().and_then(error_code);
    let result = code
        .map(|code| classify_error_code(code, retry_after_ms(&headers)))
        .unwrap_or_else(fallback);
    // A structured timeout on a 5xx still carries the server's retry window.
    let result = if status.is_server_error()
        && retry_after_ms(&headers).is_some()
        && matches!(
            result,
            ProviderError::Timeout | ProviderError::Unavailable { .. }
        ) {
        ProviderError::Unavailable {
            retry_after_ms: retry_after_ms(&headers),
        }
    } else {
        result
    };
    #[cfg(debug_assertions)]
    diag_http_unavailable(status, &headers, code, &result);
    #[cfg(debug_assertions)]
    diag_http_rate_limited(status, code, &result);
    result
}
fn retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    const MAX_RETRY_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1000;
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1000).min(MAX_RETRY_AFTER_MS));
    }
    httpdate::parse_http_date(value)
        .ok()
        .and_then(|date| date.duration_since(SystemTime::now()).ok())
        .map(|duration| duration.as_millis().min(MAX_RETRY_AFTER_MS as u128) as u64)
}
fn network_error(error: &reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        #[cfg(debug_assertions)]
        eprintln!(
            "[Gemini][diag] unavailable source=network timeout={} connect={} request={}",
            error.is_timeout(),
            error.is_connect(),
            error.is_request()
        );
        ProviderError::Unavailable {
            retry_after_ms: None,
        }
    }
}
async fn cancellation(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
impl Provider for GeminiProvider {
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
            if request.target.provider_id != "gemini" || !request.target.invocation.valid() {
                return Err(ProviderError::InvalidRequest);
            }
            let secrets = self.secrets.clone();
            let key = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = tauri::async_runtime::spawn_blocking(move || secrets.get_secret(SecretKey::GeminiApiKey)) =>
                result.map_err(|_| {
                  #[cfg(debug_assertions)] eprintln!("[Gemini][diag] unavailable source=secret_store code=task_join_failed");
                  ProviderError::Unavailable { retry_after_ms: None }
                })?.map_err(|error| {
                  #[cfg(debug_assertions)] eprintln!("[Gemini][diag] unavailable source=secret_store code={}", error.code());
                  #[cfg(not(debug_assertions))] let _ = error;
                  ProviderError::Unavailable { retry_after_ms: None }
                })?.ok_or(ProviderError::Authentication)?,
            };
            let key = HeaderValue::from_bytes(&key).map_err(|_| ProviderError::Authentication)?;
            let payload = MinimalOutboundContext::from_bundle(&request.context)?.payload(
                &request.target.invocation.model,
                &request.input,
                &request.history,
                request.max_output_tokens,
                request.target.invocation.thinking_level,
            );
            let timeouts = request
                .target
                .invocation
                .timeouts
                .unwrap_or_else(|| *self.timeouts.read().unwrap_or_else(|p| p.into_inner()));
            let send = self
                .client
                .post(&self.config.endpoint)
                .timeout(Duration::from_millis(timeouts.request_timeout_ms as u64))
                .header("x-goog-api-key", key)
                .json(&payload)
                .send();
            let mut response = tokio::select! {
              _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
              result = send => result.map_err(|e| network_error(&e))?,
            };
            if !response.status().is_success() {
                return Err(http_error(&mut response, cancelled).await);
            }
            let mut parser = SseParser::default();
            let mut text = String::new();
            let mut usage = None;
            'stream: loop {
                let next = tokio::select! {
                  _ = cancellation(cancelled) => return Err(ProviderError::Cancelled),
                  result = tokio::time::timeout(Duration::from_millis(timeouts.stream_idle_timeout_ms as u64), response.chunk()) =>
                    result.map_err(|_| ProviderError::Timeout)?.map_err(|e| network_error(&e))?,
                };
                let Some(bytes) = next else { break };
                for event in parser.push(&bytes)? {
                    match event {
                        StreamEvent::Text(piece) => {
                            text.push_str(&piece);
                            on_chunk(ProviderChunk { text: piece })?;
                        }
                        StreamEvent::Completed(result) => usage = Some(result?),
                        StreamEvent::Done => break 'stream,
                        StreamEvent::Error(error) => return Err(error),
                        StreamEvent::Ignore => {}
                    }
                }
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(ProviderError::Cancelled);
            }
            let usage = usage.ok_or(ProviderError::Protocol)?;
            if text.trim().is_empty() {
                return Err(ProviderError::Fatal);
            }
            Ok(ProviderResponse { text, usage })
        })
    }
}

enum StreamEvent {
    Text(String),
    Completed(Result<ProviderUsage, ProviderError>),
    Done,
    Error(ProviderError),
    Ignore,
}
#[derive(Default)]
struct SseParser {
    pending: Vec<u8>,
    event: Option<String>,
    data: String,
    model_steps: HashSet<u64>,
}
impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > 1_048_576 {
            return Err(ProviderError::Fatal);
        }
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let mut line = self.pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = std::str::from_utf8(&line).map_err(|_| ProviderError::Fatal)?;
            if line.is_empty() {
                if let Some(event) = self.finish()? {
                    events.push(event);
                }
            } else if let Some(value) = line.strip_prefix("event:") {
                self.event = Some(value.trim().into());
            } else if let Some(value) = line.strip_prefix("data:") {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value.trim_start());
            }
            if self.data.len() > 1_048_576 {
                return Err(ProviderError::Fatal);
            }
        }
        Ok(events)
    }
    fn finish(&mut self) -> Result<Option<StreamEvent>, ProviderError> {
        let event = self.event.take().unwrap_or_default();
        let data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return Ok(None);
        }
        if data == "[DONE]" {
            return Ok(Some(StreamEvent::Done));
        }
        if !matches!(
            event.as_str(),
            "step.start" | "step.delta" | "step.stop" | "interaction.completed" | "error"
        ) {
            return Ok(Some(StreamEvent::Ignore));
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| ProviderError::Fatal)?;
        Ok(Some(match event.as_str() {
            "step.start" => {
                if value.pointer("/step/type").and_then(Value::as_str) == Some("model_output") {
                    if let Some(index) = value.get("index").and_then(Value::as_u64) {
                        self.model_steps.insert(index);
                    }
                }
                StreamEvent::Ignore
            }
            "step.stop" => {
                if let Some(index) = value.get("index").and_then(Value::as_u64) {
                    self.model_steps.remove(&index);
                }
                StreamEvent::Ignore
            }
            "step.delta"
                if value
                    .get("index")
                    .and_then(Value::as_u64)
                    .is_some_and(|index| self.model_steps.contains(&index))
                    && value.pointer("/delta/type").and_then(Value::as_str) == Some("text") =>
            {
                StreamEvent::Text(
                    value
                        .pointer("/delta/text")
                        .and_then(Value::as_str)
                        .ok_or(ProviderError::Fatal)?
                        .into(),
                )
            }
            "interaction.completed" => {
                let status = value.pointer("/interaction/status").and_then(Value::as_str);
                let result = match status {
                    Some("completed") => {
                        let raw = value
                            .pointer("/interaction/usage")
                            .ok_or(ProviderError::Protocol)?;
                        let count = |field| {
                            raw.get(field)
                                .and_then(Value::as_u64)
                                .and_then(|n| u32::try_from(n).ok())
                        };
                        Ok(ProviderUsage {
                            calls: 1,
                            input_tokens: count("total_input_tokens")
                                .ok_or(ProviderError::Protocol)?,
                            output_tokens: count("total_output_tokens")
                                .ok_or(ProviderError::Protocol)?,
                            total_tokens: Some(
                                count("total_tokens").ok_or(ProviderError::Protocol)?,
                            ),
                            thought_tokens: count("total_thought_tokens"),
                        })
                    }
                    Some("incomplete") => {
                        #[cfg(debug_assertions)]
                        eprintln!("[Gemini][diag] incomplete");
                        Err(ProviderError::Incomplete)
                    }
                    Some("requires_action") => Err(ProviderError::RequiresAction),
                    Some("cancelled") => Err(ProviderError::RemoteCancelled),
                    Some("failed") => Err(value
                        .pointer("/interaction/errors")
                        .and_then(Value::as_array)
                        .and_then(|errors| {
                            errors.iter().find_map(|error| {
                                error
                                    .get("code")
                                    .and_then(Value::as_str)
                                    .filter(|code| !code.is_empty())
                            })
                        })
                        .map(|code| {
                            let result = classify_error_code(code, None);
                            #[cfg(debug_assertions)]
                            diag_stream_unavailable(code, &result);
                            result
                        })
                        .unwrap_or(ProviderError::Fatal)),
                    _ => Err(ProviderError::Protocol),
                };
                StreamEvent::Completed(result)
            }
            "error" => StreamEvent::Error(
                error_code(&value)
                    .map(|code| {
                        let result = classify_error_code(code, None);
                        #[cfg(debug_assertions)]
                        diag_stream_unavailable(code, &result);
                        result
                    })
                    .unwrap_or(ProviderError::Protocol),
            ),
            _ => StreamEvent::Ignore,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        registry::ProviderRegistry,
        scheduler::{Scheduler, SchedulerEvent},
        types::{ContextMetadata, ProviderCapabilities, ProviderConfig, TaskBudget},
    };
    use super::*;
    use crate::{
        persistence::{
            conversation::ConversationMessage, identity::IdentityInput, memory::MemoryRecord,
        },
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
        let dir = std::env::temp_dir().join(format!("gemini-test-{}-{n}", std::process::id()));
        let store = Arc::new(SecretStore::with_key_store(
            dir.clone(),
            Arc::new(Keys::default()),
        ));
        store
            .set_secret(SecretKey::GeminiApiKey, b"fake-secret-token")
            .unwrap();
        (store, dir)
    }
    fn bundle() -> Arc<ContextBundle> {
        let identity: IdentityInput=serde_json::from_value(json!({
      "version":"v1","canonicalName":"Luna","presentation":"private","primaryLanguage":"pt-BR","concept":"private",
      "traits":{"private":"relationship secret marker"},"behavioralInvariants":["secret"],"modes":{},
      "relationship":{"primaryPersonName":"relationship secret marker","relationModes":[],
        "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
        "interactionPreferences":{"wantsRealDisagreement":true,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":true}},
      "memoryPolicy":{"retrieval":"private","history":"private","continuity":"private","storePrivateChainOfThought":false},
      "provenance":"private","effectiveFrom":"2026-01-01"})).unwrap();
        Arc::new(ContextBundle {
            identity,
            relevant_memories: vec![MemoryRecord {
                id: 1,
                import_key: None,
                kind: "project".into(),
                domains: vec![],
                state: "active".into(),
                title: "memory secret marker".into(),
                summary: "memory secret marker".into(),
                content: None,
                retrieval_hint: Some("memory secret marker".into()),
                source_context: Some("memory secret marker".into()),
                importance: 1,
                confidence: "high".into(),
                event_date: None,
                created_at: "now".into(),
                updated_at: "now".into(),
                supersedes_id: None,
            }],
            recent_messages: vec![ConversationMessage {
                id: 1,
                session_id: 1,
                role: "user".into(),
                content: "recent private marker".into(),
                created_at: "now".into(),
            }],
            metadata: ContextMetadata {
                identity_version: "v1".into(),
                memory_count: 1,
                recent_message_count: 1,
            },
        })
    }
    use crate::cognition::types::{ProviderInvocationConfig, ProviderTarget, ProviderTaskRequest};
    fn request() -> ProviderRequest {
        ProviderRequest {
            input: "Quanto é 2 + 2?".into(),
            history: vec![],
            context: bundle(),
            max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
            target: ProviderTarget {
                provider_id: "gemini".into(),
                invocation: ProviderInvocationConfig {
                    model: MODEL.into(),
                    thinking_level: Some(ThinkingLevel::Low),
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
            selection: crate::cognition::types::ProviderSelection::Fixed("gemini".into()),
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
            "http://{}/v1beta/interactions",
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
            let response=format!("HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n{extra}\r\n{body}",body.len());
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
    const SSE:&str="event: interaction.created\ndata: {\"event_type\":\"interaction.created\"}\n\nevent: step.start\ndata: {\"index\":0,\"step\":{\"type\":\"thought\"}}\n\nevent: step.delta\ndata: {\"index\":0,\"delta\":{\"type\":\"thought_signature\",\"signature\":\"private\"}}\n\nevent: step.start\ndata: {\"index\":1,\"step\":{\"type\":\"model_output\"}}\n\nevent: step.delta\ndata: {\"index\":1,\"delta\":{\"type\":\"text\",\"text\":\"Quatro\"}}\n\nevent: future.event\ndata: {\"anything\":true}\n\nevent: step.delta\ndata: {\"index\":1,\"delta\":{\"type\":\"text\",\"text\":\".\"}}\n\nevent: interaction.completed\ndata: {\"interaction\":{\"status\":\"completed\",\"usage\":{\"total_input_tokens\":20,\"total_output_tokens\":2,\"total_tokens\":30,\"total_thought_tokens\":8}}}\n\nevent: done\ndata: [DONE]\n\n";
    #[test]
    fn provider_defaults_are_omitted_and_explicit_values_preserved() {
        let outbound = MinimalOutboundContext::from_bundle(&bundle()).unwrap();
        let defaults = outbound.payload("gemini-custom", "Oi", &[], None, None);
        assert_eq!(defaults["model"], "gemini-custom");
        assert!(defaults["generation_config"]
            .get("max_output_tokens")
            .is_none());
        assert!(defaults["generation_config"]
            .get("thinking_level")
            .is_none());
        assert_eq!(defaults["store"], false);
        let system = defaults["system_instruction"].as_str().unwrap();
        assert!(system.contains("provider cognitivo=Gemini (id gemini)"));
        assert!(system.contains("gemini-custom"));
        assert!(system.contains("não infira pelo histórico"));
        let explicit = outbound.payload(
            "gemini-custom",
            "Oi",
            &[],
            Some(8192),
            Some(ThinkingLevel::High),
        );
        assert_eq!(explicit["generation_config"]["max_output_tokens"], 8192);
        assert_eq!(explicit["generation_config"]["thinking_level"], "high");
    }
    #[test]
    fn request_privacy_stream_usage_and_secret_lifecycle() {
        let (store, dir) = fixture();
        assert!(store.get_secret(SecretKey::GeminiApiKey).unwrap().is_some());
        let (url, handle) = server("200 OK", SSE, "", true);
        let provider = GeminiProvider::new(
            GeminiConfig {
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
                    id: "gemini".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(provider),
            )
            .unwrap();
        let mut chunks = Vec::new();
        let signal = AtomicBool::new(false);
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            task_request(request()),
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
            },
            &signal,
            &mut |event| {
                if let SchedulerEvent::Chunk { text, .. } = event {
                    chunks.push(text)
                }
                Ok(())
            },
        ))
        .unwrap();
        assert_eq!(chunks, vec!["Quatro", "."]);
        assert_eq!(result.text, "Quatro.");
        assert_eq!(result.usage.total_tokens, Some(30));
        assert_eq!(result.usage.thought_tokens, Some(8));
        let raw = handle.join().unwrap();
        let (_, body) = raw.split_once("\r\n\r\n").unwrap();
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["generation_config"]["thinking_level"], "low");
        assert_eq!(payload["store"], false);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["model"], MODEL);
        assert_eq!(payload["generation_config"]["max_output_tokens"], 4096);
        assert!(payload.get("previous_interaction_id").is_none());
        assert_eq!(payload["input"], "Quanto é 2 + 2?");
        for marker in [
            "relationship secret marker",
            "memory secret marker",
            "recent private marker",
            "fake-secret-token",
        ] {
            assert!(!body.contains(marker));
        }
        assert!(raw
            .to_ascii_lowercase()
            .contains("x-goog-api-key: fake-secret-token"));
        store.delete_secret(SecretKey::GeminiApiKey).unwrap();
        assert!(store.get_secret(SecretKey::GeminiApiKey).unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn scheduler_uses_smaller_request_or_budget_limit() {
        let (store, dir) = fixture();
        for (request_limit, budget_limit, expected) in [(4096, 1024, 1024), (1024, 4096, 1024)] {
            let (url, handle) = server("200 OK", SSE, "", false);
            let provider = GeminiProvider::new(
                GeminiConfig {
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
                        id: "gemini".into(),
                        enabled: true,
                        priority: 1,
                        capabilities: ProviderCapabilities::text_stream(),
                    },
                    Arc::new(provider),
                )
                .unwrap();
            let mut chat_request = request();
            chat_request.max_output_tokens = Some(request_limit);
            let signal = AtomicBool::new(false);
            tauri::async_runtime::block_on(Scheduler::new(registry).run(
                task_request(chat_request),
                TaskBudget {
                    max_provider_calls: 1,
                    max_output_tokens: Some(budget_limit),
                },
                &signal,
                &mut |_| Ok(()),
            ))
            .unwrap();
            let raw = handle.join().unwrap();
            let (_, body) = raw.split_once("\r\n\r\n").unwrap();
            let payload: Value = serde_json::from_str(body).unwrap();
            assert_eq!(payload["generation_config"]["max_output_tokens"], expected);
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn explicit_session_history_reaches_http_without_other_sessions_or_private_context() {
        use crate::persistence::{conversation, database::Database};
        let (store, dir) = fixture();
        let db = Database::for_test(dir.join("multi-turn.sqlite3"));
        let mut conn = db.open().unwrap();
        let a = conversation::create_session(&conn).unwrap();
        let b = conversation::create_session(&conn).unwrap();
        conversation::append_exchange_to_session(
            &mut conn,
            a,
            "O código desta sessão é LARANJA-42.",
            "Entendido.",
        )
        .unwrap();
        conversation::append_exchange_to_session(
            &mut conn,
            b,
            "HISTORICO-SECRETO-55",
            "Entendido.",
        )
        .unwrap();
        conversation::close_session(&conn, b).unwrap();
        assert!(conversation::list_history(&conn, 50)
            .unwrap()
            .iter()
            .any(|item| item.id == b));
        assert_eq!(
            conversation::history_session(&conn, b)
                .unwrap()
                .unwrap()
                .messages[0]
                .content,
            "HISTORICO-SECRETO-55"
        );
        conversation::append_gemini_exchange(&mut conn, "MARCADOR-LR-6", "Entendido.").unwrap();
        conversation::create_diagnostic(&mut conn).unwrap();
        let history = conversation::outbound_history(
            &conn,
            a,
            conversation::OUTBOUND_HISTORY_MESSAGES,
            conversation::OUTBOUND_HISTORY_BYTES,
        )
        .unwrap();
        assert_eq!(history.len(), 2);
        let (url, handle) = server("200 OK", SSE, "", false);
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let mut req = request();
        req.input = "Qual é o código desta sessão?".into();
        req.history = history
            .into_iter()
            .map(|turn| ProviderMessage {
                role: match turn.role {
                    conversation::SessionRole::User => ProviderRole::User,
                    conversation::SessionRole::Assistant => ProviderRole::Assistant,
                },
                content: turn.content,
            })
            .collect();
        let signal = AtomicBool::new(false);
        assert!(
            tauri::async_runtime::block_on(provider.execute(&req, &signal, &mut |_| Ok(())))
                .is_ok()
        );
        let raw = handle.join().unwrap();
        let (_, body) = raw.split_once("\r\n\r\n").unwrap();
        let payload: Value = serde_json::from_str(body).unwrap();
        let steps = payload["input"].as_array().unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[0],
            json!({"type":"user_input","content":[{"type":"text","text":"O código desta sessão é LARANJA-42."}]})
        );
        assert_eq!(
            steps[1],
            json!({"type":"model_output","content":[{"type":"text","text":"Entendido."}]})
        );
        assert_eq!(
            steps[2],
            json!({"type":"user_input","content":[{"type":"text","text":"Qual é o código desta sessão?"}]})
        );
        assert_eq!(body.matches("Qual é o código desta sessão?").count(), 1);
        for marker in [
            "HISTORICO-SECRETO-55",
            "memory secret marker",
            "recent private marker",
            "MARCADOR-LR-6",
            "Mensagem de diagnóstico LR-4",
        ] {
            assert!(
                !body.contains(marker),
                "unexpected outbound marker {marker}"
            );
        }
        assert_eq!(payload["store"], false);
        drop(conn);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn resumed_session_is_the_only_history_in_fake_http_outbound() {
        use crate::cognition::gemini_commands::{self, CurrentRunSessions};
        use crate::luna::runtime::TaskRegistry;
        use crate::persistence::{conversation, database::Database};
        let (store, dir) = fixture();
        let db = Database::for_test(dir.join("resume-http.sqlite3"));
        let mut conn = db.open().unwrap();
        let a = conversation::create_session(&conn).unwrap();
        let b = conversation::create_session(&conn).unwrap();
        let c = conversation::create_session(&conn).unwrap();
        conversation::append_exchange_to_session(&mut conn, a, "ALFA-A-11", "Entendido.").unwrap();
        conversation::append_exchange_to_session(&mut conn, b, "BETA-B-22", "Entendido.").unwrap();
        conversation::append_exchange_to_session(&mut conn, c, "GAMA-C-33", "Entendido.").unwrap();
        conversation::close_session(&conn, a).unwrap();
        conversation::close_session(&conn, b).unwrap();
        conversation::close_session(&conn, c).unwrap();
        conn.execute(
            "UPDATE conversation_sessions SET summary_status='running' WHERE id=?1",
            [a],
        )
        .unwrap();
        assert!(conversation::complete_summary(&conn, a, "Título A", "SUMMARY-ALFA-A-11").unwrap());
        conversation::append_gemini_exchange(&mut conn, "LEGACY-55", "Entendido.").unwrap();
        conversation::create_diagnostic(&mut conn).unwrap();
        let sessions = CurrentRunSessions::default();
        gemini_commands::resume_registered_session(
            &db,
            &sessions,
            &TaskRegistry::default(),
            b,
            None,
        )
        .unwrap();
        assert_eq!(
            *sessions.0.lock().unwrap(),
            std::collections::HashSet::from([b])
        );
        let history = conversation::outbound_history(
            &conn,
            b,
            conversation::OUTBOUND_HISTORY_MESSAGES,
            conversation::OUTBOUND_HISTORY_BYTES,
        )
        .unwrap();
        assert_eq!(history.len(), 2);
        let (url, handle) = server("200 OK", SSE, "", false);
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let mut req = request();
        req.input = "Continue BETA-B-22".into();
        Arc::get_mut(&mut req.context).unwrap().relevant_memories[0].summary = "PRIVATE-44".into();
        req.history = history
            .into_iter()
            .map(|turn| ProviderMessage {
                role: match turn.role {
                    conversation::SessionRole::User => ProviderRole::User,
                    conversation::SessionRole::Assistant => ProviderRole::Assistant,
                },
                content: turn.content,
            })
            .collect();
        let signal = AtomicBool::new(false);
        assert!(
            tauri::async_runtime::block_on(provider.execute(&req, &signal, &mut |_| Ok(())))
                .is_ok()
        );
        let raw = handle.join().unwrap();
        let (_, body) = raw.split_once("\r\n\r\n").unwrap();
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["input"].as_array().unwrap().len(), 3);
        assert_eq!(body.matches("Continue BETA-B-22").count(), 1);
        assert!(body.contains("BETA-B-22"));
        for marker in [
            "ALFA-A-11",
            "GAMA-C-33",
            "PRIVATE-44",
            "LEGACY-55",
            "SUMMARY-ALFA-A-11",
            "memory secret marker",
            "recent private marker",
        ] {
            assert!(!body.contains(marker), "unexpected {marker}");
        }
        drop(conn);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn http_error_classes_and_retry_after() {
        let (store, dir) = fixture();
        for (status, extra, expected) in [
            (
                "429 Too Many Requests",
                "Retry-After: 3\r\n",
                ProviderError::RateLimited {
                    retry_after_ms: Some(3000),
                },
            ),
            ("401 Unauthorized", "", ProviderError::Authentication),
            ("403 Forbidden", "", ProviderError::Authentication),
            ("408 Request Timeout", "", ProviderError::Timeout),
            ("504 Gateway Timeout", "", ProviderError::Timeout),
            ("402 Payment Required", "", ProviderError::Fatal),
            (
                "500 Internal Server Error",
                "",
                ProviderError::Unavailable {
                    retry_after_ms: None,
                },
            ),
            (
                "503 Service Unavailable",
                "Retry-After: 30\r\n",
                ProviderError::Unavailable {
                    retry_after_ms: Some(30_000),
                },
            ),
            (
                "503 Service Unavailable",
                "",
                ProviderError::Unavailable {
                    retry_after_ms: None,
                },
            ),
        ] {
            let (url, handle) = server(status, "", extra, false);
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            let signal = AtomicBool::new(false);
            let result =
                tauri::async_runtime::block_on(
                    provider.execute(&request(), &signal, &mut |_| Ok(())),
                );
            assert_eq!(result.unwrap_err(), expected);
            let raw = handle.join().unwrap();
            assert!(!raw.contains("relationship secret marker"));
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rate_limit_diagnostic_allowlists_code_and_excludes_private_fields() {
        let private = "private prompt key history identity memory response";
        assert_eq!(rate_limit_diagnostic(StatusCode::TOO_MANY_REQUESTS,Some("rate_limit_exceeded"),Some(3000)),
      "[Gemini][diag] rate_limited source=http status=429 code=rate_limit_exceeded retry_after_ms=3000");
        assert_eq!(rate_limit_diagnostic(StatusCode::TOO_MANY_REQUESTS,Some("too_many_requests"),None),
      "[Gemini][diag] rate_limited source=http status=429 code=too_many_requests retry_after_ms=none");
        let diagnostic =
            rate_limit_diagnostic(StatusCode::TOO_MANY_REQUESTS, Some(private), Some(2000));
        assert_eq!(
            diagnostic,
            "[Gemini][diag] rate_limited source=http status=429 code=none retry_after_ms=2000"
        );
        assert!(!diagnostic.contains(private));
    }
    #[test]
    fn unavailable_diagnostics_leave_public_and_persisted_error_sanitized() {
        use crate::persistence::{database::Database, task_history};
        let (store, dir) = fixture();
        let private_message = "private Google detail and fake-secret-token";
        let body =
            json!({"error":{"code":"service_unavailable","message":private_message}}).to_string();
        let (url, handle) = server(
            "503 Service Unavailable",
            &body,
            "Retry-After: 2\r\n",
            false,
        );
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let signal = AtomicBool::new(false);
        let error =
            tauri::async_runtime::block_on(provider.execute(&request(), &signal, &mut |_| Ok(())))
                .unwrap_err();
        assert_eq!(
            error,
            ProviderError::Unavailable {
                retry_after_ms: Some(2_000)
            }
        );
        assert_eq!(error.code(), "unavailable");
        handle.join().unwrap();
        let db = Database::for_test(dir.join("diagnostic.sqlite3"));
        let conn = db.open().unwrap();
        task_history::insert(
            &conn,
            &task_history::TaskRecord {
                task_id: 1,
                kind: "gemini_chat".into(),
                state: "failed".into(),
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: "2026-01-01T00:00:01Z".into(),
                summary: None,
                error_code: Some(error.code().into()),
            },
        )
        .unwrap();
        let persisted: String = conn
            .query_row(
                "SELECT error_code FROM task_records WHERE task_id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, "unavailable");
        assert!(!persisted.contains(private_message));
        assert!(!persisted.contains(&body));
        drop(conn);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn network_failure_remains_unavailable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v1beta/interactions",
            listener.local_addr().unwrap()
        );
        drop(listener);
        let error = tauri::async_runtime::block_on(Client::new().get(url).send()).unwrap_err();
        assert!(error.is_connect());
        assert_eq!(
            network_error(&error),
            ProviderError::Unavailable {
                retry_after_ms: None
            }
        );
        assert_eq!(network_error(&error).code(), "unavailable");
    }
    #[test]
    fn http_error_code_takes_priority_and_fallback_is_bounded() {
        let (store, dir) = fixture();
        for (status, code, header, expected) in [
            (
                "429 Too Many Requests",
                "rate_limit_exceeded",
                "Retry-After: 3\r\n",
                ProviderError::RateLimited {
                    retry_after_ms: Some(3000),
                },
            ),
            (
                "429 Too Many Requests",
                "quota_exceeded",
                "",
                ProviderError::QuotaExceeded,
            ),
            (
                "429 Too Many Requests",
                "too_many_requests",
                "",
                ProviderError::RateLimited {
                    retry_after_ms: None,
                },
            ),
            (
                "401 Unauthorized",
                "authentication",
                "",
                ProviderError::Authentication,
            ),
            (
                "403 Forbidden",
                "permission_denied",
                "",
                ProviderError::Authentication,
            ),
            (
                "503 Service Unavailable",
                "service_unavailable",
                "",
                ProviderError::Unavailable {
                    retry_after_ms: None,
                },
            ),
            (
                "504 Gateway Timeout",
                "deadline_exceeded",
                "",
                ProviderError::Timeout,
            ),
            (
                "503 Service Unavailable",
                "invalid_request",
                "",
                ProviderError::InvalidRequest,
            ),
            (
                "500 Internal Server Error",
                "unknown_future_code",
                "",
                ProviderError::Fatal,
            ),
        ] {
            let body = json!({"error":{"code":code,"message":"private Google detail"}}).to_string();
            let (url, handle) = server(status, &body, header, false);
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            let signal = AtomicBool::new(false);
            assert_eq!(
                tauri::async_runtime::block_on(
                    provider.execute(&request(), &signal, &mut |_| Ok(()))
                )
                .unwrap_err(),
                expected
            );
            handle.join().unwrap();
        }
        for body in ["not JSON".to_owned(), "x".repeat(65_537)] {
            let (url, handle) = server("429 Too Many Requests", &body, "Retry-After: 2\r\n", false);
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            let signal = AtomicBool::new(false);
            assert_eq!(
                tauri::async_runtime::block_on(
                    provider.execute(&request(), &signal, &mut |_| Ok(()))
                )
                .unwrap_err(),
                ProviderError::RateLimited {
                    retry_after_ms: Some(2000)
                }
            );
            handle.join().unwrap();
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn official_error_codes_fail_closed_except_explicit_transients() {
        for code in [
            "failed_precondition",
            "out_of_range",
            "parameter_unknown",
            "already_exists",
            "aborted",
            "unimplemented",
            "safety",
            "recitation",
            "language",
            "prohibited_content",
            "spii",
            "blocklist",
            "image_safety",
            "image_prohibited_content",
            "image_recitation",
            "image_other",
            "content_blocked",
            "malformed_function_call",
            "malformed_tool_call",
            "unexpected_tool_call",
            "no_image",
            "too_many_tool_calls",
            "missing_thought_signature",
            "future_code",
        ] {
            assert_eq!(
                classify_error_code(code, None),
                ProviderError::Fatal,
                "{code}"
            );
        }
        for code in [
            "invalid_argument",
            "invalid_request",
            "not_found",
            "model_not_found",
        ] {
            assert_eq!(
                classify_error_code(code, None),
                ProviderError::InvalidRequest
            );
        }
        assert_eq!(
            classify_error_code("payment_required", None),
            ProviderError::QuotaExceeded
        );
        assert_eq!(
            classify_error_code("api_error", None),
            ProviderError::Unavailable {
                retry_after_ms: None
            }
        );
        assert_eq!(
            classify_error_code("cancelled", None),
            ProviderError::RemoteCancelled
        );
    }
    #[test]
    fn sse_error_codes_share_http_mapper() {
        let (store, dir) = fixture();
        for (code, expected) in [
            (
                "rate_limit_exceeded",
                ProviderError::RateLimited {
                    retry_after_ms: None,
                },
            ),
            (
                "too_many_requests",
                ProviderError::RateLimited {
                    retry_after_ms: None,
                },
            ),
            ("quota_exceeded", ProviderError::QuotaExceeded),
            ("authentication", ProviderError::Authentication),
            ("permission_denied", ProviderError::Authentication),
            ("deadline_exceeded", ProviderError::Timeout),
            ("gateway_timeout", ProviderError::Timeout),
            (
                "service_unavailable",
                ProviderError::Unavailable {
                    retry_after_ms: None,
                },
            ),
            ("invalid_request", ProviderError::InvalidRequest),
            ("cancelled", ProviderError::RemoteCancelled),
            ("unknown_future_code", ProviderError::Fatal),
        ] {
            let body = format!(
                "event: error\ndata: {}\n\n",
                json!({"event_type":"error","error":{"code":code,"message":"private Google detail"}})
            );
            let (url, handle) = server("200 OK", &body, "", false);
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            let signal = AtomicBool::new(false);
            assert_eq!(
                tauri::async_runtime::block_on(
                    provider.execute(&request(), &signal, &mut |_| Ok(()))
                )
                .unwrap_err(),
                expected
            );
            handle.join().unwrap();
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn completion_status_is_required_and_only_completed_succeeds() {
        use crate::persistence::{conversation, database::Database};
        let (store, dir) = fixture();
        let db = Database::for_test(dir.join("terminal.sqlite3"));
        let conn = db.open().unwrap();
        let usage = json!({"total_input_tokens":20,"total_output_tokens":2,"total_tokens":30,"total_thought_tokens":8});
        for (status, expected) in [
            (Some("incomplete"), ProviderError::Incomplete),
            (Some("failed"), ProviderError::Fatal),
            (Some("cancelled"), ProviderError::RemoteCancelled),
            (Some("requires_action"), ProviderError::RequiresAction),
            (Some("in_progress"), ProviderError::Protocol),
            (Some("future_status"), ProviderError::Protocol),
            (None, ProviderError::Protocol),
        ] {
            let mut interaction = json!({"usage":usage});
            if let Some(status) = status {
                interaction["status"] = json!(status)
            }
            let body=format!("event: step.start\ndata: {{\"index\":0,\"step\":{{\"type\":\"model_output\"}}}}\n\nevent: step.delta\ndata: {{\"index\":0,\"delta\":{{\"type\":\"text\",\"text\":\"Resposta parcial\"}}}}\n\nevent: interaction.completed\ndata: {}\n\nevent: done\ndata: [DONE]\n\n",json!({"interaction":interaction}));
            let (url, handle) = server("200 OK", &body, "", true);
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            let signal = AtomicBool::new(false);
            let mut chunks = Vec::new();
            let result = tauri::async_runtime::block_on(provider.execute(
                &request(),
                &signal,
                &mut |chunk| {
                    chunks.push(chunk.text);
                    Ok(())
                },
            ));
            assert_eq!(result.unwrap_err(), expected, "status {status:?}");
            assert_eq!(chunks, vec!["Resposta parcial"]);
            assert!(conversation::gemini_session(&conn).unwrap().is_none());
            handle.join().unwrap();
        }
        let failed = json!({"interaction":{"status":"failed","errors":[{"code":"quota_exceeded","message":"private"}]}});
        let body = format!(
            "event: interaction.completed\ndata: {failed}\n\nevent: done\ndata: [DONE]\n\n"
        );
        let (url, handle) = server("200 OK", &body, "", false);
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap();
        let signal = AtomicBool::new(false);
        assert_eq!(
            tauri::async_runtime::block_on(provider.execute(&request(), &signal, &mut |_| Ok(())))
                .unwrap_err(),
            ProviderError::QuotaExceeded
        );
        handle.join().unwrap();
        for body in [
            "event: done\ndata: [DONE]\n\n".to_owned(),
            "event: interaction.completed\ndata: {\"interaction\":{\"status\":\"completed\"}}\n\n"
                .to_owned(),
        ] {
            let (url, handle) = server("200 OK", &body, "", false);
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            assert_eq!(
                tauri::async_runtime::block_on(
                    provider.execute(&request(), &signal, &mut |_| Ok(()))
                )
                .unwrap_err(),
                ProviderError::Protocol
            );
            handle.join().unwrap();
        }
        drop(conn);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rate_limit_http_date_enters_scheduler_cooldown() {
        let mut extreme = HeaderMap::new();
        extreme.insert(
            RETRY_AFTER,
            HeaderValue::from_static("18446744073709551615"),
        );
        assert_eq!(retry_after_ms(&extreme), Some(7 * 24 * 60 * 60 * 1000));
        let (store, dir) = fixture();
        // Stronghold setup can take several seconds in debug builds, so keep the
        // synthetic HTTP date comfortably ahead of the request.
        let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60));
        let header = format!("Retry-After: {date}\r\n");
        let (url, handle) = server("429 Too Many Requests", "", &header, false);
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "gemini".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(provider),
            )
            .unwrap();
        let scheduler = Scheduler::new(registry);
        let signal = AtomicBool::new(false);
        let result = tauri::async_runtime::block_on(scheduler.run(
            task_request(request()),
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
            },
            &signal,
            &mut |_| Ok(()),
        ));
        assert!(
            matches!(result.unwrap_err(),super::super::types::SchedulerError::Provider(
      ProviderError::RateLimited {retry_after_ms:Some(ms)} ) if ms>0 && ms<=60_000)
        );
        assert!(scheduler.status()[0].cooldown_ms > 0);
        let next = tauri::async_runtime::block_on(scheduler.run(
            task_request(request()),
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
            },
            &signal,
            &mut |_| Ok(()),
        ));
        let no_provider = next.unwrap_err();
        assert_eq!(no_provider, super::super::types::SchedulerError::NoProvider);
        assert_eq!(no_provider.code(), "provider_unavailable");
        handle.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn parser_never_emits_thought_or_unknown_delta() {
        let mut parser = SseParser::default();
        let mut text = Vec::new();
        for byte in SSE.as_bytes().chunks(3) {
            for event in parser.push(byte).unwrap() {
                if let StreamEvent::Text(piece) = event {
                    text.push(piece)
                }
            }
        }
        assert_eq!(text, vec!["Quatro", "."]);
    }
    #[test]
    fn event_sink_closed_stops_stream_without_retry() {
        let (store, dir) = fixture();
        let (url, handle) = server("200 OK", SSE, "", true);
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "gemini".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(provider),
            )
            .unwrap();
        let signal = AtomicBool::new(false);
        let mut chunks = 0;
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            task_request(request()),
            TaskBudget {
                max_provider_calls: 2,
                max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
            },
            &signal,
            &mut |event| {
                if matches!(event, SchedulerEvent::Chunk { .. }) {
                    chunks += 1;
                    return Err(super::super::types::SchedulerError::EventSinkClosed);
                }
                Ok(())
            },
        ));
        assert_eq!(
            result.unwrap_err(),
            super::super::types::SchedulerError::EventSinkClosed
        );
        assert_eq!(chunks, 1);
        assert!(signal.load(Ordering::Acquire));
        handle.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn idle_timeout_and_cancel_interrupt_http_stream() {
        let (store, dir) = fixture();
        for cancel in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!(
                "http://{}/v1beta/interactions",
                listener.local_addr().unwrap()
            );
            let connected = Arc::new(AtomicBool::new(false));
            let server_connected = connected.clone();
            let handle = thread::spawn(move || {
                let (mut conn, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                let _=conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                server_connected.store(true, Ordering::Release);
                thread::sleep(Duration::from_millis(180));
            });
            let provider = GeminiProvider::new(
                GeminiConfig {
                    endpoint: url,
                    idle_timeout: Duration::from_millis(60),
                    ..Default::default()
                },
                store.clone(),
            )
            .unwrap();
            let signal = Arc::new(AtomicBool::new(false));
            let signal_thread = signal.clone();
            let trigger = if cancel {
                Some(thread::spawn(move || {
                    while !connected.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(2));
                    }
                    thread::sleep(Duration::from_millis(25));
                    signal_thread.store(true, Ordering::Release)
                }))
            } else {
                None
            };
            let result =
                tauri::async_runtime::block_on(
                    provider.execute(&request(), &signal, &mut |_| Ok(())),
                );
            assert_eq!(
                result.unwrap_err(),
                if cancel {
                    ProviderError::Cancelled
                } else {
                    ProviderError::Timeout
                }
            );
            if let Some(trigger) = trigger {
                trigger.join().unwrap()
            }
            handle.join().unwrap();
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn sqlite_context_scheduler_fake_http_and_conversation() {
        use super::super::context::{ContextBuilder, ContextRequest};
        use crate::persistence::{conversation, database::Database, identity};
        let (store, dir) = fixture();
        let db = Database::for_test(dir.join("integration.sqlite3"));
        let mut local = (*bundle()).identity.clone();
        local.modes.insert(
            "default".into(),
            identity::IdentityMode {
                priority: "clear".into(),
                tone: "natural".into(),
            },
        );
        let mut conn = db.open().unwrap();
        let tx = conn.transaction().unwrap();
        identity::insert_version(&tx, &local).unwrap();
        tx.commit().unwrap();
        let context = ContextBuilder::build(
            &conn,
            ContextRequest {
                domain: None,
                kind: None,
                min_importance: 0,
                memory_limit: 0,
                include_recent_conversation: false,
            },
        )
        .unwrap();
        assert_eq!(context.metadata.memory_count, 0);
        let (url, handle) = server("200 OK", SSE, "", true);
        let provider = GeminiProvider::new(
            GeminiConfig {
                endpoint: url,
                ..Default::default()
            },
            store,
        )
        .unwrap();
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "gemini".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(provider),
            )
            .unwrap();
        let signal = AtomicBool::new(false);
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            ProviderTaskRequest {
                input: "Quanto é 2 + 2?".into(),
                history: vec![],
                context: Arc::new(context),
                max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
                selection: crate::cognition::types::ProviderSelection::Fixed("gemini".into()),
                targets: vec![ProviderTarget {
                    provider_id: "gemini".into(),
                    invocation: ProviderInvocationConfig {
                        model: MODEL.into(),
                        thinking_level: Some(ThinkingLevel::Low),
                        timeouts: None,
                    },
                }],
                affinity_key: None,
                estimated_context_bytes: 0,
                required_capabilities: ProviderCapabilities::text_stream(),
            },
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(PROTOTYPE_CHAT_MAX_OUTPUT_TOKENS),
            },
            &signal,
            &mut |_| Ok(()),
        ))
        .unwrap();
        assert_eq!(result.provider_id, "gemini");
        assert_eq!(result.text, "Quatro.");
        conversation::append_gemini_exchange(&mut conn, "Quanto é 2 + 2?", &result.text).unwrap();
        drop(conn);
        let reopened = db.open().unwrap();
        let session = conversation::gemini_session(&reopened).unwrap().unwrap();
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[1].content, "Quatro.");
        let raw = handle.join().unwrap();
        assert!(!raw.contains("relationship secret marker"));
        fs::remove_dir_all(dir).unwrap();
    }
}
