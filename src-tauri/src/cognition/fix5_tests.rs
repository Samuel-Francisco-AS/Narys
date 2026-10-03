use super::{
    cloudflare::{CloudflareConfig, CloudflareProvider},
    groq::{GroqConfig, GroqProvider},
    orchestrator,
    policy::{CognitiveRole, CognitiveRolePolicy, CognitiveTargetPolicy, RoutingMode},
    provider::{Provider, ProviderFuture},
    registry::ProviderRegistry,
    scheduler::{Scheduler, SchedulerEvent},
    transport::{TimeoutPhase, TIMEOUT_DIAGNOSTICS},
    types::*,
};
use crate::{
    agents::planner::{output_schema, PlanV1, MAX_PLAN_BYTES},
    security::secrets::{SecretError, SecretKey, SecretStore, UnlockKeyStore},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[derive(Default)]
struct Keys(Mutex<Option<Vec<u8>>>);
impl UnlockKeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn store(&self, value: &[u8]) -> Result<(), SecretError> {
        *self.0.lock().unwrap() = Some(value.to_vec());
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
struct Fixture {
    store: Arc<SecretStore>,
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "fix5-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Arc::new(SecretStore::with_key_store(
            directory.clone(),
            Arc::new(Keys::default()),
        ));
        store
            .set_secrets(&[
                (
                    SecretKey::GroqApiKey,
                    b"synthetic-credential-marker".to_vec(),
                ),
                (
                    SecretKey::CloudflareApiToken,
                    b"synthetic-credential-marker".to_vec(),
                ),
                (
                    SecretKey::CloudflareAccountId,
                    b"synthetic-account".to_vec(),
                ),
            ])
            .unwrap();
        Self { store, directory }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

enum Exchange {
    Complete(u16, String),
    HeadersDelay,
    ErrorBodyDelay,
    Idle,
    Active,
    TlsStall,
    Oversize,
}
fn read_payload(socket: &mut std::net::TcpStream) -> serde_json::Value {
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let n = socket.read(&mut buffer).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let len = String::from_utf8_lossy(&bytes[..end])
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap();
            if bytes.len() >= end + 4 + len {
                return serde_json::from_slice(&bytes[end + 4..end + 4 + len]).unwrap();
            }
        }
    }
}
fn server(
    exchanges: Vec<Exchange>,
) -> (
    String,
    thread::JoinHandle<Vec<serde_json::Value>>,
    Arc<AtomicBool>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let tail_sent = Arc::new(AtomicBool::new(false));
    let tail = tail_sent.clone();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for exchange in exchanges {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "local fixture did not receive expected call"
                        );
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("local fixture accept failed: {e}"),
                }
            };
            if matches!(exchange, Exchange::TlsStall) {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            requests.push(read_payload(&mut socket));
            match exchange {
                Exchange::Complete(status, body) => {
                    let content_type = if body.starts_with("data:") {
                        "text/event-stream"
                    } else {
                        "application/json"
                    };
                    let head = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = socket.write_all(head.as_bytes());
                    // Exercise application/json parsing across arbitrary UTF-8/wire boundaries.
                    for piece in body.as_bytes().chunks(13) {
                        if socket.write_all(piece).is_err() {
                            break;
                        }
                    }
                }
                Exchange::HeadersDelay => {
                    thread::sleep(Duration::from_millis(200));
                }
                Exchange::ErrorBodyDelay => {
                    let _ = socket.write_all(b"HTTP/1.1 500 Fixture\r\nContent-Length: 200\r\nConnection: close\r\n\r\n{\"error\":\"synthetic-private-output-marker");
                    let _ = socket.flush();
                    thread::sleep(Duration::from_millis(200));
                }
                Exchange::Idle | Exchange::Active => {
                    let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                    let _ = socket.flush();
                    if matches!(exchange, Exchange::Idle) {
                        thread::sleep(Duration::from_millis(200));
                    } else {
                        for _ in 0..25 {
                            let piece = b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"synthetic-private-output-marker\"}}]}\n\n";
                            if write!(socket, "{:x}\r\n", piece.len()).is_err()
                                || socket.write_all(piece).is_err()
                                || socket.write_all(b"\r\n").is_err()
                            {
                                break;
                            }
                            let _ = socket.flush();
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                Exchange::Oversize => {
                    let prefix = "{\"choices\":[{\"message\":{\"content\":\"";
                    let body = format!(
                        "{prefix}{}\"}},\"finish_reason\":\"stop\"}}]}}",
                        "x".repeat(MAX_PLAN_BYTES + 1024)
                    );
                    let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len());
                    let _ = socket.write_all(&body.as_bytes()[..prefix.len() + MAX_PLAN_BYTES + 1]);
                    let _ = socket.flush();
                    thread::sleep(Duration::from_millis(300));
                    tail.store(true, Ordering::Release);
                    let _ = socket.write_all(&body.as_bytes()[prefix.len() + MAX_PLAN_BYTES + 1..]);
                }
                Exchange::TlsStall => unreachable!(),
            }
        }
        requests
    });
    (endpoint, handle, tail_sent)
}
fn mode(limit: usize) -> InvocationMode {
    InvocationMode {
        output: OutputContract::JsonSchema {
            name: "PlanV1".into(),
            schema: output_schema(),
            max_bytes: limit,
        },
        transport: TransportMode::NonStreaming,
    }
}
fn target(provider: &str, timeouts: ProviderTimeouts) -> ProviderTarget {
    ProviderTarget {
        provider_id: provider.into(),
        invocation: ProviderInvocationConfig {
            model: if provider == "groq" {
                super::groq::MODEL
            } else {
                super::cloudflare::MODEL
            }
            .into(),
            thinking_level: None,
            timeouts: Some(timeouts),
        },
    }
}
fn request(provider: &str, mode: InvocationMode, timeouts: ProviderTimeouts) -> ProviderRequest {
    ProviderRequest {
        mode,
        input: "synthetic-private-prompt-marker".into(),
        internal_system_instruction: None,
        history: vec![],
        context: Arc::new(orchestrator::technical_context()),
        max_output_tokens: Some(100),
        target: target(provider, timeouts),
        attempt: 1,
    }
}
fn timeouts() -> ProviderTimeouts {
    ProviderTimeouts {
        request_timeout_ms: 2000,
        stream_idle_timeout_ms: 500,
    }
}
fn completion(text: &str, usage: bool) -> String {
    let mut v = serde_json::json!({"choices":[{"message":{"content":text,"reasoning":"synthetic-private-output-marker"},"finish_reason":"stop"}]});
    if usage {
        v["usage"] =
            serde_json::json!({"prompt_tokens":5,"completion_tokens":10,"total_tokens":15});
    }
    v.to_string()
}
fn plan() -> String {
    serde_json::json!({"version":1,"objective":"Cache desktop","steps":[
        {"id":"vantagens","description":"Analisar vantagens","requiredCapabilities":["planning"],"dependsOn":[]},
        {"id":"riscos","description":"Analisar riscos","requiredCapabilities":["planning"],"dependsOn":[]}],
        "risks":[],"needsUserInput":false,"questions":[]}).to_string()
}
fn scheduler(f: &Fixture, endpoint: String) -> Arc<Scheduler> {
    let mut registry = ProviderRegistry::default();
    registry
        .register(
            ProviderConfig {
                id: "groq".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::with_structured_output(),
            },
            Arc::new(
                GroqProvider::new(
                    GroqConfig {
                        endpoint,
                        ..Default::default()
                    },
                    f.store.clone(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    Arc::new(Scheduler::new(registry))
}
fn policy(calls: u32, tokens: u32) -> CognitiveRolePolicy {
    CognitiveRolePolicy {
        role: CognitiveRole::Orchestrator,
        routing_mode: RoutingMode::Fixed,
        targets: vec![CognitiveTargetPolicy {
            provider_id: "groq".into(),
            model: super::groq::MODEL.into(),
            thinking_level: Some(super::policy::ThinkingLevel::Low),
        }],
        max_output_tokens: Some(tokens),
        max_provider_calls: calls,
        retry_enabled: true,
        max_retries: 1,
        retry_backoff_ms: 0,
        history_max_messages: 0,
        history_max_bytes: 0,
        summary_input_max_bytes: 0,
        context_max_bytes: 8192,
    }
}
fn run_plan(
    scheduler: Arc<Scheduler>,
    policy: CognitiveRolePolicy,
    events: &mut Vec<SchedulerEvent>,
) -> Result<orchestrator::OrchestratorResult, &'static str> {
    tauri::async_runtime::block_on(orchestrator::plan_task_graph(
        scheduler,
        policy,
        "synthetic-private-prompt-marker".into(),
        std::collections::HashMap::from([("groq".into(), timeouts())]),
        &AtomicBool::new(false),
        &mut |e| {
            events.push(e);
            Ok(())
        },
    ))
}

#[test]
fn fix5_structured_groq_payload_uses_real_schema_and_core_validation() {
    let f = Fixture::new();
    let (url, server, _) = server(vec![Exchange::Complete(200, completion(&plan(), true))]);
    let mut events = vec![];
    let result = run_plan(scheduler(&f, url), policy(1, 100), &mut events).unwrap();
    assert!(PlanV1::parse(&serde_json::to_string(&result.plan).unwrap()).is_ok());
    assert_eq!(
        super::task_graph::TaskGraph::compile(&result.plan)
            .unwrap()
            .len(),
        2
    );
    let requests = server.join().unwrap();
    let payload = &requests[0];
    assert_eq!(payload["stream"], false);
    assert!(payload.get("stream_options").is_none());
    assert_eq!(
        payload["response_format"]["json_schema"]["schema"],
        output_schema()
    );
    assert_eq!(payload["response_format"]["json_schema"]["strict"], true);
    assert_eq!(payload["reasoning_effort"], "low");
    assert_eq!(payload["include_reasoning"], false);
    assert!(payload.get("tools").is_none());
    assert!(payload.get("store").is_none());
    assert!(!payload["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("synthetic-private-prompt-marker"));
    assert!(payload["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("synthetic-private-prompt-marker"));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SchedulerEvent::OutputObserved { .. }))
            .count(),
        1
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::Chunk { .. })));
}

#[test]
fn fix5_structured_groq_invalid_syntax_shape_and_semantics_fail_closed() {
    let f = Fixture::new();
    let semantic = plan().replace("\"needsUserInput\":false", "\"needsUserInput\":true");
    let mut unknown: serde_json::Value = serde_json::from_str(&plan()).unwrap();
    unknown["unexpected"] = true.into();
    let cases = vec![
        ("{".into(), "orchestrator_json_syntax_invalid"),
        (
            format!("```json\n{}\n```", plan()),
            "orchestrator_json_syntax_invalid",
        ),
        (
            format!("{} suffix", plan()),
            "orchestrator_json_syntax_invalid",
        ),
        (
            format!("{}{}", plan(), plan()),
            "orchestrator_json_syntax_invalid",
        ),
        (unknown.to_string(), "orchestrator_plan_shape_invalid"),
        (semantic, "orchestrator_plan_semantic_invalid"),
    ];
    for (text, expected) in cases {
        let (url, server, _) = server(vec![Exchange::Complete(200, completion(&text, true))]);
        assert_eq!(
            run_plan(scheduler(&f, url), policy(2, 100), &mut vec![]).unwrap_err(),
            expected
        );
        assert_eq!(server.join().unwrap().len(), 1); // Native format never grants semantic authority/retry.
    }
}

#[test]
fn fix5_capability_mode_rejects_unproven_targets_before_credentials_or_http() {
    let f = Fixture::new();
    let groq = GroqProvider::new(GroqConfig::default(), f.store.clone()).unwrap();
    let cf = CloudflareProvider::new(CloudflareConfig::default(), f.store.clone()).unwrap();
    let structured = mode(MAX_PLAN_BYTES);
    let text = InvocationMode::default();
    assert!(groq.supports_invocation(&target("groq", timeouts()).invocation, &structured));
    assert!(groq.supports_invocation(&target("groq", timeouts()).invocation, &text));
    assert!(!cf.supports_invocation(&target("cloudflare", timeouts()).invocation, &structured));
    assert!(cf.supports_invocation(&target("cloudflare", timeouts()).invocation, &text));
    let mut other = target("groq", timeouts()).invocation;
    other.model = "unproven-model".into();
    assert!(!groq.supports_invocation(&other, &structured));
    let mut stream_schema = structured.clone();
    stream_schema.transport = TransportMode::Streaming;
    assert!(!groq.supports_invocation(&target("groq", timeouts()).invocation, &stream_schema));
    // No credentials: unsupported mode must win before secret access.
    f.store
        .delete_secret(SecretKey::CloudflareApiToken)
        .unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(cf.execute(
            &request("cloudflare", structured, timeouts()),
            &AtomicBool::new(false),
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        ProviderError::UnsupportedMode
    );
}

#[test]
fn fix5_conservative_planner_accounting_real_http_retry_unknown_usage() {
    let f = Fixture::new();
    let (url, server, _) = server(vec![
        Exchange::Complete(408, String::new()),
        Exchange::Complete(200, completion(&plan(), false)),
    ]);
    let mut events = vec![];
    let result = run_plan(scheduler(&f, url), policy(2, 200), &mut events).unwrap();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["max_completion_tokens"], 100);
    assert_eq!(requests[1]["max_completion_tokens"], 100);
    assert_eq!(result.usage.provider_calls, 2);
    assert_eq!(result.usage.retries, 1);
    assert_eq!(result.usage.output_tokens, 0);
    assert!(!result.usage.output_tokens_measured);
    assert_eq!(result.usage.output_tokens_accounted, 200);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SchedulerEvent::Retry { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SchedulerEvent::OutputObserved { .. }))
            .count(),
        1
    );
}

#[test]
fn fix5_conservative_planner_calls_and_output_exhaustion_prevent_extra_http() {
    let f = Fixture::new();
    for (calls, tokens, expected) in [(1, 200, "timeout"), (2, 1, "timeout")] {
        let (url, server, _) = server(vec![Exchange::Complete(408, String::new())]);
        let mut events = vec![];
        assert_eq!(
            run_plan(scheduler(&f, url), policy(calls, tokens), &mut events).unwrap_err(),
            expected
        );
        assert_eq!(server.join().unwrap().len(), 1);
        assert!(!events
            .iter()
            .any(|e| matches!(e, SchedulerEvent::Retry { .. })));
    }
}

#[test]
fn fix5_output_byte_limit_real_http_below_exact_and_early_overflow() {
    let f = Fixture::new();
    for text in ["á😀x", "á😀xy"] {
        let (url, server, _) = server(vec![Exchange::Complete(200, completion(text, false))]);
        let provider = GroqProvider::new(
            GroqConfig {
                endpoint: url,
                ..Default::default()
            },
            f.store.clone(),
        )
        .unwrap();
        let response = tauri::async_runtime::block_on(provider.execute(
            &request("groq", mode(8), timeouts()),
            &AtomicBool::new(false),
            &mut |_| panic!("non-streaming must not emit chunks"),
        ))
        .unwrap();
        assert_eq!(response.text, text);
        server.join().unwrap();
    }
    let (url, server, tail_sent) = server(vec![Exchange::Oversize]);
    let provider = GroqProvider::new(
        GroqConfig {
            endpoint: url,
            ..Default::default()
        },
        f.store.clone(),
    )
    .unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(provider.execute(
            &request("groq", mode(MAX_PLAN_BYTES), timeouts()),
            &AtomicBool::new(false),
            &mut |_| panic!("oversize must not emit chunks")
        ))
        .unwrap_err(),
        ProviderError::OutputLimitExceeded
    );
    assert!(
        !tail_sent.load(Ordering::Acquire),
        "must reject while content is still arriving, before terminal/envelope ends"
    );
    server.join().unwrap();
}

#[test]
fn fix5_http_timeout_phases_are_real_and_diagnostics_sanitized() {
    let f = Fixture::new();
    for provider_id in ["groq", "cloudflare"] {
        let mut cases = vec![
            (Exchange::HeadersDelay, TimeoutPhase::Overall),
            (Exchange::Idle, TimeoutPhase::StreamIdle),
            (Exchange::Active, TimeoutPhase::Overall),
            (
                Exchange::Complete(408, String::new()),
                TimeoutPhase::Http408,
            ),
            (
                Exchange::Complete(504, String::new()),
                TimeoutPhase::Http504,
            ),
            (Exchange::TlsStall, TimeoutPhase::Connect),
        ];
        if provider_id == "cloudflare" {
            // Error-body classification must not swallow a total request timeout.
            cases.push((Exchange::ErrorBodyDelay, TimeoutPhase::Overall));
        }
        for (index, (exchange, expected)) in cases.into_iter().enumerate() {
            let (mut url, server, _) = server(vec![exchange]);
            if expected == TimeoutPhase::Connect {
                url = url.replacen("http:", "https:", 1);
            }
            let timeouts = ProviderTimeouts {
                request_timeout_ms: if expected == TimeoutPhase::Connect {
                    500
                } else {
                    100
                },
                stream_idle_timeout_ms: 45,
            };
            let mut req = request(provider_id, InvocationMode::default(), timeouts);
            req.attempt = 50 + index as u32;
            let provider: Box<dyn Provider> = if provider_id == "groq" {
                Box::new(
                    GroqProvider::new(
                        GroqConfig {
                            endpoint: url,
                            connect_timeout: Duration::from_millis(60),
                            ..Default::default()
                        },
                        f.store.clone(),
                    )
                    .unwrap(),
                )
            } else {
                Box::new(
                    CloudflareProvider::new(
                        CloudflareConfig {
                            endpoint: url,
                            connect_timeout: Duration::from_millis(60),
                            ..Default::default()
                        },
                        f.store.clone(),
                    )
                    .unwrap(),
                )
            };
            assert_eq!(
                tauri::async_runtime::block_on(provider.execute(
                    &req,
                    &AtomicBool::new(false),
                    &mut |_| Ok(())
                ))
                .unwrap_err(),
                ProviderError::Timeout
            );
            server.join().unwrap();
            let diagnostics = TIMEOUT_DIAGNOSTICS.lock().unwrap();
            let diag = diagnostics
                .iter()
                .rev()
                .find(|d| d.provider == provider_id && d.attempt == req.attempt)
                .unwrap();
            assert_eq!(diag.phase, expected);
            assert_eq!(
                diag.configured_ms,
                match expected {
                    TimeoutPhase::Connect => 60,
                    TimeoutPhase::StreamIdle => 45,
                    _ => 100,
                }
            );
            let line = diag.safe_line();
            for secret in [
                "synthetic-private-prompt-marker",
                "synthetic-private-output-marker",
                "synthetic-credential-marker",
                "authorization",
                "Bearer",
            ] {
                assert!(!line.contains(secret));
            }
        }
    }
    // Structured responses have an overall deadline, even before headers or
    // while waiting for the body. They must not inherit the SSE idle deadline.
    for (index, exchange) in [Exchange::HeadersDelay, Exchange::Idle]
        .into_iter()
        .enumerate()
    {
        let (url, server, _) = server(vec![exchange]);
        let provider = GroqProvider::new(
            GroqConfig {
                endpoint: url,
                ..Default::default()
            },
            f.store.clone(),
        )
        .unwrap();
        let mut req = request(
            "groq",
            mode(MAX_PLAN_BYTES),
            ProviderTimeouts {
                request_timeout_ms: 100,
                stream_idle_timeout_ms: 45,
            },
        );
        req.attempt = 90 + index as u32;
        assert_eq!(
            tauri::async_runtime::block_on(provider.execute(
                &req,
                &AtomicBool::new(false),
                &mut |_| Ok(())
            ))
            .unwrap_err(),
            ProviderError::Timeout
        );
        server.join().unwrap();
        let diagnostics = TIMEOUT_DIAGNOSTICS.lock().unwrap();
        let diag = diagnostics
            .iter()
            .rev()
            .find(|d| d.provider == "groq" && d.attempt == req.attempt)
            .unwrap();
        assert_eq!(diag.phase, TimeoutPhase::Overall);
        assert_eq!(diag.configured_ms, 100);
    }
}

struct Pieces {
    calls: Arc<AtomicUsize>,
    pieces: usize,
}
impl Provider for Pieces {
    fn supports_invocation(
        &self,
        invocation: &ProviderInvocationConfig,
        mode: &InvocationMode,
    ) -> bool {
        invocation.valid() && mode.valid()
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        sink: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            for _ in 0..self.pieces {
                sink(ProviderChunk { text: "x".into() })?;
            }
            Ok(ProviderResponse {
                text: String::new(),
                usage: ProviderUsage::default(),
            })
        })
    }
}
#[test]
fn fix5_event_coalescing_hundreds_of_chunks_and_scheduler_byte_guard() {
    for (pieces, limit, succeeds) in [(500, 500, true), (501, 500, false)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "synthetic".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::with_structured_output(),
                },
                Arc::new(Pieces {
                    calls: calls.clone(),
                    pieces,
                }),
            )
            .unwrap();
        let req = ProviderTaskRequest {
            traffic_class: crate::cognition::admission::TrafficClass::ForegroundInteractive,
            mode: mode(limit),
            input: "data".into(),
            internal_system_instruction: None,
            history: vec![],
            context: Arc::new(orchestrator::technical_context()),
            max_output_tokens: Some(100),
            selection: ProviderSelection::Fixed("synthetic".into()),
            targets: vec![ProviderTarget {
                provider_id: "synthetic".into(),
                invocation: target("groq", timeouts()).invocation,
            }],
            affinity_key: None,
            estimated_context_bytes: 0,
            required_capabilities: ProviderCapabilities::structured(),
        };
        let mut events = vec![];
        let result = tauri::async_runtime::block_on(
            Scheduler::new(registry).run_with_retry_conservative_output(
                req,
                TaskBudget {
                    max_provider_calls: 2,
                    max_output_tokens: Some(100),
                },
                RetryPolicy {
                    enabled: true,
                    max_retries: 1,
                    initial_backoff_ms: 0,
                },
                &AtomicBool::new(false),
                &mut |e| {
                    events.push(e);
                    Ok(())
                },
            ),
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(result.is_ok(), succeeds);
        if !succeeds {
            assert_eq!(
                result.unwrap_err(),
                SchedulerError::Provider(ProviderError::OutputLimitExceeded)
            );
        }
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, SchedulerEvent::OutputObserved { .. }))
                .count(),
            1
        );
        assert!(!events
            .iter()
            .any(|e| matches!(e, SchedulerEvent::Chunk { .. })));
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], SchedulerEvent::Selected { .. }));
        assert!(matches!(events[1], SchedulerEvent::Admitted { .. }));
        assert!(matches!(events[2], SchedulerEvent::OutputObserved { .. }));
    }
}

#[test]
fn fix5_scheduler_rejects_fixed_incompatible_mode_without_selection_or_remote_call() {
    let f = Fixture::new();
    for provider_id in ["groq", "cloudflare"] {
        let mut registry = ProviderRegistry::default();
        let provider: Arc<dyn Provider> = if provider_id == "groq" {
            Arc::new(
                GroqProvider::new(
                    GroqConfig {
                        endpoint: "http://127.0.0.1:1".into(),
                        ..Default::default()
                    },
                    f.store.clone(),
                )
                .unwrap(),
            )
        } else {
            Arc::new(
                CloudflareProvider::new(
                    CloudflareConfig {
                        endpoint: "http://127.0.0.1:1".into(),
                        ..Default::default()
                    },
                    f.store.clone(),
                )
                .unwrap(),
            )
        };
        registry
            .register(
                ProviderConfig {
                    id: provider_id.into(),
                    enabled: true,
                    priority: 1,
                    capabilities: if provider_id == "groq" {
                        ProviderCapabilities::with_structured_output()
                    } else {
                        ProviderCapabilities::text_stream()
                    },
                },
                provider,
            )
            .unwrap();
        let mut target = target(provider_id, timeouts());
        if provider_id == "groq" {
            target.invocation.model = "unproven-model".into();
        }
        let req = ProviderTaskRequest {
            traffic_class: crate::cognition::admission::TrafficClass::ForegroundInteractive,
            mode: mode(MAX_PLAN_BYTES),
            input: "data".into(),
            internal_system_instruction: None,
            history: vec![],
            context: Arc::new(orchestrator::technical_context()),
            max_output_tokens: Some(100),
            selection: ProviderSelection::Fixed(provider_id.into()),
            targets: vec![target],
            affinity_key: None,
            estimated_context_bytes: 0,
            required_capabilities: ProviderCapabilities::structured(),
        };
        let mut events = vec![];
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            req,
            TaskBudget {
                max_provider_calls: 2,
                max_output_tokens: Some(100),
            },
            &AtomicBool::new(false),
            &mut |e| {
                events.push(e);
                Ok(())
            },
        ));
        assert_eq!(
            result.unwrap_err(),
            SchedulerError::Provider(ProviderError::UnsupportedMode)
        );
        assert!(events.is_empty());
    }
}

#[test]
fn fix5_structured_planner_accepts_exact_max_plan_bytes_and_standalone_mode() {
    let f = Fixture::new();
    for size in [MAX_PLAN_BYTES - 1, MAX_PLAN_BYTES] {
        let mut raw = plan();
        raw.push_str(&" ".repeat(size - raw.len()));
        assert!(PlanV1::parse(&raw).is_ok());
        let (url, server, _) = server(vec![Exchange::Complete(200, completion(&raw, true))]);
        let result = tauri::async_runtime::block_on(orchestrator::plan(
            scheduler(&f, url),
            policy(1, 100),
            "synthetic-private-prompt-marker".into(),
            std::collections::HashMap::from([("groq".into(), timeouts())]),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        ))
        .unwrap();
        assert_eq!(result.plan.steps.len(), 2);
        assert_eq!(server.join().unwrap()[0]["stream"], false);
    }
}
