//! E–L local integration evidence. No commercial adapter, UI automation or real
//! credentials. Completion/queue/output/cancellation use explicit acknowledgements.
use super::{
    admission::{AdmissionConfig, TrafficClass},
    operational::ProviderOperationalSnapshot,
    provider::{Provider, ProviderFuture},
    rate::{ClockReading, DailyBudgetPolicy, FixedWindow, LocalRateLimit, RateClock, RatePolicy},
    registry::ProviderRegistry,
    resilience::{CircuitState, JitterSource, ResilienceConfig, TransitionReason},
    scheduler::{Scheduler, SchedulerEvent},
    telemetry::{Fact, InvocationObservation, QuotaDimension, QuotaScope, Timing, UsageDimension},
    types::*,
    ProviderRuntime,
};
use crate::persistence::database::Database;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

// The public error contract does not return SchedulerUsage. This test-only,
// allocation-scoped acknowledgement observes the actual PendingSchedulerAttempt
// commit, rather than confusing Selected, execute entry or HTTP with that ledger.
type CommitLog = Arc<Mutex<Vec<String>>>;
static COMMITS: OnceLock<Mutex<HashMap<usize, CommitLog>>> = OnceLock::new();
pub(super) fn committed_ack(cancel: &AtomicBool, id: &str) {
    let key = cancel as *const AtomicBool as usize;
    let log = COMMITS
        .get()
        .and_then(|logs| logs.lock().unwrap().get(&key).cloned());
    if let Some(log) = log {
        log.lock().unwrap().push(id.into());
    }
}
struct CommitGuard {
    key: usize,
}
impl Drop for CommitGuard {
    fn drop(&mut self) {
        COMMITS.get().unwrap().lock().unwrap().remove(&self.key);
    }
}

#[derive(Default)]
struct Clock(AtomicU64);
impl Clock {
    fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}
impl RateClock for Clock {
    fn now(&self) -> ClockReading {
        let n = self.0.load(Ordering::SeqCst);
        ClockReading {
            monotonic_ms: n,
            unix_ms: Some(1_000_000 + n),
        }
    }
    fn sleep_ms(&self, _: u64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { panic!("these gate scenarios must not initiate backoff") })
    }
}
struct UpperJitter;
impl JitterSource for UpperJitter {
    fn choose(&self, _: u64, upper: u64) -> u64 {
        upper
    }
}

struct Call {
    provider: String,
    input: String,
    attempt: u32,
    structured_worker: bool,
    command: mpsc::UnboundedSender<Action>,
}
enum Action {
    Chunk(String, oneshot::Sender<()>),
    CancelSeen(oneshot::Sender<()>),
    Finish(Result<ProviderResponse, ProviderError>, Option<Timing>),
}
impl Call {
    fn finish(self, result: Result<ProviderResponse, ProviderError>) {
        self.command.send(Action::Finish(result, None)).unwrap();
    }
    fn hinted(self, error: ProviderError, hint: u64) {
        self.command
            .send(Action::Finish(Err(error), Some(Timing::DelayMs(hint))))
            .unwrap();
    }
    async fn chunk(&self, text: &str) {
        let (tx, rx) = oneshot::channel();
        self.command.send(Action::Chunk(text.into(), tx)).unwrap();
        bounded(rx).await.unwrap();
    }
    async fn cancelled(&self) {
        let (tx, rx) = oneshot::channel();
        self.command.send(Action::CancelSeen(tx)).unwrap();
        bounded(rx).await.unwrap();
    }
}
struct ControlledProvider {
    id: String,
    entered: mpsc::UnboundedSender<Call>,
    loopback: Option<String>,
    proof: Option<super::rate::TokenUpperBound>,
}
impl Provider for ControlledProvider {
    fn token_upper_bound(&self, _: &ProviderRequest) -> Option<super::rate::TokenUpperBound> {
        self.proof
    }
    fn supports_invocation(&self, inv: &ProviderInvocationConfig, mode: &InvocationMode) -> bool {
        inv.valid() && mode.valid()
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        panic!("only observed boundary is permitted")
    }
    fn execute_observed<'a>(
        &'a self,
        r: &'a ProviderRequest,
        cancel: &'a AtomicBool,
        chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        obs: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            assert_eq!(self.id, r.target.provider_id);
            assert!(
                obs.started_unless_cancelled(cancel),
                "factual boundary denied"
            );
            if let Some(url) = &self.loopback {
                assert!(url.starts_with("http://127.0.0.1:"));
                // Local-only HTTP boundary for F; no environment proxy/credentials.
                reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap()
                    .get(url)
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap();
            }
            let (command, mut commands) = mpsc::unbounded_channel();
            self.entered
                .send(Call {
                    provider: self.id.clone(),
                    input: r.input.clone(),
                    attempt: r.attempt,
                    structured_worker: r.internal_system_instruction.is_some(),
                    command,
                })
                .unwrap();
            while let Some(action) = commands.recv().await {
                match action {
                    Action::Chunk(text, ack) => {
                        chunk(ProviderChunk { text })?;
                        ack.send(()).unwrap();
                    }
                    Action::CancelSeen(ack) => {
                        assert!(cancel.load(Ordering::Acquire));
                        ack.send(()).unwrap();
                    }
                    Action::Finish(result, hint) => {
                        if let Some(hint) = hint {
                            obs.retry_hint(Some(hint));
                        }
                        if let Ok(response) = &result {
                            obs.final_usage(response.usage);
                        }
                        return result;
                    }
                }
            }
            panic!("test must explicitly complete each controlled invocation")
        })
    }
}
struct Harness {
    scheduler: Arc<Scheduler>,
    clock: Arc<Clock>,
    calls: mpsc::UnboundedReceiver<Call>,
}
impl Harness {
    fn new(db: Option<Database>, loopback: Option<String>) -> Self {
        Self::named(db, loopback, &["a", "b"])
    }
    fn named(db: Option<Database>, loopback: Option<String>, ids: &[&str]) -> Self {
        Self::with_proof(db, loopback, ids, None)
    }
    fn with_proof(
        db: Option<Database>,
        loopback: Option<String>,
        ids: &[&str],
        proof: Option<super::rate::TokenUpperBound>,
    ) -> Self {
        let (entered, calls) = mpsc::unbounded_channel();
        let clock = Arc::new(Clock::default());
        let mut registry = ProviderRegistry::default();
        for (priority, id) in ids.iter().enumerate() {
            registry
                .register(
                    ProviderConfig {
                        id: (*id).into(),
                        enabled: true,
                        priority: priority as u16 + 1,
                        capabilities: ProviderCapabilities::with_structured_output(),
                    },
                    Arc::new(ControlledProvider {
                        id: (*id).into(),
                        entered: entered.clone(),
                        proof,
                        loopback: if priority == 0 {
                            loopback.clone()
                        } else {
                            None
                        },
                    }),
                )
                .unwrap();
        }
        let scheduler = Arc::new(
            Scheduler::with_resilience_config(
                registry,
                AdmissionConfig::default(),
                clock.clone(),
                db,
                ResilienceConfig::default(),
                Arc::new(UpperJitter),
            )
            .unwrap(),
        );
        Self {
            scheduler,
            clock,
            calls,
        }
    }
    async fn call(&mut self) -> Call {
        bounded(self.calls.recv()).await.unwrap()
    }
    fn no_call(&mut self) {
        assert!(self.calls.try_recv().is_err());
    }
}
async fn bounded<T>(f: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), f)
        .await
        .expect("ack deadline (existing LR-8B bound)")
}
fn success(text: &str) -> Result<ProviderResponse, ProviderError> {
    Ok(ProviderResponse {
        text: text.into(),
        usage: ProviderUsage {
            calls: 1,
            input_tokens: 2,
            output_tokens: 3,
            total_tokens: Some(5),
            output_tokens_measured: true,
            ..Default::default()
        },
    })
}
fn request(ids: &[&str], fixed: bool, class: TrafficClass, input: &str) -> ProviderTaskRequest {
    ProviderTaskRequest {
        traffic_class: class,
        mode: InvocationMode::default(),
        input: input.into(),
        internal_system_instruction: None,
        history: vec![],
        context: Arc::new(super::orchestrator::technical_context()),
        max_output_tokens: Some(100),
        selection: if fixed {
            ProviderSelection::Fixed(ids[0].into())
        } else {
            ProviderSelection::Preferred
        },
        targets: ids
            .iter()
            .map(|id| ProviderTarget {
                provider_id: (*id).into(),
                invocation: ProviderInvocationConfig {
                    model: "local-model".into(),
                    thinking_level: None,
                    timeouts: None,
                },
            })
            .collect(),
        affinity_key: None,
        estimated_context_bytes: 0,
        required_capabilities: ProviderCapabilities::text_stream(),
    }
}
fn fixed(input: &str) -> ProviderTaskRequest {
    request(&["a"], true, TrafficClass::ForegroundInteractive, input)
}
fn retry(enabled: bool) -> RetryPolicy {
    RetryPolicy {
        enabled,
        max_retries: 2,
        initial_backoff_ms: 10,
    }
}
struct Run {
    task: tokio::task::JoinHandle<Result<TaskResult, SchedulerError>>,
    events: mpsc::UnboundedReceiver<SchedulerEvent>,
    cancel: Arc<AtomicBool>,
    commits: CommitLog,
    seen: Vec<SchedulerEvent>,
}
impl Run {
    fn start(s: &Arc<Scheduler>, r: ProviderTaskRequest, retry_enabled: bool) -> Self {
        let s = s.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let key = Arc::as_ptr(&cancel) as usize;
        let commits = Arc::new(Mutex::new(vec![]));
        assert!(COMMITS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(key, commits.clone())
            .is_none());
        let guard = CommitGuard { key };
        let flag = cancel.clone();
        let (tx, events) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let _guard = guard;
            s.run_with_retry(
                r,
                TaskBudget {
                    max_provider_calls: 4,
                    max_output_tokens: Some(100),
                },
                retry(retry_enabled),
                &flag,
                &mut |e| {
                    tx.send(e).unwrap();
                    Ok(())
                },
            )
            .await
        });
        Self {
            task,
            events,
            cancel,
            commits,
            seen: vec![],
        }
    }
    async fn queued(&mut self) {
        loop {
            let event = bounded(self.events.recv()).await.unwrap();
            let queued = matches!(event, SchedulerEvent::Queued { .. });
            self.seen.push(event);
            if queued {
                return;
            }
        }
    }
    async fn done(
        mut self,
    ) -> (
        Result<TaskResult, SchedulerError>,
        Vec<SchedulerEvent>,
        Vec<String>,
    ) {
        let result = bounded(self.task).await.unwrap();
        let mut events = self.seen;
        while let Ok(e) = self.events.try_recv() {
            events.push(e);
        }
        let commits = self.commits.lock().unwrap().clone();
        (result, events, commits)
    }
}
fn no_routing(events: &[SchedulerEvent]) {
    assert!(!events.iter().any(|e| matches!(
        e,
        SchedulerEvent::Retry { .. } | SchedulerEvent::Fallback { .. }
    )));
}
fn stable(s: &Scheduler) -> ProviderOperationalSnapshot {
    fn normalized(mut value: serde_json::Value) -> serde_json::Value {
        fn strip(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    m.remove("capturedAtUnixMs");
                    m.remove("updatedAgeMs");
                    for v in m.values_mut() {
                        strip(v);
                    }
                }
                serde_json::Value::Array(a) => {
                    for v in a {
                        strip(v);
                    }
                }
                _ => {}
            }
        }
        strip(&mut value);
        value
    }
    let first = s.operational_snapshot();
    let before = normalized(serde_json::to_value(&first).unwrap());
    for _ in 0..4 {
        assert_eq!(
            normalized(serde_json::to_value(s.operational_snapshot()).unwrap()),
            before
        );
    }
    first
}
fn requests(s: &ProviderOperationalSnapshot, id: &str) -> u64 {
    match s
        .telemetry
        .iter()
        .find(|p| p.provider_id == id)
        .unwrap()
        .usage[&UsageDimension::Requests]
        .observed
    {
        Fact::Known { value, .. } => value,
        Fact::Unknown => panic!("request count is factual"),
    }
}
fn admission<'a>(
    s: &'a ProviderOperationalSnapshot,
    id: &str,
) -> &'a super::admission::AdmissionSnapshot {
    s.admission.iter().find(|p| p.provider_id == id).unwrap()
}
fn rate<'a>(s: &'a ProviderOperationalSnapshot, id: &str) -> &'a super::rate::RateSnapshot {
    s.rate.iter().find(|p| p.provider_id == id).unwrap()
}
fn health<'a>(
    s: &'a ProviderOperationalSnapshot,
    id: &str,
) -> &'a super::resilience::ResilienceSnapshot {
    s.resilience.iter().find(|p| p.provider_id == id).unwrap()
}
fn clean(s: &Scheduler) {
    let snap = stable(s);
    assert!(snap
        .admission
        .iter()
        .all(|p| p.active_calls == 0 && p.queue_depth == 0));
    assert!(snap.rate.iter().all(|p| p.pending_reservations == 0));
    assert!(snap
        .resilience
        .iter()
        .all(|p| p.half_open_probes_active == 0));
}
fn local(capacity: u64) -> RatePolicy {
    RatePolicy {
        limits: vec![LocalRateLimit {
            scope: QuotaScope::Provider,
            dimension: QuotaDimension::RequestsPerMinute,
            capacity,
            window: FixedWindow {
                period_ms: 600_000,
                anchor_unix_ms: 1_000_000,
            },
        }],
        daily_budget: None,
    }
}

#[tokio::test]
async fn lr8e_gate_e_cancel_queued_before_boundary_and_commit() {
    let mut h = Harness::new(None, None);
    h.scheduler.rate.set_policy("a", local(10)).unwrap();
    let a = Run::start(&h.scheduler, fixed("A"), false);
    let call_a = h.call().await;
    let b = Run::start(&h.scheduler, fixed("B"), false);
    let call_b = h.call().await;
    let mut c = Run::start(&h.scheduler, fixed("C"), true);
    c.queued().await;
    let before = stable(&h.scheduler);
    let adm = admission(&before, "a");
    assert_eq!(
        (adm.active_calls, adm.max_concurrency, adm.queue_depth),
        (2, 2, 1)
    );
    assert_eq!(rate(&before, "a").pending_reservations, 3);
    assert_eq!(requests(&before, "a"), 2);
    h.no_call();
    c.cancel.store(true, Ordering::Release);
    let (result, events, commits) = c.done().await;
    assert_eq!(result.unwrap_err(), SchedulerError::Cancelled);
    assert!(commits.is_empty());
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SchedulerEvent::Selected { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SchedulerEvent::Queued { .. }))
            .count(),
        1
    );
    no_routing(&events);
    assert!(!events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::Admitted { .. })));
    let after = stable(&h.scheduler);
    assert_eq!(
        (
            admission(&after, "a").active_calls,
            admission(&after, "a").queue_depth
        ),
        (2, 0)
    );
    assert_eq!(requests(&after, "a"), 2);
    assert_eq!(rate(&after, "a").pending_reservations, 2);
    assert_eq!(rate(&after, "a").constraints[0].reserved, 0);
    assert_eq!(health(&after, "a").half_open_probes_active, 0);
    assert!(!a.task.is_finished() && !b.task.is_finished());
    h.no_call();
    assert_eq!((call_a.input.as_str(), call_b.input.as_str()), ("A", "B"));
    call_a.finish(success("A"));
    call_b.finish(success("B"));
    for run in [a, b] {
        let (r, _, commits) = run.done().await;
        assert_eq!(r.unwrap().usage.provider_calls, 1);
        assert_eq!(commits, vec!["a"]);
    }
    clean(&h.scheduler);
    assert_eq!(requests(&stable(&h.scheduler), "a"), 2);
}

#[tokio::test]
async fn lr8e_gate_f_cancel_after_loopback_http_keeps_accounting() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (http_tx, http_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, peer) = listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let mut bytes = vec![];
        let mut buffer = [0; 512];
        while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            assert!(bytes.len() <= 4096);
        }
        assert!(bytes.starts_with(b"GET / HTTP/1.1\r\n"));
        http_tx.send(()).unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let mut h = Harness::new(None, Some(url));
    h.scheduler.rate.set_policy("a", local(10)).unwrap();
    let run = Run::start(
        &h.scheduler,
        request(&["a", "b"], false, TrafficClass::ForegroundInteractive, "F"),
        true,
    );
    bounded(http_rx).await.unwrap();
    let call = h.call().await;
    bounded(server).await.unwrap();
    let snap = stable(&h.scheduler);
    assert_eq!(requests(&snap, "a"), 1);
    assert_eq!(admission(&snap, "a").active_calls, 1);
    assert_eq!(rate(&snap, "a").constraints[0].consumed, 1);
    run.cancel.store(true, Ordering::Release);
    call.cancelled().await;
    assert!(!run.task.is_finished());
    assert_eq!(admission(&stable(&h.scheduler), "a").active_calls, 1);
    call.finish(Err(ProviderError::Cancelled));
    let (result, events, commits) = run.done().await;
    assert_eq!(result.unwrap_err(), SchedulerError::Cancelled);
    no_routing(&events);
    assert_eq!(commits, vec!["a"]);
    let final_snap = stable(&h.scheduler);
    assert_eq!(requests(&final_snap, "a"), 1);
    assert_eq!(requests(&final_snap, "b"), 0);
    assert_eq!(rate(&final_snap, "a").constraints[0].consumed, 1);
    assert_eq!(health(&final_snap, "a").consecutive_eligible_failures, 0);
    clean(&h.scheduler);
    h.no_call();
}

#[tokio::test]
async fn lr8e_gate_g_background_waits_without_foreground_preemption() {
    let mut h = Harness::new(None, None);
    let a = Run::start(&h.scheduler, fixed("fg-1"), false);
    let ca = h.call().await;
    let b = Run::start(&h.scheduler, fixed("fg-2"), false);
    let cb = h.call().await;
    let mut bg = Run::start(
        &h.scheduler,
        request(&["a"], true, TrafficClass::Background, "bg"),
        false,
    );
    bg.queued().await;
    let snap = stable(&h.scheduler);
    let adm = admission(&snap, "a");
    assert_eq!(
        (
            adm.active_calls,
            adm.queue_depth,
            adm.queued_by_class[&TrafficClass::Background]
        ),
        (2, 1, 1)
    );
    assert!(!a.task.is_finished() && !b.task.is_finished());
    h.no_call();
    ca.finish(success("fg-1"));
    a.done().await.0.unwrap();
    let cbg = h.call().await;
    assert_eq!(cbg.input, "bg");
    assert!(!b.task.is_finished());
    let snap = stable(&h.scheduler);
    assert_eq!(
        (
            admission(&snap, "a").active_calls,
            admission(&snap, "a").queue_depth
        ),
        (2, 0)
    );
    assert_eq!(
        admission(&snap, "a").queued_by_class[&TrafficClass::Background],
        0
    );
    cb.finish(success("fg-2"));
    cbg.finish(success("bg"));
    for run in [b, bg] {
        let (r, events, _) = run.done().await;
        r.unwrap();
        no_routing(&events);
    }
    assert_eq!(requests(&stable(&h.scheduler), "a"), 3);
    clean(&h.scheduler);
}

#[tokio::test]
async fn lr8e_gate_g_admitted_background_survives_new_foreground() {
    let mut h = Harness::new(None, None);
    let bg = Run::start(
        &h.scheduler,
        request(&["a"], true, TrafficClass::Background, "bg"),
        false,
    );
    let cbg = h.call().await;
    let fg = Run::start(&h.scheduler, fixed("fg-1"), false);
    let cfg = h.call().await;
    let mut queued = Run::start(&h.scheduler, fixed("fg-2"), false);
    queued.queued().await;
    let snap = stable(&h.scheduler);
    assert_eq!(
        (
            admission(&snap, "a").active_calls,
            admission(&snap, "a").queue_depth
        ),
        (2, 1)
    );
    assert_eq!(
        admission(&snap, "a").queued_by_class[&TrafficClass::ForegroundInteractive],
        1
    );
    assert!(!bg.task.is_finished() && !bg.cancel.load(Ordering::Acquire));
    h.no_call();
    cfg.finish(success("fg-1"));
    fg.done().await.0.unwrap();
    let next = h.call().await;
    assert!(!bg.task.is_finished());
    assert_eq!(admission(&stable(&h.scheduler), "a").active_calls, 2);
    cbg.finish(success("bg"));
    next.finish(success("fg-2"));
    bg.done().await.0.unwrap();
    queued.done().await.0.unwrap();
    clean(&h.scheduler);
}

#[tokio::test]
async fn lr8e_gate_h_local_capacity_is_pre_http_terminal_for_preferred() {
    let mut h = Harness::new(None, None);
    h.scheduler.rate.set_policy("a", local(1)).unwrap();
    let first = Run::start(&h.scheduler, fixed("first"), true);
    h.call().await.finish(success("first"));
    let result = first.done().await.0.unwrap();
    assert_eq!(result.usage.provider_calls, 1);
    let before = stable(&h.scheduler);
    assert_eq!(requests(&before, "a"), 1);
    assert_eq!(rate(&before, "a").constraints[0].consumed, 1);
    let second = Run::start(
        &h.scheduler,
        request(
            &["a", "b"],
            false,
            TrafficClass::ForegroundInteractive,
            "blocked",
        ),
        true,
    );
    let (r, events, commits) = second.done().await;
    assert_eq!(r.unwrap_err(), SchedulerError::RateCapacityExceeded);
    assert!(commits.is_empty());
    no_routing(&events);
    assert!(!events.iter().any(|e| matches!(
        e,
        SchedulerEvent::Admitted { .. } | SchedulerEvent::Queued { .. }
    )));
    let after = stable(&h.scheduler);
    assert_eq!(requests(&after, "a"), 1);
    assert_eq!(requests(&after, "b"), 0);
    assert_eq!(
        rate(&after, "a").local_blocks,
        rate(&before, "a").local_blocks + 1
    );
    assert_eq!(
        serde_json::to_value(health(&after, "a")).unwrap(),
        serde_json::to_value(health(&before, "a")).unwrap()
    );
    assert_eq!(admission(&after, "a").total_admissions, 1);
    h.no_call();
    clean(&h.scheduler);
}

async fn remote_fallback(error: ProviderError, failures: u64) {
    let mut h = Harness::new(None, None);
    let run = Run::start(
        &h.scheduler,
        request(
            &["a", "b"],
            false,
            TrafficClass::ForegroundInteractive,
            "fallback",
        ),
        true,
    );
    let a = h.call().await;
    assert_eq!((a.provider.as_str(), a.attempt), ("a", 1));
    a.hinted(error, 7_777);
    let b = h.call().await;
    assert_eq!((b.provider.as_str(), b.attempt), ("b", 1));
    let snap = stable(&h.scheduler);
    let ha = health(&snap, "a");
    assert_eq!(ha.cooldown_remaining_ms, 5_000);
    assert_eq!(ha.consecutive_eligible_failures, failures);
    assert_eq!(ha.circuit_state, CircuitState::Closed);
    let ta = snap
        .telemetry
        .iter()
        .find(|p| p.provider_id == "a")
        .unwrap();
    assert!(matches!(
        ta.retry_hint,
        Fact::Known {
            value: Timing::DelayMs(7_777),
            ..
        }
    ));
    assert_eq!((requests(&snap, "a"), requests(&snap, "b")), (1, 1));
    let denied = Run::start(&h.scheduler, fixed("during-cooldown"), true);
    let (r, events, commits) = denied.done().await;
    assert_eq!(r.unwrap_err(), SchedulerError::NoProvider);
    assert!(commits.is_empty());
    no_routing(&events);
    h.no_call();
    b.finish(success("destination"));
    let (result, events, commits) = run.done().await;
    let result = result.unwrap();
    assert_eq!(
        (
            result.usage.provider_calls,
            result.usage.retries,
            result.usage.fallbacks
        ),
        (2, 0, 1)
    );
    assert_eq!(result.usage.providers_used, vec!["a", "b"]);
    assert_eq!(commits, vec!["a", "b"]);
    assert!(!events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::Retry { .. })));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e,SchedulerEvent::Fallback{from,to,..} if from=="a" && to=="b"))
            .count(),
        1
    );
    assert_eq!(
        health(&stable(&h.scheduler), "a").consecutive_eligible_failures,
        failures
    );
    h.clock.advance(5_000);
    let snap = stable(&h.scheduler);
    assert_eq!(health(&snap, "a").cooldown_remaining_ms, 0);
    assert!(matches!(
        snap.telemetry
            .iter()
            .find(|p| p.provider_id == "a")
            .unwrap()
            .retry_hint,
        Fact::Known {
            value: Timing::DelayMs(7_777),
            ..
        }
    ));
    h.no_call();
    clean(&h.scheduler);
}
#[tokio::test]
async fn lr8e_gate_i_rate_limited_fallback_cooldown_without_health_failure() {
    remote_fallback(
        ProviderError::RateLimited {
            retry_after_ms: Some(5_000),
        },
        0,
    )
    .await;
}
#[tokio::test]
async fn lr8e_gate_i_unavailable_fallback_does_not_recover_source() {
    remote_fallback(
        ProviderError::Unavailable {
            retry_after_ms: Some(5_000),
        },
        1,
    )
    .await;
}
#[tokio::test]
async fn lr8e_gate_i_fixed_never_falls_back() {
    let mut h = Harness::new(None, None);
    let run = Run::start(&h.scheduler, fixed("fixed"), true);
    h.call().await.hinted(
        ProviderError::RateLimited {
            retry_after_ms: Some(5_000),
        },
        5_000,
    );
    let (r, events, commits) = run.done().await;
    assert_eq!(
        r.unwrap_err(),
        SchedulerError::Provider(ProviderError::RateLimited {
            retry_after_ms: Some(5_000)
        })
    );
    assert_eq!(commits, vec!["a"]);
    no_routing(&events);
    let snap = stable(&h.scheduler);
    assert_eq!(requests(&snap, "b"), 0);
    assert_eq!(health(&snap, "a").cooldown_remaining_ms, 5_000);
    h.no_call();
    clean(&h.scheduler);
}
#[tokio::test]
async fn lr8e_gate_i_output_forbids_retry_and_fallback() {
    let mut h = Harness::new(None, None);
    let run = Run::start(
        &h.scheduler,
        request(
            &["a", "b"],
            false,
            TrafficClass::ForegroundInteractive,
            "output",
        ),
        true,
    );
    let a = h.call().await;
    a.chunk("first output").await;
    a.finish(Err(ProviderError::Timeout));
    let (r, events, commits) = run.done().await;
    assert_eq!(
        r.unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::Chunk { .. })));
    no_routing(&events);
    assert_eq!(commits, vec!["a"]);
    let snap = stable(&h.scheduler);
    assert_eq!((requests(&snap, "a"), requests(&snap, "b")), (1, 0));
    h.no_call();
    clean(&h.scheduler);
}

async fn eligible_failure(h: &mut Harness, error: ProviderError) {
    let run = Run::start(&h.scheduler, fixed("eligible failure"), false);
    h.call().await.finish(Err(error.clone()));
    let (r, events, commits) = run.done().await;
    assert_eq!(r.unwrap_err(), SchedulerError::Provider(error));
    assert_eq!(commits, vec!["a"]);
    no_routing(&events);
}
#[tokio::test]
async fn lr8e_gate_j_integrated_circuit_single_probe_recovery_and_reopen() {
    let mut h = Harness::new(None, None);
    h.scheduler.rate.set_policy("a", local(20)).unwrap();
    for n in 1..=3 {
        eligible_failure(&mut h, ProviderError::Timeout).await;
        let snap = stable(&h.scheduler);
        assert_eq!(health(&snap, "a").consecutive_eligible_failures, n);
        assert_eq!(requests(&snap, "a"), n);
    }
    let open = stable(&h.scheduler);
    let a = health(&open, "a");
    assert_eq!(
        (a.circuit_state, a.configured_threshold, a.open_remaining_ms),
        (CircuitState::Open, 3, 30_000)
    );
    assert_eq!(
        a.last_transition_reason,
        Some(TransitionReason::FailureThresholdTimeout)
    );
    let blocked = Run::start(&h.scheduler, fixed("open blocked"), true);
    let (r, events, commits) = blocked.done().await;
    assert_eq!(r.unwrap_err(), SchedulerError::NoProvider);
    assert!(events.is_empty() && commits.is_empty());
    let still = stable(&h.scheduler);
    assert_eq!(requests(&still, "a"), 3);
    assert_eq!(admission(&still, "a").total_admissions, 3);
    assert_eq!(rate(&still, "a").pending_reservations, 0);
    assert_eq!(rate(&still, "a").constraints[0].consumed, 3);
    assert_eq!(rate(&still, "a").local_blocks, 0);
    h.no_call();
    h.clock.advance(30_000);
    assert_eq!(
        health(&stable(&h.scheduler), "a").circuit_state,
        CircuitState::Open
    );
    let one = Run::start(&h.scheduler, fixed("probe-one"), false);
    let two = Run::start(&h.scheduler, fixed("probe-two"), false);
    let f1 = one.done();
    let f2 = two.done();
    tokio::pin!(f1);
    tokio::pin!(f2);
    let remaining = tokio::select! {
        blocked=&mut f1 => { assert_eq!(blocked.0.unwrap_err(),SchedulerError::NoProvider); assert!(blocked.1.is_empty() && blocked.2.is_empty()); 2 },
        blocked=&mut f2 => { assert_eq!(blocked.0.unwrap_err(),SchedulerError::NoProvider); assert!(blocked.1.is_empty() && blocked.2.is_empty()); 1 },
    };
    let probe = h.call().await;
    let snap = stable(&h.scheduler);
    let a = health(&snap, "a");
    assert_eq!(
        (
            a.circuit_state,
            a.half_open_probes_active,
            a.half_open_max_probes
        ),
        (CircuitState::HalfOpen, 1, 1)
    );
    assert_eq!(
        a.last_transition_reason,
        Some(TransitionReason::OpenDurationElapsed)
    );
    assert_eq!(admission(&snap, "a").active_calls, 1);
    assert_eq!(admission(&snap, "a").total_admissions, 4);
    assert_eq!(rate(&snap, "a").pending_reservations, 1);
    assert_eq!(requests(&snap, "a"), 4);
    h.no_call();
    probe.finish(success("recovered"));
    let finished = if remaining == 1 { f1.await } else { f2.await };
    finished.0.unwrap();
    assert_eq!(finished.2, vec!["a"]);
    let recovered = stable(&h.scheduler);
    let a = health(&recovered, "a");
    assert_eq!(
        (
            a.circuit_state,
            a.consecutive_eligible_failures,
            a.recovery_count
        ),
        (CircuitState::Closed, 0, 1)
    );
    assert_eq!(
        a.last_transition_reason,
        Some(TransitionReason::ProbeSucceeded)
    );
    clean(&h.scheduler);
    for _ in 0..3 {
        eligible_failure(
            &mut h,
            ProviderError::Unavailable {
                retry_after_ms: None,
            },
        )
        .await;
    }
    assert_eq!(
        health(&stable(&h.scheduler), "a").last_transition_reason,
        Some(TransitionReason::FailureThresholdUnavailable)
    );
    h.clock.advance(30_000);
    let run = Run::start(&h.scheduler, fixed("failed probe"), false);
    let probe = h.call().await;
    assert_eq!(
        health(&stable(&h.scheduler), "a").half_open_probes_active,
        1
    );
    probe.finish(Err(ProviderError::Timeout));
    assert_eq!(
        run.done().await.0.unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    let snap = stable(&h.scheduler);
    let a = health(&snap, "a");
    assert_eq!(a.circuit_state, CircuitState::Open);
    assert_eq!(
        a.last_transition_reason,
        Some(TransitionReason::ProbeTimeout)
    );
    assert_eq!(
        (a.breaker_open_count, a.half_open_count, a.recovery_count),
        (3, 2, 1)
    );
    assert_eq!(requests(&snap, "a"), 8);
    clean(&h.scheduler);
}

struct TempDirectory(std::path::PathBuf);
impl TempDirectory {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lr8e-gate-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> Database {
        Database::for_test(self.0.join("luna.sqlite3"))
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn lr8e_gate_l_restart_keeps_window_daily_budget_and_resets_health() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let policy = RatePolicy {
        daily_budget: Some(DailyBudgetPolicy {
            anchor_unix_ms: 1_000_000,
            max_requests: Some(1),
            max_accounted_tokens: None,
        }),
        ..local(1)
    };
    let mut h = Harness::new(Some(db.clone()), None);
    h.scheduler.rate.set_policy("a", policy.clone()).unwrap();
    let run = Run::start(&h.scheduler, fixed("persisted request"), false);
    h.call().await.hinted(
        ProviderError::Unavailable {
            retry_after_ms: Some(5_000),
        },
        5_000,
    );
    assert_eq!(
        run.done().await.0.unwrap_err(),
        SchedulerError::Provider(ProviderError::Unavailable {
            retry_after_ms: Some(5_000)
        })
    );
    let before = stable(&h.scheduler);
    assert_eq!(requests(&before, "a"), 1);
    assert_eq!(health(&before, "a").cooldown_remaining_ms, 5_000);
    assert_eq!(health(&before, "a").consecutive_eligible_failures, 1);
    assert!(rate(&before, "a")
        .constraints
        .iter()
        .filter(|c| matches!(
            c.dimension,
            QuotaDimension::RequestsPerMinute | QuotaDimension::RequestsPerDay
        ))
        .all(|c| c.consumed == 1));
    clean(&h.scheduler);
    drop(h);
    let mut restarted = Harness::new(Some(db.clone()), None);
    let snap = stable(&restarted.scheduler);
    assert_eq!(rate(&snap, "a").policy, policy);
    assert!(rate(&snap, "a")
        .constraints
        .iter()
        .filter(|c| matches!(
            c.dimension,
            QuotaDimension::RequestsPerMinute | QuotaDimension::RequestsPerDay
        ))
        .all(|c| c.consumed == 1));
    assert_eq!(
        (
            health(&snap, "a").circuit_state,
            health(&snap, "a").cooldown_remaining_ms,
            health(&snap, "a").consecutive_eligible_failures
        ),
        (CircuitState::Closed, 0, 0)
    );
    assert_eq!(requests(&snap, "a"), 0); // factual telemetry is runtime-local, not durable account usage.
    let blocked = Run::start(&restarted.scheduler, fixed("after restart"), true);
    let (r, events, commits) = blocked.done().await;
    assert_eq!(r.unwrap_err(), SchedulerError::RateCapacityExceeded);
    assert!(commits.is_empty());
    no_routing(&events);
    restarted.no_call();
    assert_eq!(requests(&stable(&restarted.scheduler), "a"), 0);
    clean(&restarted.scheduler);
}

#[tokio::test]
async fn lr8e_gate_l_durable_uncertainty_never_becomes_known_credit() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let policy = RatePolicy {
        limits: vec![],
        daily_budget: Some(DailyBudgetPolicy {
            anchor_unix_ms: 1_000_000,
            max_requests: None,
            max_accounted_tokens: Some(100),
        }),
    };
    let mut h = Harness::new(Some(db.clone()), None);
    h.scheduler.rate.set_policy("a", policy.clone()).unwrap();
    let run = Run::start(&h.scheduler, fixed("unmeasured factual timeout"), false);
    h.call().await.finish(Err(ProviderError::Timeout));
    assert_eq!(
        run.done().await.0.unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    fn uncertain(s: &ProviderOperationalSnapshot) {
        let token = rate(s, "a")
            .constraints
            .iter()
            .find(|c| c.dimension == QuotaDimension::TokensPerDay)
            .unwrap();
        assert_eq!(token.unaccounted_token_calls, 1);
        assert_eq!(token.effective_remaining, None);
        assert_eq!(token.consumed, 0);
    }
    uncertain(&stable(&h.scheduler));
    clean(&h.scheduler);
    drop(h);
    // A synthetic bounded fixture provides a proof of 5 total tokens. LR-8C
    // blocks known-bound calls against unresolved credit; unbounded adapters
    // retain their existing unknown semantics and are not claimed blocked.
    let mut restarted = Harness::with_proof(
        Some(db),
        None,
        &["a", "b"],
        Some(super::rate::TokenUpperBound::explicit_total(5).unwrap()),
    );
    let snap = stable(&restarted.scheduler);
    assert_eq!(rate(&snap, "a").policy, policy);
    uncertain(&snap);
    assert_eq!(health(&snap, "a").consecutive_eligible_failures, 0);
    let blocked = Run::start(&restarted.scheduler, fixed("uncertain restart"), true);
    let (r, events, commits) = blocked.done().await;
    assert_eq!(r.unwrap_err(), SchedulerError::RateStateUnavailable);
    assert!(commits.is_empty());
    no_routing(&events);
    restarted.no_call();
    uncertain(&stable(&restarted.scheduler));
    assert_eq!(requests(&stable(&restarted.scheduler), "a"), 0);
    clean(&restarted.scheduler);
}

#[tokio::test]
async fn lr8e_gate_k_real_taskgraph_workers_provenance_and_consolidation() {
    use super::{
        policy::{self, CognitiveRole, CognitiveTargetPolicy, RoutingMode},
        task_graph_runtime::start_task,
        task_graph_runtime_tests::{channel, collect, seed_identity, TestKeys},
    };
    use crate::{
        luna::runtime::TaskRegistry,
        security::secrets::{SecretKey, SecretStore},
    };
    // Same 10 s channel bound as the existing TaskGraph harness; no added delay.
    async fn graph_ack<T>(f: impl Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(10), f)
            .await
            .expect("existing TaskGraph ack bound")
    }
    let dir = TempDirectory::new();
    let db = dir.db();
    seed_identity(&db);
    let store = Arc::new(SecretStore::with_key_store(
        dir.0.clone(),
        Arc::new(TestKeys::default()),
    ));
    store
        .set_secrets(&[
            (SecretKey::GeminiApiKey, b"lr8e-synthetic-key".to_vec()),
            (SecretKey::GroqApiKey, b"lr8e-synthetic-key".to_vec()),
            (
                SecretKey::CloudflareApiToken,
                b"lr8e-synthetic-token".to_vec(),
            ),
            (
                SecretKey::CloudflareAccountId,
                b"lr8e-synthetic-account".to_vec(),
            ),
        ])
        .unwrap();
    let mut conn = db.open().unwrap();
    for (role, mode, targets, calls) in [
        (
            CognitiveRole::Orchestrator,
            RoutingMode::Fixed,
            vec![CognitiveTargetPolicy {
                provider_id: "gemini".into(),
                model: super::gemini::MODEL.into(),
                thinking_level: None,
            }],
            1,
        ),
        (
            CognitiveRole::Worker,
            RoutingMode::Preferred,
            vec![
                CognitiveTargetPolicy {
                    provider_id: "groq".into(),
                    model: super::groq::MODEL.into(),
                    thinking_level: None,
                },
                CognitiveTargetPolicy {
                    provider_id: "cloudflare".into(),
                    model: super::cloudflare::MODEL.into(),
                    thinking_level: None,
                },
            ],
            2,
        ),
    ] {
        let mut p = policy::load(&conn, role).unwrap();
        p.routing_mode = mode;
        p.targets = targets;
        p.max_provider_calls = calls;
        p.retry_enabled = false;
        policy::save(&mut conn, &p).unwrap();
    }
    drop(conn);
    let mut h = Harness::named(Some(db.clone()), None, &["gemini", "groq", "cloudflare"]);
    for id in ["gemini", "groq", "cloudflare"] {
        h.scheduler.rate.set_policy(id, local(20)).unwrap();
    }
    let runtime = Arc::new(ProviderRuntime {
        scheduler: h.scheduler.clone(),
    });
    let registry = Arc::new(TaskRegistry::default());
    let (channel, receiver) = channel();
    let root = start_task(
        registry.clone(),
        db.clone(),
        runtime,
        store,
        "E–L local deterministic graph".into(),
        channel,
    )
    .unwrap();
    let planner = graph_ack(h.calls.recv()).await.unwrap();
    assert_eq!(planner.provider, "gemini");
    let plan=serde_json::json!({"version":1,"objective":"local graph","steps":[
        {"id":"worker-1","description":"Independent A","requiredCapabilities":["planning"],"dependsOn":[]},
        {"id":"worker-2","description":"Independent B","requiredCapabilities":["structured_output"],"dependsOn":[]}],
        "risks":[],"needsUserInput":false,"questions":[]}).to_string();
    // The response goes through the real Orchestrator parser and TaskGraph compiler.
    planner.finish(success(&plan));
    let first = graph_ack(h.calls.recv()).await.unwrap();
    let second = graph_ack(h.calls.recv()).await.unwrap();
    let workers = [first, second];
    let mut ids = workers
        .iter()
        .map(|c| c.provider.as_str())
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["cloudflare", "groq"]);
    let snap = stable(&h.scheduler);
    for id in ["groq", "cloudflare"] {
        assert_eq!(
            (
                admission(&snap, id).active_calls,
                admission(&snap, id).queue_depth
            ),
            (1, 0)
        );
        assert_eq!(rate(&snap, id).pending_reservations, 1);
        assert_eq!(requests(&snap, id), 1);
    }
    assert_eq!(admission(&snap, "gemini").active_calls, 0);
    assert_eq!(requests(&snap, "gemini"), 1);
    let mut expected = HashMap::new();
    for call in workers {
        let id = call
            .input
            .split("SUBTAREFA ")
            .nth(1)
            .unwrap()
            .split(':')
            .next()
            .unwrap()
            .to_owned();
        assert!(matches!(id.as_str(), "worker-1" | "worker-2"));
        assert_eq!(call.attempt, 1);
        let text = format!("result-{id}");
        expected.insert(id.clone(), call.provider.clone());
        // Match the real request's plain/structured Worker contract.
        assert_eq!(call.structured_worker, id == "worker-2");
        let response = if call.structured_worker {
            serde_json::json!({"subtaskId":id,"text":text}).to_string()
        } else {
            text
        };
        call.finish(success(&response));
    }
    let events = graph_ack(tokio::task::spawn_blocking(move || collect(&receiver)))
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = events
        .iter()
        .map(|e| serde_json::from_str(e).unwrap())
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "task_completed")
            .count(),
        1
    );
    assert!(!events.iter().any(|e| e["type"] == "task_failed"
        || e["type"] == "subtask_retry"
        || e["type"] == "provider_fallback"));
    let result = &events
        .iter()
        .find(|e| e["type"] == "task_graph_result_ready")
        .unwrap()["result"];
    assert_eq!(result["subtasks"].as_array().unwrap().len(), 2);
    for item in result["subtasks"].as_array().unwrap() {
        let id = item["subtaskId"].as_str().unwrap();
        assert_eq!(item["providerId"], expected[id]);
        assert_eq!(item["text"], format!("result-{id}"));
    }
    assert_eq!(result["workerUsage"]["providerCalls"], 2);
    assert_eq!(result["workerUsage"]["retries"], 0);
    assert_eq!(result["workerUsage"]["fallbacks"], 0);
    let mut used = result["workerUsage"]["providersUsed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    used.sort();
    assert_eq!(used, vec!["cloudflare", "groq"]);
    let text = result["consolidatedText"].as_str().unwrap();
    assert!(text.find("result-worker-1").unwrap() < text.find("result-worker-2").unwrap());
    let conn = db.open().unwrap();
    let rows:Vec<(String,String)>=conn.prepare("SELECT subtask_id,provider_id FROM task_subtask_records WHERE root_task_id=?1 AND state='completed' ORDER BY subtask_id").unwrap()
        .query_map([root.0],|row|Ok((row.get(0)?,row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        rows,
        vec![
            ("worker-1".into(), expected["worker-1"].clone()),
            ("worker-2".into(), expected["worker-2"].clone())
        ]
    );
    drop(conn);
    assert!(!registry.contains_for_test(root));
    let final_snap = stable(&h.scheduler);
    for id in ["gemini", "groq", "cloudflare"] {
        assert_eq!(requests(&final_snap, id), 1);
        assert_eq!(rate(&final_snap, id).constraints[0].consumed, 1);
    }
    h.no_call();
    clean(&h.scheduler);
}

#[tokio::test]
async fn lr8e_gate_l_daily_request_budget_is_durable_and_enforced() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let policy = RatePolicy {
        limits: vec![],
        daily_budget: Some(DailyBudgetPolicy {
            anchor_unix_ms: 1_000_000,
            max_requests: Some(1),
            max_accounted_tokens: None,
        }),
    };
    let mut h = Harness::new(Some(db.clone()), None);
    h.scheduler.rate.set_policy("a", policy.clone()).unwrap();
    let first = Run::start(&h.scheduler, fixed("daily request"), false);
    h.call().await.finish(success("daily request"));
    first.done().await.0.unwrap();
    let before = stable(&h.scheduler);
    assert_eq!(requests(&before, "a"), 1);
    assert_eq!(rate(&before, "a").constraints[0].consumed, 1);
    clean(&h.scheduler);
    drop(h);
    let mut restarted = Harness::new(Some(db), None);
    let snap = stable(&restarted.scheduler);
    assert_eq!(rate(&snap, "a").policy, policy);
    assert_eq!(rate(&snap, "a").constraints[0].consumed, 1);
    let second = Run::start(&restarted.scheduler, fixed("daily after restart"), true);
    let (result, events, commits) = second.done().await;
    assert_eq!(result.unwrap_err(), SchedulerError::DailyBudgetExceeded);
    assert!(commits.is_empty());
    no_routing(&events);
    restarted.no_call();
    assert_eq!(requests(&stable(&restarted.scheduler), "a"), 0);
    clean(&restarted.scheduler);
}
