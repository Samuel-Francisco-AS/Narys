use super::{
    provider::{Provider, ProviderFuture},
    registry::ProviderRegistry,
    task_graph_runtime::start_task,
    types::{
        ProviderCapabilities, ProviderChunk, ProviderConfig, ProviderError, ProviderRequest,
        ProviderResponse, ProviderUsage,
    },
    ProviderRuntime,
};
use crate::{
    cognition::policy::{self, CognitiveRole},
    luna::{runtime::TaskRegistry, task::TaskEvent},
    persistence::{
        conversation,
        database::Database,
        identity::{self, IdentityInput},
    },
    security::secrets::{SecretError, SecretKey, SecretStore, UnlockKeyStore},
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::ipc::Channel;

#[derive(Default)]
pub(super) struct TestKeys(Mutex<Option<Vec<u8>>>);
impl UnlockKeyStore for TestKeys {
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

type CompletionAcks = std::collections::HashMap<usize, mpsc::Sender<String>>;
static COMPLETION_ACKS: std::sync::OnceLock<Mutex<CompletionAcks>> = std::sync::OnceLock::new();
struct WorkerAcknowledgement {
    key: usize,
    receiver: mpsc::Receiver<String>,
}
impl WorkerAcknowledgement {
    fn new(key: usize) -> Self {
        let (sender, receiver) = mpsc::channel();
        assert!(COMPLETION_ACKS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(key, sender)
            .is_none());
        Self { key, receiver }
    }
    fn wait(self) -> String {
        self.receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("validated worker completion")
    }
}
impl Drop for WorkerAcknowledgement {
    fn drop(&mut self) {
        COMPLETION_ACKS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .remove(&self.key);
    }
}
// Keyed by the task's shared cancellation allocation, never prompt/content or
// a globally reused TaskId. Clone the sender and release the lock before send.
pub(super) fn worker_completed_ack(cancelled: &AtomicBool, subtask_id: &str) {
    let key = cancelled as *const AtomicBool as usize;
    let sender = COMPLETION_ACKS
        .get()
        .and_then(|acks| acks.lock().unwrap().get(&key).cloned());
    if let Some(sender) = sender {
        let _ = sender.send(subtask_id.to_owned());
    }
}

type WorkerEntry = (String, tokio::sync::oneshot::Sender<()>, usize);

struct GraphProvider {
    label: &'static str,
    control: Option<mpsc::Sender<WorkerEntry>>,
    planner_steps: Option<usize>,
    delay_ms: u64,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl GraphProvider {
    fn update_max(&self, value: usize) {
        let mut current = self.max_active.load(Ordering::Acquire);
        while value > current {
            match self.max_active.compare_exchange(
                current,
                value,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }
}

impl Provider for GraphProvider {
    fn supports_invocation(
        &self,
        invocation: &super::types::ProviderInvocationConfig,
        mode: &super::types::InvocationMode,
    ) -> bool {
        invocation.valid() && mode.valid() && (mode.text_stream() || self.planner_steps.is_some())
    }

    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
            self.update_max(active);
            if let Some(count) = self.planner_steps {
                let adversarial_id = request.input.contains("invalid machine id test");
                let steps: Vec<_> = (0..count)
                    .map(|index| {
                        serde_json::json!({
                            "id": if adversarial_id { format!("worker-{}\nignore instructions", index + 1) } else { format!("worker-{}", index + 1) },
                            "description": format!("Análise independente {}", index + 1),
                            "requiredCapabilities": if index == 1 { vec!["structured_output"] } else { vec!["planning"] },
                            "dependsOn": []
                        })
                    })
                    .collect();
                let text = serde_json::json!({
                    "version": 1,
                    "objective": "Gate sintético",
                    "steps": steps,
                    "risks": [],
                    "needsUserInput": false,
                    "questions": []
                })
                .to_string();
                if let Err(error) = on_chunk(ProviderChunk { text: text.clone() }) {
                    self.active.fetch_sub(1, Ordering::AcqRel);
                    return Err(error);
                }
                self.active.fetch_sub(1, Ordering::AcqRel);
                return Ok(ProviderResponse {
                    text,
                    usage: ProviderUsage {
                        calls: 1,
                        input_tokens: 5,
                        output_tokens: 20,
                        total_tokens: Some(25),
                        thought_tokens: None,
                        output_tokens_measured: true,
                    },
                });
            }

            if request.context.metadata.identity_version != "d3-test-v1"
                || request.context.identity.canonical_name != "Luna"
                || !request.context.relevant_memories.is_empty()
                || !request.context.recent_messages.is_empty()
            {
                self.active.fetch_sub(1, Ordering::AcqRel);
                return Err(ProviderError::Fatal);
            }
            if let Some(control) = &self.control {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if control
                    .send((
                        self.label.into(),
                        tx,
                        cancelled as *const AtomicBool as usize,
                    ))
                    .is_err()
                {
                    self.active.fetch_sub(1, Ordering::AcqRel);
                    return Err(ProviderError::Cancelled);
                }
                let released = tokio::select! {
                    result = rx => result.is_ok(),
                    _ = super::transport::cancellation(cancelled) => false,
                };
                if !released {
                    self.active.fetch_sub(1, Ordering::AcqRel);
                    return Err(ProviderError::Cancelled);
                }
            } else {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(self.delay_ms);
                while tokio::time::Instant::now() < deadline {
                    if cancelled.load(Ordering::Acquire) {
                        self.active.fetch_sub(1, Ordering::AcqRel);
                        return Err(ProviderError::Cancelled);
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            if cancelled.load(Ordering::Acquire) {
                self.active.fetch_sub(1, Ordering::AcqRel);
                return Err(ProviderError::Cancelled);
            }
            let text = if request.internal_system_instruction.is_some() {
                let id = request
                    .input
                    .split("SUBTAREFA ")
                    .nth(1)
                    .and_then(|rest| rest.split(':').next())
                    .unwrap_or("invalid");
                serde_json::json!({"subtaskId": id, "text": format!("resultado-{}", self.label)})
                    .to_string()
            } else {
                format!("resultado-{}", self.label)
            };
            let emitted = on_chunk(ProviderChunk { text: text.clone() });
            self.active.fetch_sub(1, Ordering::AcqRel);
            emitted?;
            Ok(ProviderResponse {
                text,
                usage: ProviderUsage {
                    calls: 1,
                    input_tokens: 7,
                    output_tokens: 3,
                    total_tokens: Some(10),
                    thought_tokens: None,
                    output_tokens_measured: true,
                },
            })
        })
    }
}

fn dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("d3-runtime-{label}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub(super) fn seed_identity(db: &Database) {
    let identity: IdentityInput = serde_json::from_value(serde_json::json!({
        "version": "d3-test-v1",
        "canonicalName": "Luna",
        "presentation": "feminina",
        "primaryLanguage": "pt-BR",
        "concept": "assistente de teste",
        "traits": {"test": "true"},
        "behavioralInvariants": ["preservar identidade"],
        "modes": {"default": {"priority": "normal", "tone": "neutral"}},
        "relationship": {
            "primaryPersonName": "Sam",
            "relationModes": ["test"],
            "affectionStyle": {
                "warm": true,
                "provocative": false,
                "playfulJealousy": false,
                "playfulTerritoriality": false,
                "coercion": false,
                "isolation": false,
                "emotionalBlackmail": false
            },
            "interactionPreferences": {
                "wantsRealDisagreement": true,
                "wantsLunaToProposeDirectionsDuringStructuring": true,
                "prefersLinearFlowDuringImplementation": true
            }
        },
        "memoryPolicy": {
            "retrieval": "selective",
            "history": "versioned",
            "continuity": "revisable",
            "storePrivateChainOfThought": false
        },
        "provenance": "lr-7d3-test",
        "effectiveFrom": "2026-10-01"
    }))
    .unwrap();
    let mut conn = db.open().unwrap();
    let tx = conn.transaction().unwrap();
    identity::insert_version(&tx, &identity).unwrap();
    tx.commit().unwrap();
}

fn fixture(
    label: &str,
    planner_steps: usize,
    worker_delay_ms: u64,
) -> (
    Database,
    Arc<ProviderRuntime>,
    Arc<SecretStore>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    PathBuf,
) {
    fixture_with_worker_control(label, planner_steps, worker_delay_ms, None)
}

fn fixture_with_worker_control(
    label: &str,
    planner_steps: usize,
    worker_delay_ms: u64,
    control: Option<mpsc::Sender<WorkerEntry>>,
) -> (
    Database,
    Arc<ProviderRuntime>,
    Arc<SecretStore>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    PathBuf,
) {
    let dir = dir(label);
    let db = Database::for_test(dir.join("luna.sqlite3"));
    seed_identity(&db);
    let store = Arc::new(SecretStore::with_key_store(
        dir.clone(),
        Arc::new(TestKeys::default()),
    ));
    // One durable synthetic credential batch avoids three unnecessary scrypt
    // unlocks per fixture competing with TaskGraph preflight in the global suite.
    store
        .set_secrets(&[
            (SecretKey::GeminiApiKey, b"gemini".to_vec()),
            (SecretKey::GroqApiKey, b"groq".to_vec()),
            (SecretKey::CloudflareApiToken, b"cloudflare-token".to_vec()),
            (
                SecretKey::CloudflareAccountId,
                b"cloudflare-account".to_vec(),
            ),
        ])
        .unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let mut registry = ProviderRegistry::default();
    for (id, priority, planner) in [
        ("gemini", 1, Some(planner_steps)),
        ("groq", 2, None),
        ("cloudflare", 4, None),
    ] {
        registry
            .register(
                ProviderConfig {
                    id: id.into(),
                    enabled: true,
                    priority,
                    capabilities: if planner.is_some() {
                        ProviderCapabilities::with_structured_output()
                    } else {
                        ProviderCapabilities::text_stream()
                    },
                },
                Arc::new(GraphProvider {
                    label: id,
                    planner_steps: planner,
                    delay_ms: if planner.is_some() {
                        0
                    } else {
                        worker_delay_ms
                    },
                    control: if planner.is_some() {
                        None
                    } else {
                        control.clone()
                    },
                    active: active.clone(),
                    max_active: max_active.clone(),
                }),
            )
            .unwrap();
    }
    (
        db,
        Arc::new(ProviderRuntime::new(registry)),
        store,
        active,
        max_active,
        dir,
    )
}

pub(super) fn channel() -> (Channel<TaskEvent>, mpsc::Receiver<String>) {
    let (sender, receiver) = mpsc::channel();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body {
            sender
                .send(json)
                .map_err(|_| std::io::Error::other("test channel closed"))?;
        }
        Ok(())
    });
    (channel, receiver)
}

fn channel_fail_on(needle: &'static str) -> (Channel<TaskEvent>, mpsc::Receiver<String>) {
    let (sender, receiver) = mpsc::channel();
    let failed = Arc::new(AtomicBool::new(false));
    let failed_for_channel = failed.clone();
    let channel = Channel::new(move |body| {
        if failed_for_channel.load(Ordering::Acquire) {
            return Err(std::io::Error::other("synthetic task graph channel failure").into());
        }
        if let tauri::ipc::InvokeResponseBody::Json(json) = body {
            let must_fail = json.contains(needle);
            sender
                .send(json)
                .map_err(|_| std::io::Error::other("test channel closed"))?;
            if must_fail {
                failed_for_channel.store(true, Ordering::Release);
                return Err(std::io::Error::other("synthetic task graph channel failure").into());
            }
        }
        Ok(())
    });
    (channel, receiver)
}

fn cancel_on_task_started(
    registry: Arc<TaskRegistry>,
) -> (Channel<TaskEvent>, mpsc::Receiver<String>) {
    let (sender, receiver) = mpsc::channel();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&json) {
                if event["type"] == "task_started" {
                    let id = event["taskId"].as_u64().expect("root TaskId");
                    assert!(registry.cancel(crate::luna::task::TaskId(id)));
                }
            }
            sender
                .send(json)
                .map_err(|_| std::io::Error::other("test channel closed"))?;
        }
        Ok(())
    });
    (channel, receiver)
}

pub(super) fn collect(receiver: &mpsc::Receiver<String>) -> Vec<String> {
    let mut events = Vec::new();
    loop {
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(event) => events.push(event),
            Err(mpsc::RecvTimeoutError::Disconnected) => return events,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("task graph test timed out"),
        }
    }
}

fn wait_for(receiver: &mpsc::Receiver<String>, needle: &str) -> Vec<String> {
    let mut events = Vec::new();
    while !events.iter().any(|item: &String| item.contains(needle)) {
        events.push(
            receiver
                .recv_timeout(Duration::from_secs(10))
                .expect("expected task graph event"),
        );
    }
    events
}

fn local_sse_server(
    bodies: Vec<String>,
) -> (String, std::thread::JoinHandle<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut payloads = Vec::new();
        for body in bodies {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let count = conn.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if let Some(end) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            payloads.push(serde_json::from_slice(&request[end..]).unwrap());
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                if body.starts_with("data:") { "text/event-stream" } else { "application/json" }, body.len()
            );
            conn.write_all(head.as_bytes()).unwrap();
            // Deliberately split every byte, including bytes inside UTF-8 code points.
            for byte in body.as_bytes() {
                if conn.write_all(std::slice::from_ref(byte)).is_err() {
                    break;
                }
                let _ = conn.flush();
                std::thread::sleep(Duration::from_micros(100));
            }
        }
        payloads
    });
    (format!("http://{addr}"), handle)
}

fn groq_completion(text: &str, finish: &str, usage: bool) -> String {
    let mut value = serde_json::json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":finish}]});
    if usage {
        value["usage"] =
            serde_json::json!({"prompt_tokens":10,"completion_tokens":20,"total_tokens":30});
    }
    value.to_string()
}

fn groq_sse(text: &str, usage: bool) -> String {
    let mut body = format!(
        "data: {}\n\n",
        serde_json::json!({
            "choices": [{"delta": {"content": text}, "finish_reason": "stop"}]
        })
    );
    if usage {
        body.push_str("data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":20,\"total_tokens\":30}}\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

fn cloudflare_sse(text: &str) -> String {
    format!(
        "data: {}\n\ndata: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":10,\"completion_tokens\":20,\"total_tokens\":30}}}}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":{"content":text},"finish_reason":"stop"}]})
    )
}

fn cloudflare_sse_without_usage(text: &str) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":{"content":text},"finish_reason":"stop"}]})
    )
}

fn terminal_count(events: &[String]) -> usize {
    events
        .iter()
        .filter(|event| {
            event.contains("\"task_completed\"")
                || event.contains("\"task_cancelled\"")
                || event.contains("\"task_failed\"")
        })
        .count()
}

#[test]
fn invalid_planner_id_fails_before_any_worker_subtask_starts() {
    let (db, runtime, store, active, _, dir) = fixture("invalid-id", 2, 1);
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry,
        db.clone(),
        runtime,
        store,
        "invalid machine id test".into(),
        channel,
    )
    .unwrap();
    let events = collect(&receiver);
    assert_eq!(terminal_count(&events), 1);
    assert!(
        events
            .iter()
            .any(|event| event.contains("task_graph_id_invalid")),
        "{events:?}"
    );
    assert!(!events
        .iter()
        .any(|event| event.contains("\"subtask_started\"")));
    assert_eq!(active.load(Ordering::Acquire), 0);
    let conn = db.open().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn independent_workers_overlap_use_distinct_providers_and_persist_provenance() {
    let (entered, workers) = mpsc::channel();
    let (db, runtime, store, active, max_active, dir) =
        fixture_with_worker_control("parallel", 2, 0, Some(entered));
    let session_id = {
        let conn = db.open().unwrap();
        conversation::create_session(&conn).unwrap()
    };
    let session_before: (String, String, i64) = {
        let conn = db.open().unwrap();
        conn.query_row(
            "SELECT status,updated_at,(SELECT COUNT(*) FROM conversation_messages WHERE session_id=?1) FROM conversation_sessions WHERE id=?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    };
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "Gate sintético".into(),
        channel,
    )
    .unwrap();
    let first = workers
        .recv_timeout(Duration::from_secs(10))
        .expect("first worker entered");
    let second = workers
        .recv_timeout(Duration::from_secs(5))
        .expect("second worker entered");
    assert_ne!(first.0, second.0);
    assert_eq!(active.load(Ordering::Acquire), 2);
    first.1.send(()).unwrap();
    second.1.send(()).unwrap();
    let events = collect(&receiver);
    assert_eq!(terminal_count(&events), 1);
    assert!(events
        .iter()
        .any(|event| event.contains("\"task_completed\"")));
    assert!(events.iter().any(|event| event.contains("\"groq\"")));
    assert!(events.iter().any(|event| event.contains("\"cloudflare\"")));
    assert!(max_active.load(Ordering::Acquire) >= 2);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert!(!registry.contains_for_test(id));

    let conn = db.open().unwrap();
    let providers: Vec<String> = conn
        .prepare(
            "SELECT provider_id FROM task_subtask_records WHERE root_task_id=?1 ORDER BY subtask_id",
        )
        .unwrap()
        .query_map([id.0], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(providers, vec!["groq", "cloudflare"]);
    let root: String = conn
        .query_row(
            "SELECT state FROM task_records WHERE task_id=?1",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(root, "completed");
    let session_after: (String, String, i64) = conn
        .query_row(
            "SELECT status,updated_at,(SELECT COUNT(*) FROM conversation_messages WHERE session_id=?1) FROM conversation_sessions WHERE id=?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(session_after, session_before);
    assert_eq!(
        identity::current_identity(&conn)
            .unwrap()
            .unwrap()
            .input
            .version,
        "d3-test-v1"
    );
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn production_structured_groq_plan_through_scheduler_and_dispatch_two_workers() {
    use super::{
        cloudflare::{CloudflareConfig, CloudflareProvider},
        groq::{GroqConfig, GroqProvider},
        policy::{CognitiveTargetPolicy, RoutingMode},
    };

    let (db, _synthetic_runtime, store, _active, _max_active, dir) = fixture("http-boundary", 2, 1);
    let planner_json = serde_json::json!({
        "version": 1,
        "objective": "Analisar cache local desktop",
        "steps": [
            {"id":"vantagens","description":"Avaliar benefícios — análise UTF-8 de cache local","requiredCapabilities":["planning"],"dependsOn":[]},
            {"id":"riscos","description":"Avaliar riscos operacionais e privacidade","requiredCapabilities":["planning"],"dependsOn":[]}
        ],
        "risks": [], "needsUserInput": false, "questions": []
    }).to_string();
    let (groq_endpoint, groq_server) = local_sse_server(vec![
        groq_completion(&planner_json, "stop", true),
        groq_sse("Análise concluída pelo Worker Groq.", true),
    ]);
    let (cloudflare_endpoint, cloudflare_server) =
        local_sse_server(vec![cloudflare_sse_without_usage(
            "Análise concluída pelo Worker Cloudflare.",
        )]);
    let groq = Arc::new(
        GroqProvider::new(
            GroqConfig {
                endpoint: format!("{groq_endpoint}/openai/v1/chat/completions"),
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap(),
    );
    let cloudflare = Arc::new(
        CloudflareProvider::new(
            CloudflareConfig {
                endpoint: cloudflare_endpoint,
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap(),
    );
    let mut provider_registry = ProviderRegistry::default();
    provider_registry
        .register(
            ProviderConfig {
                id: "groq".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::with_structured_output(),
            },
            groq,
        )
        .unwrap();
    provider_registry
        .register(
            ProviderConfig {
                id: "cloudflare".into(),
                enabled: true,
                priority: 2,
                capabilities: ProviderCapabilities::text_stream(),
            },
            cloudflare,
        )
        .unwrap();
    let runtime = Arc::new(ProviderRuntime::new(provider_registry));
    {
        let mut conn = db.open().unwrap();
        let mut planner = policy::load(&conn, CognitiveRole::Orchestrator).unwrap();
        planner.routing_mode = RoutingMode::Fixed;
        planner.targets = vec![CognitiveTargetPolicy {
            provider_id: "groq".into(),
            model: "openai/gpt-oss-20b".into(),
            thinking_level: None,
        }];
        planner.max_provider_calls = 1;
        planner.retry_enabled = false;
        planner.max_retries = 0;
        policy::save(&mut conn, &planner).unwrap();
        let mut workers = policy::load(&conn, CognitiveRole::Worker).unwrap();
        workers.routing_mode = RoutingMode::Preferred;
        workers.targets = vec![
            CognitiveTargetPolicy {
                provider_id: "groq".into(),
                model: "openai/gpt-oss-20b".into(),
                thinking_level: None,
            },
            CognitiveTargetPolicy {
                provider_id: "cloudflare".into(),
                model: "@cf/zai-org/glm-4.7-flash".into(),
                thinking_level: None,
            },
        ];
        workers.retry_enabled = false;
        workers.max_retries = 0;
        policy::save(&mut conn, &workers).unwrap();
    }
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "analisar vantagens e riscos de cache local em desktop".into(),
        channel,
    )
    .unwrap();
    let events = collect(&receiver);
    let payloads = groq_server.join().unwrap();
    assert_eq!(payloads[0]["stream"], false);
    assert!(payloads[0].get("stream_options").is_none());
    assert_eq!(payloads[0]["response_format"]["type"], "json_schema");
    assert_eq!(
        payloads[0]["response_format"]["json_schema"]["strict"],
        true
    );
    assert_eq!(
        payloads[0]["response_format"]["json_schema"]["schema"],
        crate::agents::planner::output_schema()
    );
    assert_eq!(payloads[1]["stream"], true);
    assert!(payloads[1].get("response_format").is_none());
    assert_eq!(
        events
            .iter()
            .filter(|e| e.contains("\"provider_output_observed\""))
            .count(),
        1
    );
    cloudflare_server.join().unwrap();
    assert_eq!(terminal_count(&events), 1);
    assert!(
        events
            .iter()
            .any(|event| event.contains("\"task_completed\"")),
        "{events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.contains("\"subtask_started\""))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.contains("\"subtask_completed\""))
            .count(),
        2
    );
    assert!(events
        .iter()
        .any(|event| event.contains("Análise concluída pelo Worker Groq")));
    assert!(events
        .iter()
        .any(|event| event.contains("Análise concluída pelo Worker Cloudflare")));
    let result_event = events
        .iter()
        .find(|event| event.contains("\"task_graph_result_ready\""))
        .unwrap();
    let result_json: serde_json::Value = serde_json::from_str(result_event).unwrap();
    assert_eq!(
        result_json["result"]["workerUsage"]["outputTokensMeasured"],
        false
    );
    assert_eq!(result_json["result"]["workerUsage"]["outputTokens"], 20);
    assert!(
        result_json["result"]["workerUsage"]["outputTokensAccounted"]
            .as_u64()
            .unwrap()
            >= 2_068
    );
    let conn = db.open().unwrap();
    let providers: Vec<String> = conn.prepare("SELECT provider_id FROM task_subtask_records WHERE root_task_id=?1 ORDER BY subtask_id").unwrap()
        .query_map([id.0], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(providers, vec!["cloudflare", "groq"]);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn production_groq_incomplete_plan_fails_before_any_worker_is_compiled_or_started() {
    use super::{
        cloudflare::{CloudflareConfig, CloudflareProvider},
        groq::{GroqConfig, GroqProvider},
        policy::{CognitiveTargetPolicy, RoutingMode},
    };

    let (db, _synthetic_runtime, store, _active, _max_active, dir) =
        fixture("http-incomplete", 2, 1);
    let partial = groq_completion("{\"version\":1,\"objective\":\"partial\"", "length", true);
    let (groq_endpoint, groq_server) = local_sse_server(vec![partial]);
    let (unused_cloudflare_endpoint, unused_cloudflare_server) = local_sse_server(vec![]);
    let groq = Arc::new(
        GroqProvider::new(
            GroqConfig {
                endpoint: format!("{groq_endpoint}/openai/v1/chat/completions"),
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap(),
    );
    let cloudflare = Arc::new(
        CloudflareProvider::new(
            CloudflareConfig {
                endpoint: unused_cloudflare_endpoint,
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap(),
    );
    let mut provider_registry = ProviderRegistry::default();
    provider_registry
        .register(
            ProviderConfig {
                id: "groq".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::with_structured_output(),
            },
            groq,
        )
        .unwrap();
    provider_registry
        .register(
            ProviderConfig {
                id: "cloudflare".into(),
                enabled: true,
                priority: 2,
                capabilities: ProviderCapabilities::text_stream(),
            },
            cloudflare,
        )
        .unwrap();
    let runtime = Arc::new(ProviderRuntime::new(provider_registry));
    {
        let mut conn = db.open().unwrap();
        let mut planner = policy::load(&conn, CognitiveRole::Orchestrator).unwrap();
        planner.routing_mode = RoutingMode::Fixed;
        planner.targets = vec![CognitiveTargetPolicy {
            provider_id: "groq".into(),
            model: "openai/gpt-oss-20b".into(),
            thinking_level: None,
        }];
        planner.max_provider_calls = 1;
        planner.retry_enabled = false;
        planner.max_retries = 0;
        policy::save(&mut conn, &planner).unwrap();
    }
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry,
        db.clone(),
        runtime,
        store,
        "objective incompleto".into(),
        channel,
    )
    .unwrap();
    let events = collect(&receiver);
    groq_server.join().unwrap();
    unused_cloudflare_server.join().unwrap();
    assert_eq!(terminal_count(&events), 1);
    assert!(events.iter().any(|event| event.contains("\"task_failed\"") && event.contains("provider_incomplete")), "{events:?}");
    assert!(!events
        .iter()
        .any(|event| event.contains("\"subtask_started\"")));
    let conn = db.open().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let root: (String, Option<String>) = conn
        .query_row(
            "SELECT state,error_code FROM task_records WHERE task_id=?1",
            [id.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(root, ("failed".into(), Some("provider_incomplete".into())));
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn production_cloudflare_is_skipped_as_structured_planner_but_remains_worker() {
    use super::{
        cloudflare::{CloudflareConfig, CloudflareProvider},
        groq::{GroqConfig, GroqProvider},
        policy::{CognitiveTargetPolicy, RoutingMode},
    };

    let (db, _synthetic_runtime, store, _active, _max_active, dir) =
        fixture("cloudflare-planner", 2, 1);
    let planner_json = serde_json::json!({
        "version":1,"objective":"cache local",
        "steps":[
            {"id":"vantagens","description":"Avaliar benefícios e custo local","requiredCapabilities":["planning"],"dependsOn":[]},
            {"id":"riscos","description":"Avaliar riscos e privacidade","requiredCapabilities":["planning"],"dependsOn":[]}
        ],"risks":[],"needsUserInput":false,"questions":[]
    }).to_string();
    let (groq_endpoint, groq_server) = local_sse_server(vec![
        groq_completion(&planner_json, "stop", true),
        groq_sse("Worker Groq real-style.", true),
    ]);
    let (cloudflare_endpoint, cloudflare_server) =
        local_sse_server(vec![cloudflare_sse("Worker Cloudflare real-style.")]);
    let groq = Arc::new(
        GroqProvider::new(
            GroqConfig {
                endpoint: format!("{groq_endpoint}/openai/v1/chat/completions"),
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap(),
    );
    let cloudflare = Arc::new(
        CloudflareProvider::new(
            CloudflareConfig {
                endpoint: cloudflare_endpoint,
                ..Default::default()
            },
            store.clone(),
        )
        .unwrap(),
    );
    let mut providers = ProviderRegistry::default();
    providers
        .register(
            ProviderConfig {
                id: "groq".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::with_structured_output(),
            },
            groq,
        )
        .unwrap();
    providers
        .register(
            ProviderConfig {
                id: "cloudflare".into(),
                enabled: true,
                priority: 2,
                capabilities: ProviderCapabilities::text_stream(),
            },
            cloudflare,
        )
        .unwrap();
    let runtime = Arc::new(ProviderRuntime::new(providers));
    {
        let mut conn = db.open().unwrap();
        let mut planner = policy::load(&conn, CognitiveRole::Orchestrator).unwrap();
        planner.routing_mode = RoutingMode::Preferred;
        planner.targets = vec![
            CognitiveTargetPolicy {
                provider_id: "cloudflare".into(),
                model: "@cf/zai-org/glm-4.7-flash".into(),
                thinking_level: None,
            },
            CognitiveTargetPolicy {
                provider_id: "groq".into(),
                model: "openai/gpt-oss-20b".into(),
                thinking_level: None,
            },
        ];
        planner.max_provider_calls = 2;
        planner.retry_enabled = false;
        planner.max_retries = 0;
        policy::save(&mut conn, &planner).unwrap();
    }
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry,
        db.clone(),
        runtime,
        store,
        "analisar vantagens e riscos de cache local".into(),
        channel,
    )
    .unwrap();
    let events = collect(&receiver);
    groq_server.join().unwrap();
    cloudflare_server.join().unwrap();
    assert_eq!(terminal_count(&events), 1);
    assert!(events
        .iter()
        .filter(|e| e.contains("\"provider_selected\""))
        .all(|e| e.contains("\"provider_id\":\"groq\"")));
    assert!(!events.iter().any(|e| e.contains("\"provider_fallback\"")));
    assert!(
        events
            .iter()
            .any(|event| event.contains("\"task_completed\"")),
        "{events:?}"
    );
    assert!(events
        .iter()
        .any(|event| event.contains("\"provider_id\":\"groq\"")));
    assert!(events
        .iter()
        .any(|event| event.contains("\"provider_id\":\"cloudflare\"")));
    let conn = db.open().unwrap();
    let providers: Vec<String> = conn.prepare("SELECT provider_id FROM task_subtask_records WHERE root_task_id=?1 ORDER BY subtask_id").unwrap()
        .query_map([id.0], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(providers, vec!["cloudflare", "groq"]);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn root_cancel_propagates_to_parallel_workers_and_never_completes() {
    let (db, runtime, store, active, _max_active, dir) = fixture("cancel", 2, 5_000);
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "Cancelamento sintético".into(),
        channel,
    )
    .unwrap();
    let mut events = wait_for(&receiver, "\"subtask_started\"");
    assert!(registry.cancel(id));
    events.extend(collect(&receiver));
    assert_eq!(terminal_count(&events), 1);
    assert!(events
        .iter()
        .any(|event| event.contains("\"task_cancelled\"")));
    assert!(!events
        .iter()
        .any(|event| event.contains("\"task_completed\"")));
    assert_eq!(active.load(Ordering::Acquire), 0);

    let conn = db.open().unwrap();
    let root: String = conn
        .query_row(
            "SELECT state FROM task_records WHERE task_id=?1",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(root, "cancelled");
    let bad: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1 AND state='completed'",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bad, 0);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn completed_worker_is_preserved_when_sibling_is_cancelled() {
    let (entered, workers) = mpsc::channel();
    let (db, runtime, store, active, max_active, dir) =
        fixture_with_worker_control("partial-cancel", 2, 0, Some(entered));
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "cancelar irmã lenta".into(),
        channel,
    )
    .unwrap();
    let first = workers
        .recv_timeout(Duration::from_secs(10))
        .expect("first worker entered");
    let second = workers
        .recv_timeout(Duration::from_secs(5))
        .expect("parallel sibling entered");
    assert_eq!(max_active.load(Ordering::Acquire), 2);
    let (fast, slow) = if first.0 == "groq" {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(slow.0, "cloudflare");
    let completed = WorkerAcknowledgement::new(fast.2);
    fast.1.send(()).unwrap();
    assert_eq!(completed.wait(), "worker-1");
    // The acknowledgement is after Scheduler/worker validation; the wave-level
    // SubtaskCompleted event is deliberately emitted only after both futures.
    let mut events = Vec::new();
    assert_eq!(active.load(Ordering::Acquire), 1);
    assert!(registry.cancel(id));
    events.extend(collect(&receiver));
    drop(slow);
    assert_eq!(terminal_count(&events), 1);
    assert!(events
        .iter()
        .any(|event| event.contains("\"task_cancelled\"")));
    assert!(events
        .iter()
        .any(|event| event.contains("\"subtask_completed\"")));
    assert_eq!(active.load(Ordering::Acquire), 0);

    let conn = db.open().unwrap();
    let states: Vec<(String, String)> = conn.prepare(
        "SELECT subtask_id,state FROM task_subtask_records WHERE root_task_id=?1 ORDER BY subtask_id"
    ).unwrap().query_map([id.0], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        states,
        vec![
            ("worker-1".into(), "completed".into()),
            ("worker-2".into(), "cancelled".into())
        ]
    );
    let (root_finished, completed_finished): (String, String) = conn.query_row(
        "SELECT r.finished_at,s.finished_at FROM task_records r JOIN task_subtask_records s ON s.root_task_id=r.task_id WHERE r.task_id=?1 AND s.subtask_id='worker-1'",
        [id.0],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert!(
        completed_finished <= root_finished,
        "completed unit must retain its own completion time"
    );
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn root_cancel_requested_by_task_started_callback_is_cancelled_without_provider_call() {
    let (db, runtime, store, active, max_active, dir) = fixture("immediate-cancel", 2, 10);
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = cancel_on_task_started(registry.clone());
    let id = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "cancelar imediatamente".into(),
        channel,
    )
    .unwrap();
    let events = collect(&receiver);
    assert_eq!(terminal_count(&events), 1);
    assert!(events
        .iter()
        .any(|event| event.contains("\"task_cancelled\"")));
    assert!(!events
        .iter()
        .any(|event| event.contains("\"subtask_started\"")));
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert_eq!(
        max_active.load(Ordering::Acquire),
        0,
        "a provider was called before immediate cancellation took effect"
    );
    assert!(!registry.contains_for_test(id));

    let conn = db.open().unwrap();
    let root: (String, Option<String>) = conn
        .query_row(
            "SELECT state,error_code FROM task_records WHERE task_id=?1",
            [id.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(root, ("cancelled".into(), None));
    let subtasks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(subtasks, 0);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn root_worker_budget_blocks_new_wave_before_exceeding_call_limit() {
    let (db, runtime, store, active, _max_active, dir) = fixture("budget", 3, 40);
    {
        let mut conn = db.open().unwrap();
        let mut worker = policy::load(&conn, CognitiveRole::Worker).unwrap();
        worker.max_provider_calls = 2;
        worker.retry_enabled = false;
        worker.max_retries = 0;
        policy::save(&mut conn, &worker).unwrap();
    }
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let id = start_task(
        registry,
        db.clone(),
        runtime,
        store,
        "Budget sintético".into(),
        channel,
    )
    .unwrap();
    let events = collect(&receiver);
    assert_eq!(terminal_count(&events), 1);
    assert!(events.iter().any(|event| event.contains("\"task_failed\"")));
    assert!(events
        .iter()
        .any(|event| event.contains("task_graph_budget_exceeded")));
    assert_eq!(active.load(Ordering::Acquire), 0);

    let conn = db.open().unwrap();
    let root: (String, Option<String>) = conn
        .query_row(
            "SELECT state,error_code FROM task_records WHERE task_id=?1",
            [id.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(root.0, "failed");
    assert_eq!(root.1.as_deref(), Some("task_graph_budget_exceeded"));
    let completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1 AND state='completed'",
            [id.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(completed, 2);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn worker_channel_failure_is_failed_and_stops_parallel_sibling() {
    let (db, runtime, store, active, _max_active, dir) = fixture("channel", 2, 80);
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel_fail_on("\"subtask_output_observed\"");
    let id = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "Channel sintético".into(),
        channel,
    )
    .unwrap();
    let _events = collect(&receiver);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert!(!registry.contains_for_test(id));

    let conn = db.open().unwrap();
    let root: (String, Option<String>) = conn
        .query_row(
            "SELECT state,error_code FROM task_records WHERE task_id=?1",
            [id.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(root.0, "failed");
    assert_eq!(root.1.as_deref(), Some("channel_closed"));
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fix5_production_structured_invalid_planner_never_starts_workers_and_terminal_is_unique() {
    use super::{
        groq::{GroqConfig, GroqProvider},
        policy::{CognitiveTargetPolicy, RoutingMode},
    };
    for (output, code) in [
        ("{".to_owned(), "orchestrator_json_syntax_invalid"),
        (
            serde_json::json!({"version":1,"objective":"cache","steps":[
            {"id":"a","description":"A","requiredCapabilities":["planning"],"dependsOn":["a"]}],
            "risks":[],"needsUserInput":false,"questions":[]})
            .to_string(),
            "orchestrator_plan_semantic_invalid",
        ),
    ] {
        let (db, _synthetic, store, _, _, dir) = fixture("fix5-invalid", 2, 1);
        let (endpoint, server) = local_sse_server(vec![groq_completion(&output, "stop", true)]);
        let mut providers = ProviderRegistry::default();
        providers
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
                        store.clone(),
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        // Workers use the production text streaming Cloudflare adapter. Neither can be called on planner failure.
        providers
            .register(
                ProviderConfig {
                    id: "cloudflare".into(),
                    enabled: true,
                    priority: 2,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(
                    super::cloudflare::CloudflareProvider::new(
                        super::cloudflare::CloudflareConfig {
                            endpoint: "http://127.0.0.1:1".into(),
                            ..Default::default()
                        },
                        store.clone(),
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        {
            let mut conn = db.open().unwrap();
            let mut planner = policy::load(&conn, CognitiveRole::Orchestrator).unwrap();
            planner.routing_mode = RoutingMode::Fixed;
            planner.targets = vec![CognitiveTargetPolicy {
                provider_id: "groq".into(),
                model: super::groq::MODEL.into(),
                thinking_level: None,
            }];
            planner.retry_enabled = false;
            planner.max_provider_calls = 1;
            policy::save(&mut conn, &planner).unwrap();
        }
        let (channel, receiver) = channel();
        let id = start_task(
            Arc::new(TaskRegistry::default()),
            db.clone(),
            Arc::new(ProviderRuntime::new(providers)),
            store,
            "cache desktop".into(),
            channel,
        )
        .unwrap();
        let events = collect(&receiver);
        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(terminal_count(&events), 1);
        assert!(events
            .iter()
            .any(|e| e.contains("\"task_failed\"") && e.contains(code)));
        assert!(!events
            .iter()
            .any(|e| e.contains("\"subtask_started\"") || e.contains("\"task_planned\"")));
        let count: i64 = db
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1",
                [id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        fs::remove_dir_all(dir).unwrap();
    }
}

/// Worker completion is controlled by channels; no overlap assertion depends on sleeps.
struct AdmissionGraphWorker {
    entered: mpsc::Sender<(String, tokio::sync::oneshot::Sender<()>)>,
}
impl Provider for AdmissionGraphWorker {
    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let id = request
                .input
                .split("SUBTAREFA ")
                .nth(1)
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .to_owned();
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.entered.send((id.clone(), tx)).unwrap();
            rx.await.unwrap();
            Ok(ProviderResponse {
                text: serde_json::json!({"subtaskId":id,"text":format!("result-{id}")}).to_string(),
                usage: ProviderUsage {
                    calls: 1,
                    output_tokens: 3,
                    output_tokens_measured: true,
                    ..ProviderUsage::default()
                },
            })
        })
    }
}
fn same_provider_workers_admission(cap: usize) {
    use super::{admission::AdmissionConfig, scheduler::Scheduler};
    let (db, _old_runtime, store, active, max_active, dir) = fixture("lr8b-workers", 2, 0);
    let mut conn = db.open().unwrap();
    let mut worker = policy::load(&conn, CognitiveRole::Worker).unwrap();
    worker.routing_mode = policy::RoutingMode::Fixed;
    worker.targets = vec![policy::CognitiveTargetPolicy {
        provider_id: "groq".into(),
        model: "openai/gpt-oss-20b".into(),
        thinking_level: Some(policy::ThinkingLevel::Low),
    }];
    worker.retry_enabled = false;
    policy::save(&mut conn, &worker).unwrap();
    drop(conn);
    let (tx, entered) = mpsc::channel();
    let mut providers = ProviderRegistry::default();
    providers
        .register(
            ProviderConfig {
                id: "gemini".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::with_structured_output(),
            },
            Arc::new(GraphProvider {
                label: "gemini",
                control: None,
                planner_steps: Some(2),
                delay_ms: 0,
                active,
                max_active,
            }),
        )
        .unwrap();
    providers
        .register(
            ProviderConfig {
                id: "groq".into(),
                enabled: true,
                priority: 2,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(AdmissionGraphWorker { entered: tx }),
        )
        .unwrap();
    let runtime = Arc::new(ProviderRuntime {
        scheduler: Arc::new(
            Scheduler::with_admission_config(
                providers,
                AdmissionConfig {
                    max_concurrency_per_provider: cap,
                    ..AdmissionConfig::default()
                },
            )
            .unwrap(),
        ),
    });
    let (channel, receiver) = channel();
    let task = start_task(
        Arc::new(TaskRegistry::default()),
        db.clone(),
        runtime.clone(),
        store,
        "Gate sintético".into(),
        channel,
    )
    .unwrap();
    let first = entered
        .recv_timeout(Duration::from_secs(10))
        .expect("first worker admitted");
    // Independent units may complete durable preparation in either order.
    // Admission capacity/overlap and final plan ordering remain the contracts.
    assert!(matches!(first.0.as_str(), "worker-1" | "worker-2"));
    let mut events = vec![];
    let second = if cap == 1 {
        events.extend(wait_for(&receiver, "\"provider_queued\""));
        assert!(entered.try_recv().is_err());
        let s = runtime
            .scheduler
            .admission_snapshot()
            .into_iter()
            .find(|s| s.provider_id == "groq")
            .unwrap();
        assert_eq!((s.active_calls, s.queue_depth), (1, 1));
        first.1.send(()).unwrap();
        entered
            .recv_timeout(Duration::from_secs(10))
            .expect("second worker admitted after release")
    } else {
        let second = entered
            .recv_timeout(Duration::from_secs(10))
            .expect("both workers admitted before release");
        let s = runtime
            .scheduler
            .admission_snapshot()
            .into_iter()
            .find(|s| s.provider_id == "groq")
            .unwrap();
        assert_eq!((s.active_calls, s.queue_depth), (2, 0));
        first.1.send(()).unwrap();
        second
    };
    assert!(matches!(second.0.as_str(), "worker-1" | "worker-2"));
    assert_ne!(first.0, second.0);
    second.1.send(()).unwrap();
    events.extend(collect(&receiver));
    assert_eq!(terminal_count(&events), 1);
    assert!(
        events.iter().any(|e| e.contains("\"task_completed\"")),
        "{events:?}"
    );
    let result: serde_json::Value = events
        .iter()
        .map(|e| serde_json::from_str::<serde_json::Value>(e).unwrap())
        .find(|e| e["type"] == "task_graph_result_ready")
        .unwrap()["result"]
        .clone();
    assert_eq!(result["subtasks"][0]["subtaskId"], "worker-1");
    assert_eq!(result["subtasks"][1]["subtaskId"], "worker-2");
    assert_eq!(result["subtasks"][0]["providerId"], "groq");
    assert_eq!(result["subtasks"][1]["providerId"], "groq");
    let consolidated = result["consolidatedText"].as_str().unwrap();
    assert!(
        consolidated.find("result-worker-1").unwrap()
            < consolidated.find("result-worker-2").unwrap()
    );
    let conn = db.open().unwrap();
    let rows:i64=conn.query_row("SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=?1 AND provider_id='groq' AND state='completed'",[task.0],|row|row.get(0)).unwrap();
    assert_eq!(rows, 2);
    drop(conn);
    for s in runtime.scheduler.admission_snapshot() {
        assert_eq!((s.active_calls, s.queue_depth), (0, 0));
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn lr8b_task_graph_cap_one_serializes_same_provider_workers_with_provenance() {
    same_provider_workers_admission(1);
}
#[test]
fn lr8b_task_graph_cap_two_admits_parallel_same_provider_workers_with_provenance() {
    same_provider_workers_admission(2);
}

#[test]
fn c4_fix1_a_b_initial_path_history_failures_roll_back_terminal_and_history() {
    for table in ["task_records", "task_subtask_records"] {
        let (db, runtime, store, active, _, dir) = fixture("terminal-history-rollback", 2, 1);
        db.open().unwrap().execute_batch(&format!(
            "CREATE TRIGGER fail_terminal_history BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'local terminal fault'); END;"
        )).unwrap();
        let registry = Arc::new(TaskRegistry::default());
        let (channel, receiver) = channel();
        let id = start_task(
            registry.clone(),
            db.clone(),
            runtime,
            store,
            "Terminal atomicity gate".into(),
            channel,
        )
        .unwrap();
        let events = collect(&receiver);
        assert!(!events.iter().any(|e| e.contains("\"task_completed\"")));
        assert!(events.iter().any(|e| e.contains("\"task_paused\"")));
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert!(!registry.contains_for_test(id));
        let conn = db.open().unwrap();
        let state: (String, Option<String>) = conn
            .query_row(
                "SELECT state,pause_reason FROM cognitive_continuations WHERE root_task_id=?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, ("paused".into(), Some("recovery_required".into())));
        for table in ["task_records", "task_subtask_records"] {
            let root_column = if table == "task_records" {
                "task_id"
            } else {
                "root_task_id"
            };
            let count: u32 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {root_column}=?1"),
                    [id.0],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        }
        let loaded =
            crate::persistence::continuations::ContinuationRepository::load(&conn, id.0).unwrap();
        assert_eq!(loaded.completed.len(), 2);
        assert!(loaded.uncertain.is_empty());
        let checkpoints: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM cognitive_checkpoints WHERE root_task_id=?1",
                [id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(checkpoints, 2);
        drop(conn);
        fs::remove_dir_all(dir).unwrap();
    }
}
