use super::*;
use crate::cognition::{
    policy::{CognitiveTargetPolicy, RoutingMode, ThinkingLevel},
    provider::{Provider, ProviderFuture},
    registry::ProviderRegistry,
    types::{
        ProviderChunk, ProviderConfig, ProviderError, ProviderRequest, ProviderResponse,
        ProviderUsage,
    },
};
use crate::persistence::{identity, provider_timeouts};
use crate::security::secrets::{SecretError, SecretKey, UnlockKeyStore};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{mpsc, Condvar, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

// Gates are keyed by the fixture's shared session registry, never message content.
// They block only inside spawn_blocking, and release on drop even after an assertion panic.
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    cv: Condvar,
}
impl Gate {
    fn block(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.cv.notify_all();
        while !state.1 {
            state = self.cv.wait(state).unwrap();
        }
    }
    fn entered(&self) {
        let state = self.state.lock().unwrap();
        let (state, timeout) = self
            .cv
            .wait_timeout_while(state, Duration::from_secs(30), |state| !state.0)
            .unwrap();
        assert!(
            state.0 && !timeout.timed_out(),
            "preflight did not reach gate"
        );
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.cv.notify_all();
    }
}
struct ReleaseOnDrop(Arc<Gate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
type Gates = HashMap<usize, (bool, Arc<Gate>)>;
static GATES: OnceLock<Mutex<Gates>> = OnceLock::new();
struct InstalledGate {
    key: usize,
    gate: Arc<Gate>,
}
impl InstalledGate {
    fn new(sessions: &CurrentRunSessions, after: bool) -> Self {
        let key = Arc::as_ptr(&sessions.0) as usize;
        let gate = Arc::new(Gate::default());
        GATES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(key, (after, gate.clone()));
        Self { key, gate }
    }
}
impl Drop for InstalledGate {
    fn drop(&mut self) {
        self.gate.release();
        GATES.get().unwrap().lock().unwrap().remove(&self.key);
    }
}
pub(super) fn wait_at_gate(sessions: &CurrentRunSessions, after: bool) {
    let key = Arc::as_ptr(&sessions.0) as usize;
    let gate = GATES
        .get()
        .and_then(|gates| gates.lock().unwrap().get(&key).cloned());
    if let Some((phase, gate)) = gate {
        if phase == after {
            gate.block();
        }
    }
}

#[derive(Default)]
struct Keys {
    key: Mutex<Option<Vec<u8>>>,
    gate: Mutex<Option<Arc<Gate>>>,
    loads: std::sync::atomic::AtomicUsize,
    unavailable: AtomicBool,
}
impl UnlockKeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(SecretError::CredentialStoreUnavailable);
        }
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.block();
        }
        Ok(self.key.lock().unwrap().clone())
    }
    fn store(&self, key: &[u8]) -> Result<(), SecretError> {
        *self.key.lock().unwrap() = Some(key.to_vec());
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretError> {
        *self.key.lock().unwrap() = None;
        Ok(())
    }
}
#[derive(Debug)]
struct Observed {
    target: ProviderTarget,
    input: String,
    history: Vec<ProviderMessage>,
}
struct RecordingProvider {
    requests: Arc<Mutex<Vec<Observed>>>,
    fail: Arc<AtomicBool>,
}
impl Provider for RecordingProvider {
    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        _cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(Observed {
                target: request.target.clone(),
                input: request.input.clone(),
                history: request.history.clone(),
            });
            if self.fail.load(Ordering::Acquire) {
                return Err(ProviderError::Timeout);
            }
            on_chunk(ProviderChunk {
                text: "answer".into(),
            })?;
            Ok(ProviderResponse {
                text: "answer".into(),
                usage: ProviderUsage {
                    calls: 1,
                    input_tokens: 2,
                    output_tokens: 3,
                    total_tokens: Some(5),
                    thought_tokens: None,
                    output_tokens_measured: true,
                },
            })
        })
    }
}
struct Fixture {
    dir: PathBuf,
    db: Database,
    registry: Arc<TaskRegistry>,
    runtime: Arc<ProviderRuntime>,
    store: Arc<SecretStore>,
    keys: Arc<Keys>,
    sessions: CurrentRunSessions,
    session: i64,
    requests: Arc<Mutex<Vec<Observed>>>,
    gemini_fail: Arc<AtomicBool>,
}
impl Fixture {
    fn new(credentials: &[SecretKey]) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "conversation-preflight-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let db = Database::for_test(dir.join("test.sqlite3"));
        let mut conn = db.open().unwrap();
        let identity = serde_json::from_value(json!({
            "version":"test","canonicalName":"Synthetic","presentation":"neutral","primaryLanguage":"pt-BR","concept":"test",
            "traits":{"curiosity":"high"},"behavioralInvariants":["be_clear"],"modes":{"test":{"priority":"test","tone":"calm"}},
            "relationship":{"primaryPersonName":"Tester","relationModes":["testing"],
              "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
              "interactionPreferences":{"wantsRealDisagreement":false,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":false}},
            "memoryPolicy":{"retrieval":"none","history":"none","continuity":"none","storePrivateChainOfThought":false},
            "provenance":"test","effectiveFrom":"2026-01-01"
        })).unwrap();
        let tx = conn.transaction().unwrap();
        identity::insert_version(&tx, &identity).unwrap();
        tx.commit().unwrap();
        let session = conversation::create_session(&conn).unwrap();
        let sessions = CurrentRunSessions::default();
        sessions.0.lock().unwrap().insert(session);
        let keys = Arc::new(Keys::default());
        let store = Arc::new(SecretStore::with_key_store(
            dir.join("secrets"),
            keys.clone(),
        ));
        for key in credentials {
            store
                .set_secret(*key, b"synthetic-test-credential")
                .unwrap();
        }
        let requests = Arc::new(Mutex::new(vec![]));
        let gemini_fail = Arc::new(AtomicBool::new(false));
        let mut providers = ProviderRegistry::default();
        for (id, priority, fail) in [
            ("gemini", 1, gemini_fail.clone()),
            ("groq", 2, Arc::new(AtomicBool::new(false))),
        ] {
            providers
                .register(
                    ProviderConfig {
                        id: id.into(),
                        enabled: true,
                        priority,
                        capabilities: ProviderCapabilities::text_stream(),
                    },
                    Arc::new(RecordingProvider {
                        requests: requests.clone(),
                        fail,
                    }),
                )
                .unwrap();
        }
        let fixture = Self {
            dir,
            db,
            registry: Arc::new(TaskRegistry::default()),
            runtime: Arc::new(ProviderRuntime::new(providers)),
            store,
            keys,
            sessions,
            session,
            requests,
            gemini_fail,
        };
        fixture.configure(RoutingMode::Fixed, &["groq"]);
        fixture
    }
    fn configure(&self, mode: RoutingMode, ids: &[&str]) {
        let mut conn = self.db.open().unwrap();
        let mut policy = policy::load(&conn, CognitiveRole::Conversation).unwrap();
        policy.routing_mode = mode;
        policy.targets = ids
            .iter()
            .map(|id| CognitiveTargetPolicy {
                provider_id: (*id).into(),
                model: format!("{id}-persisted-model"),
                thinking_level: Some(if *id == "gemini" {
                    ThinkingLevel::High
                } else {
                    ThinkingLevel::Low
                }),
            })
            .collect();
        policy.retry_enabled = false;
        policy.max_provider_calls = 2;
        policy::save(&mut conn, &policy).unwrap();
        for id in ids {
            provider_timeouts::save(
                &conn,
                id,
                crate::cognition::types::ProviderTimeouts {
                    request_timeout_ms: if *id == "gemini" { 12345 } else { 23456 },
                    stream_idle_timeout_ms: 4321,
                },
            )
            .unwrap();
        }
    }
    fn start(&self, session: i64, message: &str) -> (TaskId, mpsc::Receiver<Value>) {
        let (sender, receiver) = mpsc::channel();
        let channel = Channel::new(move |body| {
            if let crate::channel::InvokeResponseBody::Json(json) = body {
                sender
                    .send(serde_json::from_str(&json).unwrap())
                    .map_err(|_| std::io::Error::other("test channel closed"))?;
            }
            Ok(())
        });
        let id = start_conversation(
            self.registry.clone(),
            self.db.clone(),
            self.runtime.clone(),
            self.store.clone(),
            self.sessions.clone(),
            session,
            message.into(),
            channel,
        )
        .unwrap();
        (id, receiver)
    }
    fn collect(&self, id: TaskId, receiver: mpsc::Receiver<Value>, state: &str) -> Vec<Value> {
        let mut events = vec![];
        loop {
            match receiver.recv_timeout(Duration::from_secs(30)) {
                Ok(event) => events.push(event),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("task channel did not close"),
            }
        }
        assert_eq!(events.first().unwrap()["type"], "task_started");
        let terminals: Vec<_> = events
            .iter()
            .filter(|e| {
                ["task_completed", "task_cancelled", "task_failed"]
                    .contains(&e["type"].as_str().unwrap())
            })
            .collect();
        assert_eq!(terminals.len(), 1, "{events:?}");
        assert_eq!(terminals[0]["state"], state);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event["taskId"], id.0);
            assert_eq!(event["sequence"], index + 1);
        }
        assert!(!self.registry.contains_for_test(id));
        assert!(!self.registry.has_foreground_provider_work());
        let conn = self.db.open().unwrap();
        let stored: (String, Option<String>) = conn
            .query_row(
                "SELECT state,error_code FROM task_records WHERE task_id=?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored.0, state);
        if state != "failed" {
            assert_eq!(stored.1, None);
        }
        events
    }
    fn no_exchange(&self) {
        assert!(
            conversation::session(&self.db.open().unwrap(), self.session)
                .unwrap()
                .unwrap()
                .messages
                .is_empty()
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn b4_conversation_real_preflight_missing_corrupt_auto_and_explicit_independence() {
    let f = Fixture::new(&[SecretKey::GroqApiKey, SecretKey::GeminiApiKey]);
    let conn = f.db.open().unwrap();
    conn.execute(
        "DELETE FROM cognitive_role_allocation_policies WHERE role='conversation'",
        [],
    )
    .unwrap();
    f.configure(RoutingMode::Auto, &["groq", "gemini"]);
    let (id, receiver) = f.start(f.session, "input");
    let events = f.collect(id, receiver, "failed");
    assert_eq!(events.len(), 2);
    assert_eq!(events.last().unwrap()["detail"], "read_failed");
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();

    // Explicit modes execute with the missing economic row, with no fallback default.
    for mode in [RoutingMode::Fixed, RoutingMode::Preferred] {
        f.configure(
            mode,
            if mode == RoutingMode::Fixed {
                &["groq"]
            } else {
                &["groq", "gemini"]
            },
        );
        let (id, receiver) = f.start(f.session, "explicit");
        let events = f.collect(id, receiver, "completed");
        assert!(events
            .iter()
            .any(|event| event["type"] == "provider_selected" && event["provider_id"] == "groq"));
    }
    let calls = f.requests.lock().unwrap().len();
    // Simulate corruption bypassing SQLite CHECKs. Auto cannot proceed to selection.
    conn.execute_batch("PRAGMA ignore_check_constraints=ON; INSERT INTO cognitive_role_allocation_policies(role,allocation_profile,variant_selection_mode,paid_use_policy) VALUES('conversation','invalid','auto','deny'); PRAGMA ignore_check_constraints=OFF;").unwrap();
    f.configure(RoutingMode::Auto, &["groq", "gemini"]);
    let (id, receiver) = f.start(f.session, "corrupt");
    let events = f.collect(id, receiver, "failed");
    assert_eq!(events.len(), 2);
    assert_eq!(events.last().unwrap()["detail"], "read_failed");
    assert_eq!(f.requests.lock().unwrap().len(), calls);
}

#[test]
fn b4_conversation_real_preflight_freezes_routing_and_allocation_before_save() {
    use crate::cognition::allocation_policy::{self, PaidUseMode};
    use crate::cognitive_resources::{AllocationProfile, VariantSelectionMode};
    let f = Fixture::new(&[SecretKey::GroqApiKey, SecretKey::GeminiApiKey]);
    f.configure(RoutingMode::Auto, &["groq", "gemini"]);
    let gate = InstalledGate::new(&f.sessions, true);
    let (id, receiver) = f.start(f.session, "old snapshot");
    gate.gate.entered();
    let mut conn = f.db.open().unwrap();
    let mut routing = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    routing.targets.reverse();
    let mut allocation = allocation_policy::load(&conn, CognitiveRole::Conversation).unwrap();
    allocation.allocation_profile = AllocationProfile::Fast;
    allocation.variant_selection_mode = VariantSelectionMode::Explicit;
    allocation.minimum_cognitive_tier = Some(255); // Production tiers are Unknown.
    allocation.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
    allocation.max_paid_currency = Some("USD".into());
    allocation.max_paid_micros = Some(7);
    allocation.reduced_below_percent = Some(80);
    allocation.reserve_below_percent = Some(40);
    allocation_policy::save_role_settings(&mut conn, &routing, &allocation).unwrap();
    gate.gate.release();
    let events = f.collect(id, receiver, "completed");
    assert!(events
        .iter()
        .any(|event| event["type"] == "provider_selected"
            && event["provider_id"] == "groq"
            && event["routing_reason"] == "auto_allocator"));
    assert_eq!(f.requests.lock().unwrap().len(), 1);
    drop(gate);
    let (id, receiver) = f.start(f.session, "new snapshot");
    let events = f.collect(id, receiver, "failed");
    assert!(!events
        .iter()
        .any(|event| event["type"] == "provider_selected"));
    assert_eq!(f.requests.lock().unwrap().len(), 1);
}

#[test]
fn task_id_and_started_exist_before_preflight_and_policy_is_read_in_worker() {
    let f = Fixture::new(&[SecretKey::GroqApiKey]);
    let gate = InstalledGate::new(&f.sessions, false);
    let (id, receiver) = f.start(f.session, "input");
    gate.gate.entered();
    assert!(f.registry.contains_for_test(id));
    assert!(f
        .registry
        .has_foreground_provider_work_for_session(f.session));
    assert!(f.requests.lock().unwrap().is_empty());
    // A change made after registration is what the worker actually loads.
    let mut conn = f.db.open().unwrap();
    let mut policy = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    policy.targets[0].model = "updated-after-registration".into();
    policy.targets[0].thinking_level = Some(ThinkingLevel::Medium);
    policy::save(&mut conn, &policy).unwrap();
    provider_timeouts::save(
        &conn,
        "groq",
        crate::cognition::types::ProviderTimeouts {
            request_timeout_ms: 56789,
            stream_idle_timeout_ms: 3456,
        },
    )
    .unwrap();
    gate.gate.release();
    let events = f.collect(id, receiver, "completed");
    assert!(events
        .iter()
        .any(|e| e["type"] == "provider_selected" && e["routing_reason"] == "fixed"));
    assert!(events.iter().any(|e| e["type"] == "task_result_ready"));
    let requests = f.requests.lock().unwrap();
    assert_eq!(
        requests[0].target.invocation.model,
        "updated-after-registration"
    );
    assert_eq!(
        requests[0].target.invocation.thinking_level,
        Some(ThinkingLevel::Medium)
    );
    assert_eq!(
        requests[0]
            .target
            .invocation
            .timeouts
            .unwrap()
            .request_timeout_ms,
        56789
    );
    assert_eq!(
        requests[0]
            .target
            .invocation
            .timeouts
            .unwrap()
            .stream_idle_timeout_ms,
        3456
    );
    assert_eq!(
        conversation::session(&conn, f.session)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
}

#[test]
fn cancellation_before_preflight_work_has_one_terminal_no_call_or_exchange() {
    let f = Fixture::new(&[]);
    let gate = InstalledGate::new(&f.sessions, false);
    let (id, receiver) = f.start(f.session, "input");
    gate.gate.entered();
    assert!(f.registry.cancel(id));
    gate.gate.release();
    let events = f.collect(id, receiver, "cancelled");
    assert_eq!(events.len(), 2);
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

#[test]
fn cancellation_after_successful_preflight_prevents_provider_selection() {
    let f = Fixture::new(&[SecretKey::GroqApiKey]);
    let gate = InstalledGate::new(&f.sessions, true);
    let (id, receiver) = f.start(f.session, "input");
    gate.gate.entered();
    assert!(f.registry.cancel(id));
    gate.gate.release();
    assert_eq!(f.collect(id, receiver, "cancelled").len(), 2);
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

#[test]
fn cancellation_wins_over_failed_preflight() {
    let f = Fixture::new(&[]);
    let gate = InstalledGate::new(&f.sessions, true);
    let (id, receiver) = f.start(f.session, "input");
    gate.gate.entered();
    assert!(f.registry.cancel(id));
    gate.gate.release();
    assert_eq!(f.collect(id, receiver, "cancelled").len(), 2);
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

#[test]
fn blocked_credential_lookup_does_not_block_task_registration_or_cancellation() {
    let f = Fixture::new(&[SecretKey::GroqApiKey]);
    let gate = Arc::new(Gate::default());
    let _release_on_panic = ReleaseOnDrop(gate.clone());
    *f.keys.gate.lock().unwrap() = Some(gate.clone());
    let (id, receiver) = f.start(f.session, "input");
    gate.entered();
    assert!(f.registry.contains_for_test(id));
    // The session registry is not held while Stronghold's key store is blocked.
    assert!(f.sessions.0.try_lock().is_ok());
    let accepted = f.registry.cancel(id);
    gate.release();
    assert!(accepted);
    assert_eq!(f.collect(id, receiver, "cancelled").len(), 2);
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

#[test]
fn invalid_sessions_fail_closed_with_exactly_one_terminal() {
    let f = Fixture::new(&[]);
    let conn = f.db.open().unwrap();
    let unregistered = conversation::create_session(&conn).unwrap();
    let closed = conversation::create_session(&conn).unwrap();
    conversation::close_session(&conn, closed).unwrap();
    f.sessions.0.lock().unwrap().extend([closed, 999999]);
    for session in [unregistered, closed, 999999] {
        let (id, receiver) = f.start(session, "input");
        let events = f.collect(id, receiver, "failed");
        assert_eq!(events.last().unwrap()["detail"], "session_invalid");
        assert_eq!(events.len(), 2);
    }
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

#[test]
fn absent_credentials_fail_closed_for_fixed_and_every_multi_target() {
    let f = Fixture::new(&[SecretKey::GroqApiKey]);
    for (mode, targets) in [
        (RoutingMode::Fixed, vec!["gemini"]),
        (RoutingMode::Preferred, vec!["groq", "gemini"]),
        (RoutingMode::Auto, vec!["groq", "gemini"]),
    ] {
        f.configure(mode, &targets);
        let (id, receiver) = f.start(f.session, "input");
        let events = f.collect(id, receiver, "failed");
        assert_eq!(events.last().unwrap()["detail"], "provider_not_configured");
        assert_eq!(events.len(), 2);
    }
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

#[test]
fn preferred_auto_affinity_and_session_history_use_the_same_scheduler_contract() {
    let f = Fixture::new(&[SecretKey::GroqApiKey, SecretKey::GeminiApiKey]);
    // Preferred fallback records real session affinity, with independent invocations.
    f.configure(RoutingMode::Preferred, &["gemini", "groq"]);
    f.gemini_fail.store(true, Ordering::Release);
    let (id, receiver) = f.start(f.session, "first");
    let events = f.collect(id, receiver, "completed");
    let selected: Vec<_> = events
        .iter()
        .filter(|e| e["type"] == "provider_selected")
        .collect();
    assert_eq!(
        selected
            .iter()
            .map(|e| e["provider_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["gemini", "groq"]
    );
    assert!(selected
        .iter()
        .all(|e| e["routing_reason"] == "preferred_order" && e["score"].is_null()));
    f.gemini_fail.store(false, Ordering::Release);
    f.configure(RoutingMode::Auto, &["gemini", "groq"]);
    let (id, receiver) = f.start(f.session, &"x".repeat(4000));
    let events = f.collect(id, receiver, "completed");
    let selected = events
        .iter()
        .find(|e| e["type"] == "provider_selected")
        .unwrap();
    assert_eq!(selected["provider_id"], "groq");
    assert_eq!(selected["routing_reason"], "auto_allocator");
    assert_eq!(selected["score"], 1542); // B2: 1524 + registry 2 + continuity 4*4.
    let other = conversation::create_session(&f.db.open().unwrap()).unwrap();
    f.sessions.0.lock().unwrap().insert(other);
    let (id, receiver) = f.start(other, &"y".repeat(4000));
    let events = f.collect(id, receiver, "completed");
    assert!(events.iter().any(|e| e["type"] == "provider_selected"
        && e["provider_id"] == "gemini"
        && e["routing_reason"] == "auto_allocator"));
    // Explicit Preferred ignores the existing affinity and starts at Gemini again.
    f.configure(RoutingMode::Preferred, &["gemini", "groq"]);
    let (id, receiver) = f.start(f.session, "explicit");
    let events = f.collect(id, receiver, "completed");
    assert!(events.iter().any(|e| e["type"] == "provider_selected"
        && e["provider_id"] == "gemini"
        && e["routing_reason"] == "preferred_order"));
    let requests = f.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    for request in requests.iter() {
        let gemini = request.target.provider_id == "gemini";
        assert_eq!(
            request.target.invocation.model,
            format!("{}-persisted-model", request.target.provider_id)
        );
        assert_eq!(
            request.target.invocation.thinking_level,
            Some(if gemini {
                ThinkingLevel::High
            } else {
                ThinkingLevel::Low
            })
        );
        assert_eq!(
            request
                .target
                .invocation
                .timeouts
                .unwrap()
                .request_timeout_ms,
            if gemini { 12345 } else { 23456 }
        );
    }
    assert_eq!(
        requests[2]
            .history
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "answer"]
    );
    assert!(requests[3].history.is_empty());
    assert_eq!(requests[2].input.len(), 4000);
    assert_eq!(
        conversation::session(&f.db.open().unwrap(), f.session)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        6
    );
}

#[test]
fn cancellation_stays_cancelled_when_preflight_and_history_db_are_unavailable() {
    let mut f = Fixture::new(&[]);
    let blocker = f.dir.join("not-a-directory");
    fs::write(&blocker, b"test").unwrap();
    f.db = Database::for_test(blocker.join("unavailable.sqlite3"));
    let gate = InstalledGate::new(&f.sessions, true);
    let (id, receiver) = f.start(f.session, "input");
    gate.gate.entered();
    // TaskStarted has already reached the channel while failed DB preflight is held.
    let started = receiver.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(started["type"], "task_started");
    assert_eq!(started["taskId"], id.0);
    assert!(f.registry.cancel(id));
    gate.gate.release();
    let terminal = receiver.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(terminal["type"], "task_cancelled");
    assert_eq!(terminal["state"], "cancelled");
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(30)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    assert!(!f.registry.contains_for_test(id));
    assert!(!f.registry.has_foreground_provider_work());
    assert!(f.requests.lock().unwrap().is_empty());
}

#[test]
fn cheap_invalid_input_is_rejected_before_registration() {
    let f = Fixture::new(&[]);
    for (session, message, code) in [
        (f.session, " ".into(), "conversation_input_invalid"),
        (f.session, "x".repeat(4097), "conversation_input_invalid"),
        (0, "input".into(), "session_invalid"),
    ] {
        let channel = Channel::new(|_| Ok(()));
        let result = start_conversation(
            f.registry.clone(),
            f.db.clone(),
            f.runtime.clone(),
            f.store.clone(),
            f.sessions.clone(),
            session,
            message,
            channel,
        );
        assert_eq!(result.unwrap_err(), code);
    }
    assert!(f.registry.active.lock().unwrap().is_empty());
    assert!(!f.registry.has_foreground_provider_work());
    assert!(f.requests.lock().unwrap().is_empty());
}

#[test]
fn routing_status_batches_presence_but_task_revalidates_after_credential_change() {
    let f = Fixture::new(&[SecretKey::GeminiApiKey, SecretKey::GroqApiKey]);
    f.configure(RoutingMode::Auto, &["gemini", "groq"]);
    let mut statuses = f.runtime.scheduler.status();
    statuses
        .iter_mut()
        .find(|status| status.id == "gemini")
        .unwrap()
        .cooldown_ms = 888;
    f.keys.loads.store(0, Ordering::SeqCst);
    let status = super::super::routing_status_from_backend(&f.db, &statuses, &f.store).unwrap();
    assert_eq!(f.keys.loads.load(Ordering::SeqCst), 1);
    let frontend = serde_json::to_value(status).unwrap();
    assert_eq!(frontend["routingMode"], "auto");
    assert_eq!(frontend["targets"][0]["providerId"], "gemini");
    assert_eq!(frontend["targets"][1]["providerId"], "groq");
    assert_eq!(frontend["targets"][0]["cooldownMs"], 888);
    for target in frontend["targets"].as_array().unwrap() {
        assert_eq!(target["configured"], true);
        assert_eq!(target.as_object().unwrap().len(), 4);
    }
    assert!(!frontend.to_string().contains("synthetic-test-credential"));
    f.store.delete_secret(SecretKey::GroqApiKey).unwrap();
    f.keys.loads.store(0, Ordering::SeqCst);
    let (id, receiver) = f.start(f.session, "input");
    let events = f.collect(id, receiver, "failed");
    assert_eq!(events.last().unwrap()["detail"], "provider_not_configured");
    assert_eq!(f.keys.loads.load(Ordering::SeqCst), 1);
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
    f.keys.loads.store(0, Ordering::SeqCst);
    let frontend = serde_json::to_value(
        super::super::routing_status_from_backend(&f.db, &statuses, &f.store).unwrap(),
    )
    .unwrap();
    assert_eq!(frontend["targets"][0]["configured"], true);
    assert_eq!(frontend["targets"][1]["configured"], false);
    assert_eq!(f.keys.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn credential_batch_failure_never_selects_provider_and_status_is_fail_closed() {
    let f = Fixture::new(&[SecretKey::GeminiApiKey, SecretKey::GroqApiKey]);
    f.configure(RoutingMode::Preferred, &["gemini", "groq"]);
    f.keys.unavailable.store(true, Ordering::SeqCst);
    f.keys.loads.store(0, Ordering::SeqCst);
    let status =
        super::super::routing_status_from_backend(&f.db, &f.runtime.scheduler.status(), &f.store)
            .unwrap();
    let frontend = serde_json::to_value(status).unwrap();
    assert!(frontend["targets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|target| target["configured"] == false));
    assert_eq!(f.keys.loads.load(Ordering::SeqCst), 1);
    f.keys.loads.store(0, Ordering::SeqCst);
    let (id, receiver) = f.start(f.session, "input");
    let events = f.collect(id, receiver, "failed");
    assert_eq!(events.last().unwrap()["detail"], "provider_not_configured");
    assert_eq!(events.len(), 2);
    assert_eq!(f.keys.loads.load(Ordering::SeqCst), 1);
    assert!(f.requests.lock().unwrap().is_empty());
    f.no_exchange();
}

// Core-only reference workload for PERF-1A. No WebView, credentials outside the
// synthetic fixture, commercial API, or latency assertion tied to machine speed.
#[test]
fn perf1a_core_baseline_conversation_fixture() {
    let fixture = Fixture::new(&[SecretKey::GroqApiKey]);
    let mut samples = Vec::new();
    for _ in 0..5 {
        let started = std::time::Instant::now();
        let (id, receiver) = fixture.start(fixture.session, "PERF-1A synthetic baseline");
        let registration_us = started.elapsed().as_micros();
        let events = fixture.collect(id, receiver, "completed");
        let persisted_us = started.elapsed().as_micros();
        assert!(events.iter().all(|event| event["taskId"] == id.0));
        assert_eq!(
            events.iter().filter(|event| event["type"] == "provider_selected").count(),
            1
        );
        samples.push(serde_json::json!({
            "taskId": id.0, "registrationUs": registration_us,
            "terminalAndPersistenceUs": persisted_us,
        }));
    }
    assert_eq!(fixture.requests.lock().unwrap().len(), 5);
    let conn = fixture.db.open().unwrap();
    assert_eq!(
        conversation::session(&conn, fixture.session).unwrap().unwrap().messages.len(),
        10
    );
    eprintln!("[PERF-1A Core baseline] {}", serde_json::json!({
        "workload": "real conversation runtime, SQLite and Scheduler; fixture provider",
        "samples": samples,
    }));
}

#[test]
fn headless_conversation_survives_detach_rehydrates_same_id_without_another_provider_call() {
    let fixture = Fixture::new(&[SecretKey::GroqApiKey]);
    let gate = InstalledGate::new(&fixture.sessions, true);
    let channel = Channel::new(|_| Err(std::io::Error::other("WebView destroyed").into()));
    let id = start_conversation_with_policy(fixture.registry.clone(), fixture.db.clone(), fixture.runtime.clone(), fixture.store.clone(), fixture.sessions.clone(), fixture.session, "headless fixture".into(), channel, TaskAttachmentPolicy::HeadlessSafe).unwrap();
    gate.gate.entered();
    fixture.registry.events.detach_main();
    assert_eq!(fixture.sessions.selected().unwrap(), Some(fixture.session));
    assert!(fixture.registry.contains_for_test(id));
    let channel = Channel::new(|_| Ok(()));
    let recovered = fixture.registry.events.attach(id, fixture.session, 0, channel).unwrap();
    assert_eq!(recovered.task_id, id); assert_eq!(recovered.session_id, fixture.session);
    assert!(fixture.requests.lock().unwrap().is_empty());
    gate.gate.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if fixture.registry.events.snapshot(fixture.session).unwrap().state == TaskState::Completed { break; }
        assert!(std::time::Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    let snapshot = fixture.registry.events.snapshot(fixture.session).unwrap();
    assert_eq!(snapshot.task_id, id); assert!(snapshot.terminal.is_some());
    let conn = fixture.db.open().unwrap();
    assert_eq!(conversation::session(&conn, fixture.session).unwrap().unwrap().messages.len(), 2);
    fixture.registry.events.attach(id, fixture.session, snapshot.sequence, Channel::new(|_| Ok(()))).unwrap();
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
}

#[test]
fn headless_same_task_id_is_cancelable_and_shutdown_blocks_new_work() {
    let fixture = Fixture::new(&[SecretKey::GroqApiKey]);
    let gate = InstalledGate::new(&fixture.sessions, false);
    let id = start_conversation_with_policy(fixture.registry.clone(), fixture.db.clone(), fixture.runtime.clone(), fixture.store.clone(), fixture.sessions.clone(), fixture.session, "cancel fixture".into(), Channel::new(|_| Ok(())), TaskAttachmentPolicy::HeadlessSafe).unwrap();
    gate.gate.entered(); fixture.registry.events.detach_main();
    assert!(fixture.registry.cancel(id)); gate.gate.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while fixture.registry.events.snapshot(fixture.session).unwrap().state != TaskState::Cancelled {
        assert!(std::time::Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fixture.requests.lock().unwrap().is_empty());
    fixture.registry.shutdown(); assert!(fixture.registry.register().is_err());
    fixture.sessions.0.lock().unwrap().insert(fixture.session + 1);
    assert_eq!(fixture.sessions.selected().unwrap_err(), "ambiguous_product_session");
}

fn durable_start(f: &Fixture, text: &str) -> TaskId {
    let conn=f.db.open().unwrap();
    conn.execute("UPDATE server_provider_permissions SET enabled=1,free_tier_confirmed=1 WHERE provider_id IN ('groq','gemini')",[]).unwrap();
    let reader=Arc::new(SecretStore::existing_with_key_store(f.dir.join("secrets"),f.keys.clone()));
    start_durable_conversation(f.registry.clone(),f.db.clone(),f.runtime.clone(),reader,f.sessions.clone(),f.session,text.into()).unwrap()
}
fn durable_wait(f:&Fixture,id:TaskId,state:&str)->Value {
    let deadline=std::time::Instant::now()+Duration::from_secs(30);
    loop {
        let task=crate::persistence::conversation_runs::get(&f.db.open().unwrap(),id.0).unwrap();
        if task["state"]==state && f.registry.worker_count()==0 {return task;}
        assert!(std::time::Instant::now()<deadline,"{task}");std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn durable_shared_engine_fixed_preferred_auto_persist_provenance_and_exclude_current_input_from_history() {
    for mode in [RoutingMode::Fixed,RoutingMode::Preferred,RoutingMode::Auto] {
        let f=Fixture::new(&[SecretKey::GroqApiKey,SecretKey::GeminiApiKey]);
        f.configure(mode,if mode==RoutingMode::Fixed{&["groq"]}else{&["gemini","groq"]});
        let bytes=fs::read(f.dir.join("secrets/luna-lr3.stronghold")).unwrap();
        let mut conn=f.db.open().unwrap();conversation::append_exchange_to_session(&mut conn,f.session,"old-user","old-answer").unwrap();
        let id=durable_start(&f,"durable-user");f.registry.events.detach_main();
        let task=durable_wait(&f,id,"completed");assert_eq!(task["result"]["text"],"answer");assert_eq!(task["policy"]["routingMode"],mode.as_str());
        let requests=f.requests.lock().unwrap();assert_eq!(requests.len(),1);assert_eq!(requests[0].input,"durable-user");assert_eq!(requests[0].history.len(),2);drop(requests);
        assert_eq!(conversation::session(&conn,f.session).unwrap().unwrap().messages.len(),4);
        assert_eq!(crate::persistence::conversation_runs::recover(&mut conn).unwrap(),0);
        let selected:String=conn.query_row("SELECT details_json FROM server_events WHERE task_id=?1 AND code='provider_selected'",[id.0],|r|r.get(0)).unwrap();assert!(selected.contains("persisted-model"));
        assert_eq!(fs::read(f.dir.join("secrets/luna-lr3.stronghold")).unwrap(),bytes);
        assert_eq!(task_history::max_id(&conn).unwrap(),id.0);
    }
}
#[test]
fn durable_cancel_keeps_input_rejects_duplicate_and_never_invokes_provider() {
    let f=Fixture::new(&[SecretKey::GroqApiKey]);let gate=InstalledGate::new(&f.sessions,false);
    let id=durable_start(&f,"cancelled-durable-input");gate.gate.entered();
    assert!(start_durable_conversation(f.registry.clone(),f.db.clone(),f.runtime.clone(),f.store.clone(),f.sessions.clone(),f.session,"duplicate".into()).is_err());
    assert!(f.registry.cancel(id));assert!(f.registry.cancel(id));gate.gate.release();
    assert!(durable_wait(&f,id,"cancelled")["result"].is_null());
    let messages=conversation::session(&f.db.open().unwrap(),f.session).unwrap().unwrap().messages;
    assert_eq!(messages.len(),1);assert_eq!(messages[0].content,"cancelled-durable-input");assert!(f.requests.lock().unwrap().is_empty());
    assert!(!f.registry.cancel(id));
}
#[test]
fn durable_permission_gate_precedes_secret_access_and_any_provider_effect() {
    let f=Fixture::new(&[SecretKey::GroqApiKey]);let loads=f.keys.loads.load(Ordering::SeqCst);
    let id=start_durable_conversation(f.registry.clone(),f.db.clone(),f.runtime.clone(),f.store.clone(),f.sessions.clone(),f.session,"blocked-user".into()).unwrap();
    assert_eq!(durable_wait(&f,id,"failed")["error_code"],"free_provider_authorization_required");
    assert_eq!(f.keys.loads.load(Ordering::SeqCst),loads);assert!(f.requests.lock().unwrap().is_empty());
}
