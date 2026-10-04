//! LR-8D: fake monotonic clock and explicit acknowledgements, no arbitrary sleeps.
use super::{
    admission::{AdmissionConfig, TrafficClass},
    provider::{Provider, ProviderFuture},
    rate::{
        ClockReading, DailyBudgetPolicy, FixedWindow, LocalRateLimit, RateClock, RatePolicy,
        TokenUpperBound,
    },
    registry::ProviderRegistry,
    resilience::{
        CircuitState, JitterSource, ResilienceConfig, ResilienceManager, ResilienceSnapshot,
        TransitionReason,
    },
    scheduler::{Scheduler, SchedulerEvent},
    telemetry::{
        Fact, InvocationObservation, Provenance, QuotaDimension, QuotaScope, UsageDimension,
        MAX_FACT_VALUE,
    },
    types::*,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, Notify};

#[derive(Default)]
struct Clock {
    mono: AtomicU64,
    wall: AtomicU64,
    wake: Notify,
    waiting: Notify,
}
impl Clock {
    fn advance(&self, ms: u64) {
        self.mono.fetch_add(ms, Ordering::SeqCst);
        self.wake.notify_waiters();
    }
}
impl RateClock for Clock {
    fn now(&self) -> ClockReading {
        ClockReading {
            monotonic_ms: self.mono.load(Ordering::SeqCst),
            unix_ms: Some(self.wall.load(Ordering::SeqCst)),
        }
    }
    fn sleep_ms(&self, ms: u64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let deadline = self.now().monotonic_ms.saturating_add(ms);
            loop {
                let wake = self.wake.notified();
                tokio::pin!(wake);
                wake.as_mut().enable();
                if self.now().monotonic_ms >= deadline {
                    return;
                }
                self.waiting.notify_one();
                wake.await;
            }
        })
    }
}
struct Jitter {
    upper: bool,
    calls: AtomicU64,
}
impl Jitter {
    fn new(upper: bool) -> Arc<Self> {
        Arc::new(Self {
            upper,
            calls: AtomicU64::new(0),
        })
    }
}
impl JitterSource for Jitter {
    fn choose(&self, low: u64, high: u64) -> u64 {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.upper {
            high
        } else {
            low
        }
    }
}
fn config() -> ResilienceConfig {
    ResilienceConfig {
        open_duration_ms: 30,
        ..ResilienceConfig::default()
    }
}
fn manager(
    cfg: ResilienceConfig,
    upper: bool,
) -> (Arc<ResilienceManager>, Arc<Clock>, Arc<Jitter>) {
    let c = Arc::new(Clock::default());
    let j = Jitter::new(upper);
    (
        ResilienceManager::new(["a".into(), "b".into()], cfg, c.clone(), j.clone()).unwrap(),
        c,
        j,
    )
}
fn snap(m: &ResilienceManager, id: &str) -> ResilienceSnapshot {
    m.snapshots()
        .into_iter()
        .find(|s| s.provider_id == id)
        .unwrap()
}
fn outcome(m: &Arc<ResilienceManager>, id: &str, error: Option<ProviderError>) {
    m.authorize(id, 0)
        .expect("authorized")
        .finish(true, error.as_ref());
}
fn open(m: &Arc<ResilienceManager>, id: &str) {
    for _ in 0..snap(m, id).configured_threshold {
        outcome(m, id, Some(ProviderError::Timeout));
    }
}
fn policy(initial: u64) -> RetryPolicy {
    RetryPolicy {
        enabled: true,
        max_retries: 10,
        initial_backoff_ms: initial,
    }
}
fn neutral_errors() -> Vec<ProviderError> {
    vec![
        ProviderError::RateLimited {
            retry_after_ms: None,
        },
        ProviderError::QuotaExceeded,
        ProviderError::Authentication,
        ProviderError::InvalidRequest,
        ProviderError::Fatal,
        ProviderError::Protocol,
        ProviderError::Incomplete,
        ProviderError::RequiresAction,
        ProviderError::OutputLimitExceeded,
        ProviderError::UnsupportedMode,
        ProviderError::RemoteCancelled,
        ProviderError::Cancelled,
        ProviderError::EventSinkClosed,
    ]
}
#[test]
fn resilience_exponential_base_preserved() {
    for (n, base) in [(1, 1500), (2, 3000), (3, 6000), (4, 12000), (5, 24000)] {
        assert_eq!(policy(1500).backoff_ms(n), base);
        let (m, _, _) = manager(config(), true);
        assert_eq!(m.backoff_ms(policy(1500), n), base);
    }
}
#[test]
fn resilience_equal_jitter_lower_bound_ceil() {
    let (m, _, _) = manager(config(), false);
    for (base, low) in [(1, 1), (2, 1), (3, 2), (1001, 501)] {
        assert_eq!(m.backoff_ms(policy(base), 1), low);
    }
}
#[test]
fn resilience_equal_jitter_upper_bound() {
    let (m, _, _) = manager(config(), true);
    assert_eq!(m.backoff_ms(policy(1001), 1), 1001);
}
#[test]
fn resilience_cap_applies_before_jitter_and_exponential_never_wraps() {
    for upper in [false, true] {
        let (m, _, _) = manager(config(), upper);
        for n in [6, 64, 65, u32::MAX] {
            assert_eq!(
                m.backoff_ms(policy(u64::MAX), n),
                if upper { 30000 } else { 15000 }
            );
        }
    }
}
#[test]
fn resilience_zero_backoff_never_calls_rng() {
    let (m, _, j) = manager(config(), true);
    for n in [0, 1, 65, u32::MAX] {
        assert_eq!(m.backoff_ms(policy(0), n), 0);
    }
    assert_eq!(j.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn resilience_config_defaults_validation_and_test_override() {
    let d = ResilienceConfig::default();
    assert_eq!(
        (
            d.failure_threshold,
            d.open_duration_ms,
            d.half_open_max_probes,
            d.max_retry_backoff_ms
        ),
        (3, 30000, 1, 30000)
    );
    for c in [
        ResilienceConfig {
            failure_threshold: 0,
            ..d
        },
        ResilienceConfig {
            open_duration_ms: 0,
            ..d
        },
        ResilienceConfig {
            half_open_max_probes: 0,
            ..d
        },
        ResilienceConfig {
            max_retry_backoff_ms: u64::MAX,
            ..d
        },
    ] {
        assert!(c.validate().is_err());
    }
    let (m, _, _) = manager(
        ResilienceConfig {
            max_retry_backoff_ms: 7,
            failure_threshold: 1,
            open_duration_ms: 1,
            ..d
        },
        false,
    );
    assert_eq!(m.backoff_ms(policy(100), 1), 4);
    open(&m, "a");
    assert_eq!(snap(&m, "a").open_remaining_ms, 1);
}
#[test]
fn resilience_injected_rng_out_of_bounds_is_bounded() {
    struct Bad;
    impl JitterSource for Bad {
        fn choose(&self, _: u64, _: u64) -> u64 {
            u64::MAX
        }
    }
    let m = ResilienceManager::new(
        ["a".into()],
        config(),
        Arc::new(Clock::default()),
        Arc::new(Bad),
    )
    .unwrap();
    assert_eq!(m.backoff_ms(policy(5), 1), 5);
}
#[test]
fn resilience_retry_after_never_calls_jitter() {
    let (m, _, j) = manager(config(), true);
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(12345),
        }),
    );
    assert_eq!(snap(&m, "a").cooldown_remaining_ms, 12345);
    assert_eq!(j.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn resilience_cooldown_larger_extends_deadline() {
    let (m, c, _) = manager(config(), true);
    let a = m.authorize("a", 0).unwrap();
    let b = m.authorize("a", 0).unwrap();
    a.finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(100),
        }),
    );
    c.advance(10);
    b.finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(200),
        }),
    );
    assert_eq!(snap(&m, "a").cooldown_remaining_ms, 200);
}
#[test]
fn resilience_cooldown_smaller_concurrent_result_cannot_shorten() {
    let (m, c, _) = manager(config(), true);
    let a = m.authorize("a", 0).unwrap();
    let b = m.authorize("a", 0).unwrap();
    a.finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(100),
        }),
    );
    c.advance(10);
    b.finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(5),
        }),
    );
    assert_eq!(snap(&m, "a").cooldown_remaining_ms, 90);
}
#[test]
fn resilience_rate_limited_missing_hint_defaults_to_three_seconds() {
    let (m, _, _) = manager(config(), true);
    outcome(
        &m,
        "a",
        Some(ProviderError::RateLimited {
            retry_after_ms: None,
        }),
    );
    assert_eq!(snap(&m, "a").cooldown_remaining_ms, 3000);
}
#[test]
fn resilience_one_hundred_429_never_open_or_require_probe() {
    let (m, c, _) = manager(config(), true);
    for _ in 0..100 {
        outcome(
            &m,
            "a",
            Some(ProviderError::RateLimited {
                retry_after_ms: None,
            }),
        );
        c.advance(3000);
    }
    let s = snap(&m, "a");
    assert_eq!(
        (
            s.circuit_state,
            s.consecutive_eligible_failures,
            s.transition_count
        ),
        (CircuitState::Closed, 0, 0)
    );
    let p = m.authorize("a", 0).unwrap();
    assert_eq!(snap(&m, "a").half_open_probes_active, 0);
    drop(p);
}
#[test]
fn resilience_timeout_increments_eligible_failure() {
    let (m, _, _) = manager(config(), true);
    outcome(&m, "a", Some(ProviderError::Timeout));
    assert_eq!(snap(&m, "a").consecutive_eligible_failures, 1);
}
#[test]
fn resilience_unavailable_without_hint_increments_failure() {
    let (m, _, _) = manager(config(), true);
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: None,
        }),
    );
    assert_eq!(snap(&m, "a").consecutive_eligible_failures, 1);
    assert_eq!(snap(&m, "a").cooldown_remaining_ms, 0);
}
#[test]
fn resilience_unavailable_hint_can_open_and_cool_simultaneously() {
    let (m, _, _) = manager(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        true,
    );
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(100),
        }),
    );
    let s = snap(&m, "a");
    assert_eq!(
        (
            s.circuit_state,
            s.consecutive_eligible_failures,
            s.cooldown_remaining_ms
        ),
        (CircuitState::Open, 1, 100)
    );
    assert_eq!(
        s.last_transition_reason,
        Some(TransitionReason::FailureThresholdUnavailable)
    );
}
#[test]
fn resilience_success_resets_closed_failures() {
    let (m, _, _) = manager(config(), true);
    outcome(&m, "a", Some(ProviderError::Timeout));
    outcome(&m, "a", None);
    assert_eq!(snap(&m, "a").consecutive_eligible_failures, 0);
}
#[test]
fn resilience_exact_threshold_and_below_threshold() {
    let (m, _, _) = manager(config(), true);
    for n in 1..=3 {
        outcome(&m, "a", Some(ProviderError::Timeout));
        let s = snap(&m, "a");
        assert_eq!(s.consecutive_eligible_failures, n);
        assert_eq!(
            s.circuit_state,
            if n == 3 {
                CircuitState::Open
            } else {
                CircuitState::Closed
            }
        );
    }
    assert_eq!(
        snap(&m, "a").last_transition_reason,
        Some(TransitionReason::FailureThresholdTimeout)
    );
}
#[test]
fn resilience_open_half_open_exact_monotonic_boundary_and_wall_clock_irrelevance() {
    let (m, c, _) = manager(config(), true);
    open(&m, "a");
    c.wall.store(MAX_FACT_VALUE, Ordering::SeqCst);
    c.advance(29);
    assert!(m.authorize("a", 0).is_none());
    c.advance(1);
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::Open);
    let p = m.authorize("a", 0).unwrap();
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::HalfOpen);
    assert_eq!(
        snap(&m, "a").last_transition_reason,
        Some(TransitionReason::OpenDurationElapsed)
    );
    drop(p);
}
#[test]
fn resilience_atomic_concurrent_probe_bound() {
    let (m, c, _) = manager(config(), true);
    open(&m, "a");
    c.advance(30);
    let b = Arc::new(std::sync::Barrier::new(3));
    let (tx, rx) = std::sync::mpsc::channel();
    let mut joins = vec![];
    for _ in 0..2 {
        let (m, b, tx) = (m.clone(), b.clone(), tx.clone());
        joins.push(std::thread::spawn(move || {
            b.wait();
            tx.send(m.authorize("a", 0)).unwrap();
        }));
    }
    b.wait();
    let a = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let b = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    assert_eq!(snap(&m, "a").half_open_probes_active, 1);
    drop((a, b));
    for j in joins {
        j.join().unwrap();
    }
    assert_eq!(snap(&m, "a").half_open_probes_active, 0);
}
#[test]
fn resilience_probe_success_closes_and_resets_failures() {
    let (m, c, _) = manager(config(), true);
    open(&m, "a");
    c.advance(30);
    outcome(&m, "a", None);
    let s = snap(&m, "a");
    assert_eq!(
        (
            s.circuit_state,
            s.consecutive_eligible_failures,
            s.half_open_probes_active,
            s.recovery_count
        ),
        (CircuitState::Closed, 0, 0, 1)
    );
    assert_eq!(
        s.last_transition_reason,
        Some(TransitionReason::ProbeSucceeded)
    );
}
#[test]
fn resilience_probe_timeout_reopens_with_fresh_duration() {
    probe_failure(ProviderError::Timeout, TransitionReason::ProbeTimeout);
}
#[test]
fn resilience_probe_unavailable_reopens_with_fresh_duration() {
    probe_failure(
        ProviderError::Unavailable {
            retry_after_ms: None,
        },
        TransitionReason::ProbeUnavailable,
    );
}
fn probe_failure(e: ProviderError, reason: TransitionReason) {
    let (m, c, _) = manager(config(), true);
    open(&m, "a");
    c.advance(30);
    outcome(&m, "a", Some(e));
    let s = snap(&m, "a");
    assert_eq!(
        (
            s.circuit_state,
            s.open_remaining_ms,
            s.half_open_probes_active
        ),
        (CircuitState::Open, 30, 0)
    );
    assert_eq!(s.last_transition_reason, Some(reason));
}
#[test]
fn resilience_every_neutral_probe_releases_without_opening() {
    for error in neutral_errors() {
        let (m, c, _) = manager(config(), true);
        open(&m, "a");
        c.advance(30);
        outcome(&m, "a", Some(error.clone()));
        let s = snap(&m, "a");
        assert_eq!(s.circuit_state, CircuitState::HalfOpen, "{error:?}");
        assert_eq!(s.half_open_probes_active, 0);
        assert_eq!(s.breaker_open_count, 1);
        assert_eq!(s.consecutive_eligible_failures, 3);
        c.advance(3000);
        assert!(m.authorize("a", 0).is_some());
    }
}
#[test]
fn resilience_preflight_without_http_cannot_fail_or_close_probe() {
    for error in [
        None,
        Some(ProviderError::Authentication),
        Some(ProviderError::Timeout),
        Some(ProviderError::Unavailable {
            retry_after_ms: None,
        }),
    ] {
        let (m, c, _) = manager(config(), true);
        m.authorize("a", 0).unwrap().finish(false, error.as_ref());
        assert_eq!(snap(&m, "a").consecutive_eligible_failures, 0);
        open(&m, "a");
        c.advance(30);
        m.authorize("a", 0).unwrap().finish(false, error.as_ref());
        assert_eq!(snap(&m, "a").circuit_state, CircuitState::HalfOpen);
        assert_eq!(snap(&m, "a").half_open_probes_active, 0);
    }
}
#[test]
fn resilience_neutral_closed_errors_never_increment_or_reset() {
    let (m, c, _) = manager(config(), true);
    outcome(&m, "a", Some(ProviderError::Timeout));
    for error in neutral_errors() {
        outcome(&m, "a", Some(error));
        c.advance(3000);
        assert_eq!(snap(&m, "a").consecutive_eligible_failures, 1);
        assert_eq!(snap(&m, "a").circuit_state, CircuitState::Closed);
    }
}
#[test]
fn resilience_old_generation_results_and_permits_cannot_mutate_new_era() {
    for error in [
        ProviderError::Timeout,
        ProviderError::Unavailable {
            retry_after_ms: Some(100),
        },
        ProviderError::RateLimited {
            retry_after_ms: Some(100),
        },
    ] {
        let (m, _, _) = manager(config(), true);
        let p = m.authorize("a", 0).unwrap();
        m.invalidate("a", 1);
        p.finish(true, Some(&error));
        let s = snap(&m, "a");
        assert_eq!(
            (
                s.circuit_state,
                s.consecutive_eligible_failures,
                s.cooldown_remaining_ms
            ),
            (CircuitState::Closed, 0, 0)
        );
        assert!(m.authorize("a", 0).is_none());
        assert!(m.authorize("a", 1).is_some());
    }
}
#[test]
fn resilience_rotation_clears_circuit_cooldown_and_old_probe_cannot_close_or_release_new_probe() {
    let (m, c, _) = manager(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        true,
    );
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(50),
        }),
    );
    m.invalidate("a", 1);
    let s = snap(&m, "a");
    assert_eq!(
        (
            s.circuit_state,
            s.cooldown_remaining_ms,
            s.consecutive_eligible_failures
        ),
        (CircuitState::Closed, 0, 0)
    );
    m.authorize("a", 1)
        .unwrap()
        .finish(true, Some(&ProviderError::Timeout));
    c.advance(30);
    let old = m.authorize("a", 1).unwrap();
    m.invalidate("a", 2);
    m.authorize("a", 2)
        .unwrap()
        .finish(true, Some(&ProviderError::Timeout));
    c.advance(30);
    let new = m.authorize("a", 2).unwrap();
    old.finish(true, None);
    assert_eq!(snap(&m, "a").half_open_probes_active, 1);
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::HalfOpen);
    drop(new);
}
#[test]
fn resilience_previous_circuit_epoch_cannot_close_new_probe() {
    let (m, c, _) = manager(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        true,
    );
    let old = m.authorize("a", 0).unwrap();
    open(&m, "a");
    c.advance(30);
    let probe = m.authorize("a", 0).unwrap();
    old.finish(true, None);
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::HalfOpen);
    probe.finish(true, None);
}
#[test]
fn resilience_open_before_cooldown_expiry_does_not_spend_probe() {
    let (m, c, _) = manager(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        true,
    );
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(100),
        }),
    );
    c.advance(30);
    assert!(m.authorize("a", 0).is_none());
    let s = snap(&m, "a");
    assert_eq!(
        (
            s.circuit_state,
            s.half_open_probes_active,
            s.half_open_count
        ),
        (CircuitState::Open, 0, 0)
    );
    c.advance(70);
    let p = m.authorize("a", 0).unwrap();
    assert!(m.authorize("a", 0).is_none());
    assert_eq!(snap(&m, "a").half_open_probes_active, 1);
    drop(p);
}
#[test]
fn resilience_cooldown_before_open_expiry_stays_open() {
    let (m, c, _) = manager(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        true,
    );
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(5),
        }),
    );
    c.advance(5);
    assert_eq!(snap(&m, "a").cooldown_remaining_ms, 0);
    assert!(!m.eligible("a"));
    assert!(m.authorize("a", 0).is_none());
    assert_eq!(snap(&m, "a").half_open_count, 0);
}
#[test]
fn resilience_provider_isolation_destination_success_never_closes_source() {
    let (m, _, _) = manager(config(), true);
    open(&m, "a");
    outcome(&m, "b", None);
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::Open);
    assert_eq!(snap(&m, "b").consecutive_eligible_failures, 0);
}
#[test]
fn resilience_deadline_overflow_is_flagged_bounded_and_fail_closed() {
    let (m, c, _) = manager(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        true,
    );
    c.mono.store(u64::MAX - 1, Ordering::SeqCst);
    outcome(
        &m,
        "a",
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(u64::MAX),
        }),
    );
    c.mono.store(u64::MAX, Ordering::SeqCst);
    let s = snap(&m, "a");
    assert!(s.saturated);
    assert_eq!(
        (s.open_remaining_ms, s.cooldown_remaining_ms),
        (MAX_FACT_VALUE, MAX_FACT_VALUE)
    );
    assert!(!m.eligible("a"));
}
#[test]
fn resilience_snapshot_is_read_only_allowlisted_and_content_free() {
    let (m, c, _) = manager(config(), true);
    open(&m, "a");
    c.advance(30);
    let first = serde_json::to_value(m.snapshots()).unwrap();
    assert_eq!(first, serde_json::to_value(m.snapshots()).unwrap());
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::Open);
    for marker in [
        "prompt",
        "output",
        "reasoning",
        "headers",
        "error_body",
        "api_key",
        "account_id",
        "authorization",
    ] {
        assert!(!first.to_string().contains(marker));
    }
    assert_eq!(first[0].as_object().unwrap().len(), 14);
}
#[tokio::test]
async fn resilience_aborted_future_drops_probe() {
    let (m, c, _) = manager(config(), true);
    open(&m, "a");
    c.advance(30);
    let (tx, rx) = oneshot::channel();
    let cloned = m.clone();
    let task = tokio::spawn(async move {
        let _probe = cloned.authorize("a", 0).unwrap();
        tx.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    bounded(rx).await.unwrap();
    assert_eq!(snap(&m, "a").half_open_probes_active, 1);
    task.abort();
    assert!(bounded(task).await.unwrap_err().is_cancelled());
    assert_eq!(snap(&m, "a").half_open_probes_active, 0);
    assert!(m.authorize("a", 0).is_some());
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), future)
        .await
        .expect("synchronization deadline")
}

#[derive(Debug)]
struct Completion {
    error: Option<ProviderError>,
    partial: bool,
    total: Option<u32>,
}
impl Completion {
    fn success() -> Self {
        Self {
            error: None,
            partial: false,
            total: Some(3),
        }
    }
    fn error(error: ProviderError) -> Self {
        Self {
            error: Some(error),
            partial: false,
            total: None,
        }
    }
}
struct Call {
    id: String,
    attempt: u32,
    finish: oneshot::Sender<Completion>,
}
struct Gate {
    id: String,
    tx: mpsc::UnboundedSender<Call>,
    preflight: Option<ProviderError>,
}
impl Provider for Gate {
    fn token_upper_bound(&self, _: &ProviderRequest) -> Option<TokenUpperBound> {
        Some(TokenUpperBound::explicit_total(20).unwrap())
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async { panic!("observer boundary required") })
    }
    fn execute_observed<'a>(
        &'a self,
        r: &'a ProviderRequest,
        c: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        o: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if let Some(error) = &self.preflight {
                return Err(error.clone());
            }
            if !o.started_unless_cancelled(c) {
                return Err(ProviderError::Cancelled);
            }
            assert!(o.was_started());
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(Call {
                    id: self.id.clone(),
                    attempt: r.attempt,
                    finish: tx,
                })
                .unwrap();
            let result = tokio::select! {result=rx=>result.expect("completion acknowledgement"),_=super::transport::cancellation(c)=>return Err(ProviderError::Cancelled)};
            if result.partial {
                on_chunk(ProviderChunk {
                    text: "private-output-marker".into(),
                })?;
            }
            let usage = ProviderUsage {
                calls: 1,
                input_tokens: 0,
                output_tokens: result.total.unwrap_or(0),
                total_tokens: result.total,
                output_tokens_measured: result.total.is_some(),
                ..Default::default()
            };
            if result.total.is_some() {
                o.final_usage(usage);
            }
            if let Some(e) = result.error {
                return Err(e);
            }
            Ok(ProviderResponse {
                text: "private-output-marker".into(),
                usage,
            })
        })
    }
}
fn registry(tx: mpsc::UnboundedSender<Call>, preflight: Option<ProviderError>) -> ProviderRegistry {
    let mut r = ProviderRegistry::default();
    for (id, priority) in [("a", 1), ("b", 2), ("c", 0)] {
        r.register(
            ProviderConfig {
                id: id.into(),
                priority,
                enabled: true,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(Gate {
                id: id.into(),
                tx: tx.clone(),
                preflight: preflight.clone(),
            }),
        )
        .unwrap();
    }
    r
}
fn harness(
    cfg: ResilienceConfig,
    admission: AdmissionConfig,
    preflight: Option<ProviderError>,
) -> (
    Arc<Scheduler>,
    Arc<Clock>,
    Arc<Jitter>,
    mpsc::UnboundedReceiver<Call>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let c = Arc::new(Clock::default());
    let j = Jitter::new(false);
    let s = Scheduler::with_resilience_config(
        registry(tx, preflight),
        admission,
        c.clone(),
        None,
        cfg,
        j.clone(),
    )
    .unwrap();
    (Arc::new(s), c, j, rx)
}
fn default_harness() -> (
    Arc<Scheduler>,
    Arc<Clock>,
    Arc<Jitter>,
    mpsc::UnboundedReceiver<Call>,
) {
    harness(config(), AdmissionConfig::default(), None)
}
fn request(ids: &[&str], selection: ProviderSelection) -> ProviderTaskRequest {
    ProviderTaskRequest {
        traffic_class: TrafficClass::ForegroundInteractive,
        mode: InvocationMode::default(),
        input: "private-prompt-marker".into(),
        internal_system_instruction: Some("private-reasoning-marker".into()),
        history: vec![],
        context: Arc::new(super::orchestrator::technical_context()),
        max_output_tokens: None,
        selection,
        targets: ids
            .iter()
            .map(|id| ProviderTarget {
                provider_id: (*id).into(),
                invocation: ProviderInvocationConfig {
                    model: "m".into(),
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
fn fixed() -> ProviderTaskRequest {
    request(&["a", "b"], ProviderSelection::Fixed("a".into()))
}
fn budget() -> TaskBudget {
    TaskBudget {
        max_provider_calls: 5,
        max_output_tokens: None,
    }
}
fn no_retry() -> RetryPolicy {
    RetryPolicy {
        enabled: false,
        max_retries: 0,
        initial_backoff_ms: 0,
    }
}
type Run = tokio::task::JoinHandle<Result<TaskResult, SchedulerError>>;
fn start(
    s: Arc<Scheduler>,
    r: ProviderTaskRequest,
    c: Arc<AtomicBool>,
    retry: RetryPolicy,
) -> (Run, mpsc::UnboundedReceiver<SchedulerEvent>) {
    start_budget(s, r, c, retry, budget())
}
fn start_budget(
    s: Arc<Scheduler>,
    r: ProviderTaskRequest,
    c: Arc<AtomicBool>,
    retry: RetryPolicy,
    b: TaskBudget,
) -> (Run, mpsc::UnboundedReceiver<SchedulerEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        tokio::spawn(async move {
            s.run_with_retry(r, b, retry, &c, &mut |e| {
                tx.send(e).unwrap();
                Ok(())
            })
            .await
        }),
        rx,
    )
}
async fn event(
    rx: &mut mpsc::UnboundedReceiver<SchedulerEvent>,
    wanted: impl Fn(&SchedulerEvent) -> bool,
) -> SchedulerEvent {
    loop {
        let e = bounded(rx.recv()).await.unwrap();
        if wanted(&e) {
            return e;
        }
    }
}
fn clean(s: &Scheduler) {
    assert!(s
        .rate_snapshot()
        .iter()
        .all(|r| r.pending_reservations == 0));
    assert!(s
        .admission_snapshot()
        .iter()
        .all(|a| a.active_calls == 0 && a.queue_depth == 0));
    assert!(s
        .resilience_snapshot()
        .iter()
        .all(|r| r.half_open_probes_active == 0));
}
fn factual(s: &Scheduler, id: &str) -> u64 {
    match &s
        .telemetry_snapshot()
        .iter()
        .find(|s| s.provider_id == id)
        .unwrap()
        .usage[&UsageDimension::Requests]
        .observed
    {
        Fact::Known { value, .. } => *value,
        Fact::Unknown => panic!("requests must be factual"),
    }
}
async fn fail_once(
    s: Arc<Scheduler>,
    calls: &mut mpsc::UnboundedReceiver<Call>,
    error: ProviderError,
) {
    let (run, _events) = start(s, fixed(), Arc::new(AtomicBool::new(false)), no_retry());
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Completion::error(error.clone()))
        .unwrap();
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::Provider(error)
    );
}
async fn succeed(
    s: Arc<Scheduler>,
    calls: &mut mpsc::UnboundedReceiver<Call>,
    r: ProviderTaskRequest,
) -> (TaskResult, Vec<SchedulerEvent>) {
    let (run, mut events) = start(s, r, Arc::new(AtomicBool::new(false)), no_retry());
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Completion::success())
        .unwrap();
    let result = bounded(run).await.unwrap().unwrap();
    let mut all = vec![];
    while let Ok(e) = events.try_recv() {
        all.push(e);
    }
    (result, all)
}
fn rpm(capacity: u64) -> RatePolicy {
    RatePolicy {
        limits: vec![LocalRateLimit {
            scope: QuotaScope::Provider,
            dimension: QuotaDimension::RequestsPerMinute,
            capacity,
            window: FixedWindow {
                period_ms: 60000,
                anchor_unix_ms: 0,
            },
        }],
        daily_budget: None,
    }
}
fn daily(capacity: u64) -> RatePolicy {
    RatePolicy {
        limits: vec![],
        daily_budget: Some(DailyBudgetPolicy {
            anchor_unix_ms: 0,
            max_requests: Some(capacity),
            max_accounted_tokens: None,
        }),
    }
}
fn probe_ready(s: &Scheduler, c: &Clock) {
    open(&s.resilience, "a");
    c.advance(30);
}
#[tokio::test]
async fn resilience_open_blocks_before_rate_admission_provider_and_task_call_budget() {
    let (s, _, _, mut calls) = default_harness();
    s.rate.set_policy("a", rpm(1)).unwrap();
    open(&s.resilience, "a");
    assert_eq!(
        s.run(fixed(), budget(), &AtomicBool::new(false), &mut |_| panic!(
            "blocked provider emits no selection/admission"
        ))
        .await
        .unwrap_err(),
        SchedulerError::NoProvider
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "a"), 0);
    assert_eq!(s.admission_snapshot()[0].total_admissions, 0);
    assert_eq!(s.rate_snapshot()[0].constraints[0].consumed, 0);
    clean(&s);
    let (result, _) = succeed(
        s.clone(),
        &mut calls,
        request(&["a", "b"], ProviderSelection::Preferred),
    )
    .await;
    assert_eq!(result.usage.provider_calls, 1);
    assert_eq!(result.usage.retries, 0);
    assert_eq!(result.usage.fallbacks, 0);
}
#[tokio::test]
async fn resilience_fixed_open_has_no_hidden_alternative() {
    let (s, _, _, mut calls) = default_harness();
    open(&s.resilience, "a");
    assert_eq!(
        s.run(fixed(), budget(), &AtomicBool::new(false), &mut |_| Ok(()))
            .await
            .unwrap_err(),
        SchedulerError::NoProvider
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "b"), 0);
}
#[tokio::test]
async fn resilience_preferred_skips_open_preserves_authorized_order() {
    let (s, _, _, mut calls) = default_harness();
    open(&s.resilience, "a");
    let (result, events) = succeed(
        s.clone(),
        &mut calls,
        request(&["a", "b", "c"], ProviderSelection::Preferred),
    )
    .await;
    assert_eq!(result.provider_id, "b");
    assert!(
        matches!(&events[0],SchedulerEvent::Selected{provider_id,routing_reason:"preferred_order",score:None,..}if provider_id=="b")
    );
    assert_eq!(factual(&s, "c"), 0);
}
#[tokio::test]
async fn resilience_auto_gates_after_unchanged_score_and_ranking() {
    let (s, _, _, mut calls) = default_harness();
    let r = request(&["a", "b", "c"], ProviderSelection::Auto);
    assert_eq!(
        s.ranked_provider_ids(&r.selection, &r.targets, &r.required_capabilities)
            .unwrap(),
        vec!["a", "b", "c"]
    );
    open(&s.resilience, "a");
    assert_eq!(
        s.ranked_provider_ids(&r.selection, &r.targets, &r.required_capabilities)
            .unwrap(),
        vec!["b", "c"]
    );
    let (result, events) = succeed(s, &mut calls, r).await;
    assert_eq!(result.provider_id, "b");
    assert!(matches!(
        &events[0],
        SchedulerEvent::Selected {
            score: Some(230),
            ..
        }
    ));
}
#[tokio::test]
async fn resilience_temporary_open_does_not_erase_affinity_or_change_scores() {
    let (s, c, _, mut calls) = default_harness();
    let mut r = request(&["b"], ProviderSelection::Fixed("b".into()));
    r.affinity_key = Some("session".into());
    succeed(s.clone(), &mut calls, r).await;
    open(&s.resilience, "b");
    let mut auto = request(&["a", "b"], ProviderSelection::Auto);
    auto.affinity_key = Some("session".into());
    auto.estimated_context_bytes = 8192;
    let (result, events) = succeed(s.clone(), &mut calls, auto).await;
    assert_eq!(result.provider_id, "a");
    assert!(matches!(
        &events[0],
        SchedulerEvent::Selected {
            score: Some(231),
            ..
        }
    ));
    // That real success legitimately refreshes session affinity. Use another key
    // to prove an operationally blocked attempt alone never deletes continuity.
    let mut b = request(&["b"], ProviderSelection::Fixed("b".into()));
    b.affinity_key = Some("other".into());
    c.advance(30);
    succeed(s.clone(), &mut calls, b).await;
    open(&s.resilience, "b");
    let mut blocked = request(&["a", "b"], ProviderSelection::Fixed("b".into()));
    blocked.affinity_key = Some("other".into());
    assert_eq!(
        s.run(blocked, budget(), &AtomicBool::new(false), &mut |_| Ok(()))
            .await
            .unwrap_err(),
        SchedulerError::NoProvider
    );
    c.advance(30);
    let mut r = request(&["a", "b"], ProviderSelection::Auto);
    r.affinity_key = Some("other".into());
    r.estimated_context_bytes = 8192;
    let (result, events) = succeed(s.clone(), &mut calls, r).await;
    assert_eq!(result.provider_id, "b");
    assert!(matches!(
        &events[0],
        SchedulerEvent::Selected {
            routing_reason: "auto_affinity",
            score: Some(380),
            ..
        }
    ));
    clean(&s);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resilience_two_tasks_exactly_one_half_open_probe_before_other_gates() {
    let (s, c, _, mut calls) = default_harness();
    probe_ready(&s, &c);
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let (tx, mut results) = mpsc::unbounded_channel();
    let mut tasks = vec![];
    for _ in 0..2 {
        let (s, b, tx) = (s.clone(), barrier.clone(), tx.clone());
        tasks.push(tokio::spawn(async move {
            b.wait().await;
            tx.send(
                s.run(fixed(), budget(), &AtomicBool::new(false), &mut |_| Ok(()))
                    .await,
            )
            .unwrap();
        }));
    }
    barrier.wait().await;
    let call = bounded(calls.recv()).await.unwrap();
    assert_eq!(
        bounded(results.recv()).await.unwrap().unwrap_err(),
        SchedulerError::NoProvider
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(s.rate_snapshot()[0].pending_reservations, 1);
    assert_eq!(s.admission_snapshot()[0].total_admissions, 1);
    assert_eq!(factual(&s, "a"), 1);
    call.finish.send(Completion::success()).unwrap();
    assert_eq!(
        bounded(results.recv())
            .await
            .unwrap()
            .unwrap()
            .usage
            .provider_calls,
        1
    );
    for task in tasks {
        bounded(task).await.unwrap();
    }
    assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Closed);
    clean(&s);
}
#[tokio::test]
async fn resilience_cancellation_during_backoff_prevents_new_attempt() {
    let (s, c, _, mut calls) = default_harness();
    let cancel = Arc::new(AtomicBool::new(false));
    let (run, mut events) = start(s.clone(), fixed(), cancel.clone(), policy(1000));
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Completion::error(ProviderError::Timeout))
        .unwrap();
    event(&mut events, |e| matches!(e, SchedulerEvent::Retry { .. })).await;
    bounded(c.waiting.notified()).await;
    clean(&s);
    cancel.store(true, Ordering::Release);
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::Cancelled
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "a"), 1);
    assert_eq!(snap(&s.resilience, "a").consecutive_eligible_failures, 1);
    clean(&s);
}
#[tokio::test]
async fn resilience_breaker_open_while_backoff_prevents_retry() {
    let (s, c, _, mut calls) = default_harness();
    let (run, mut events) = start(
        s.clone(),
        fixed(),
        Arc::new(AtomicBool::new(false)),
        policy(10),
    );
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Completion::error(ProviderError::Timeout))
        .unwrap();
    event(&mut events, |e| matches!(e, SchedulerEvent::Retry { .. })).await;
    bounded(c.waiting.notified()).await;
    outcome(&s.resilience, "a", Some(ProviderError::Timeout));
    outcome(&s.resilience, "a", Some(ProviderError::Timeout));
    c.advance(5);
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "a"), 1);
    clean(&s);
}
#[tokio::test]
async fn resilience_cooldown_during_backoff_prevents_retry_and_allows_authorized_fallback() {
    let (s, c, _, mut calls) = default_harness();
    let (run, mut events) = start(
        s.clone(),
        request(&["a", "b"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        policy(10),
    );
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Completion::error(ProviderError::Timeout))
        .unwrap();
    event(&mut events, |e| matches!(e, SchedulerEvent::Retry { .. })).await;
    bounded(c.waiting.notified()).await;
    outcome(
        &s.resilience,
        "a",
        Some(ProviderError::RateLimited {
            retry_after_ms: Some(100),
        }),
    );
    c.advance(5);
    let call = bounded(calls.recv()).await.unwrap();
    assert_eq!(call.id, "b");
    call.finish.send(Completion::success()).unwrap();
    let result = bounded(run).await.unwrap().unwrap();
    assert_eq!(
        (
            result.usage.provider_calls,
            result.usage.retries,
            result.usage.fallbacks
        ),
        (2, 0, 1)
    );
    assert_eq!(factual(&s, "a"), 1);
    clean(&s);
}
#[tokio::test]
async fn resilience_retry_releases_every_resource_then_reacquires_and_accounts_exactly() {
    let (s, c, j, mut calls) = default_harness();
    s.rate.set_policy("a", rpm(3)).unwrap();
    let (run, mut events) = start(
        s.clone(),
        fixed(),
        Arc::new(AtomicBool::new(false)),
        policy(10),
    );
    let a = bounded(calls.recv()).await.unwrap();
    assert_eq!(a.attempt, 1);
    a.finish
        .send(Completion::error(ProviderError::Timeout))
        .unwrap();
    event(&mut events, |e| matches!(e, SchedulerEvent::Retry { .. })).await;
    bounded(c.waiting.notified()).await;
    clean(&s);
    c.advance(4);
    assert!(calls.try_recv().is_err());
    c.advance(1);
    let b = bounded(calls.recv()).await.unwrap();
    assert_eq!(b.attempt, 2);
    b.finish.send(Completion::success()).unwrap();
    let result = bounded(run).await.unwrap().unwrap();
    assert_eq!((result.usage.provider_calls, result.usage.retries), (2, 1));
    assert_eq!(j.calls.load(Ordering::SeqCst), 1);
    assert_eq!(s.rate_snapshot()[0].constraints[0].consumed, 2);
    assert_eq!(snap(&s.resilience, "a").consecutive_eligible_failures, 0);
    assert_eq!(factual(&s, "a"), 2);
    clean(&s);
}
#[tokio::test]
async fn resilience_retry_after_preserves_fallback_and_fixed_without_jitter() {
    for error in [
        ProviderError::RateLimited {
            retry_after_ms: None,
        },
        ProviderError::RateLimited {
            retry_after_ms: Some(15),
        },
        ProviderError::Unavailable {
            retry_after_ms: Some(15),
        },
    ] {
        for selection in [
            ProviderSelection::Fixed("a".into()),
            ProviderSelection::Preferred,
            ProviderSelection::Auto,
        ] {
            let (s, _, j, mut calls) = default_harness();
            let (run, _events) = start(
                s.clone(),
                request(&["a", "b"], selection.clone()),
                Arc::new(AtomicBool::new(false)),
                policy(100),
            );
            bounded(calls.recv())
                .await
                .unwrap()
                .finish
                .send(Completion::error(error.clone()))
                .unwrap();
            if matches!(selection, ProviderSelection::Fixed(_)) {
                assert_eq!(
                    bounded(run).await.unwrap().unwrap_err(),
                    SchedulerError::Provider(error.clone())
                );
            } else {
                let call = bounded(calls.recv()).await.unwrap();
                assert_eq!(call.id, "b");
                call.finish.send(Completion::success()).unwrap();
                let result = bounded(run).await.unwrap().unwrap();
                assert_eq!(
                    (
                        result.usage.provider_calls,
                        result.usage.retries,
                        result.usage.fallbacks
                    ),
                    (2, 0, 1)
                );
            }
            assert_eq!(j.calls.load(Ordering::SeqCst), 0);
            assert_eq!(
                snap(&s.resilience, "a").cooldown_remaining_ms,
                if matches!(
                    error,
                    ProviderError::RateLimited {
                        retry_after_ms: None
                    }
                ) {
                    3000
                } else {
                    15
                }
            );
            clean(&s);
        }
    }
}
#[tokio::test]
async fn resilience_fallback_destination_does_not_share_source_breaker() {
    let (s, _, _, mut calls) = harness(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        AdmissionConfig::default(),
        None,
    );
    let (run, _events) = start(
        s.clone(),
        request(&["a", "b"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        policy(0),
    );
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Completion::error(ProviderError::Timeout))
        .unwrap();
    let call = bounded(calls.recv()).await.unwrap();
    assert_eq!(call.id, "b");
    assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Open);
    assert_eq!(snap(&s.resilience, "b").consecutive_eligible_failures, 0);
    call.finish.send(Completion::success()).unwrap();
    let result = bounded(run).await.unwrap().unwrap();
    assert_eq!(result.usage.fallbacks, 1);
    assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Open);
    assert_eq!(snap(&s.resilience, "b").circuit_state, CircuitState::Closed);
    clean(&s);
}
#[tokio::test]
async fn resilience_partial_output_still_prevents_retry_and_fallback() {
    for error in [
        ProviderError::Timeout,
        ProviderError::Unavailable {
            retry_after_ms: None,
        },
        ProviderError::RateLimited {
            retry_after_ms: None,
        },
    ] {
        let (s, _, _, mut calls) = default_harness();
        let (run, mut events) = start(
            s.clone(),
            request(&["a", "b"], ProviderSelection::Preferred),
            Arc::new(AtomicBool::new(false)),
            policy(0),
        );
        let call = bounded(calls.recv()).await.unwrap();
        call.finish
            .send(Completion {
                error: Some(error.clone()),
                partial: true,
                total: None,
            })
            .unwrap();
        assert_eq!(
            bounded(run).await.unwrap().unwrap_err(),
            SchedulerError::Provider(error)
        );
        assert!(calls.try_recv().is_err());
        assert_eq!(factual(&s, "a"), 1);
        assert_eq!(factual(&s, "b"), 0);
        while let Ok(e) = events.try_recv() {
            assert!(!matches!(
                e,
                SchedulerEvent::Retry { .. } | SchedulerEvent::Fallback { .. }
            ));
        }
        clean(&s);
    }
}
#[tokio::test]
async fn resilience_cancelled_and_aborted_running_probe_never_leak() {
    for abort in [false, true] {
        let (s, c, _, mut calls) = default_harness();
        probe_ready(&s, &c);
        let cancel = Arc::new(AtomicBool::new(false));
        let (run, _events) = start(s.clone(), fixed(), cancel.clone(), no_retry());
        let call = bounded(calls.recv()).await.unwrap();
        assert_eq!(snap(&s.resilience, "a").half_open_probes_active, 1);
        if abort {
            run.abort();
            assert!(bounded(run).await.unwrap_err().is_cancelled());
        } else {
            cancel.store(true, Ordering::Release);
            assert_eq!(
                bounded(run).await.unwrap().unwrap_err(),
                SchedulerError::Cancelled
            );
        }
        drop(call);
        assert_eq!(
            snap(&s.resilience, "a").circuit_state,
            CircuitState::HalfOpen
        );
        clean(&s);
        assert!(s.resilience.authorize("a", 0).is_some());
    }
}
#[tokio::test]
async fn resilience_rate_capacity_and_daily_block_release_probe_neutrally() {
    for (is_daily, policy, error) in [
        (false, rpm(0), SchedulerError::RateCapacityExceeded),
        (true, daily(0), SchedulerError::DailyBudgetExceeded),
    ] {
        let (s, c, _, mut calls) = default_harness();
        s.rate.set_policy("a", policy).unwrap();
        probe_ready(&s, &c);
        assert_eq!(
            s.run(fixed(), budget(), &AtomicBool::new(false), &mut |_| Ok(()))
                .await
                .unwrap_err(),
            error
        );
        assert!(calls.try_recv().is_err());
        let health = snap(&s.resilience, "a");
        assert_eq!(
            (health.circuit_state, health.consecutive_eligible_failures),
            (CircuitState::HalfOpen, 3)
        );
        clean(&s);
        s.rate
            .set_policy("a", if is_daily { daily(2) } else { rpm(2) })
            .unwrap();
        let (result, _) = succeed(s.clone(), &mut calls, fixed()).await;
        assert_eq!(result.usage.provider_calls, 1);
        assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Closed);
    }
}
#[tokio::test]
async fn resilience_admission_queue_full_and_timeout_release_probe() {
    for timeout in [false, true] {
        let (s, c, _, mut calls) = harness(
            config(),
            AdmissionConfig {
                max_concurrency_per_provider: 1,
                queue_capacity_per_provider: if timeout { 1 } else { 0 },
                queue_timeout_ms: 1,
                ..AdmissionConfig::default()
            },
            None,
        );
        let hold = s
            .admission
            .acquire(
                "a",
                TrafficClass::ForegroundTask,
                &AtomicBool::new(false),
                &mut |_| Ok(()),
            )
            .await
            .unwrap();
        probe_ready(&s, &c);
        assert_eq!(
            bounded(s.run(fixed(), budget(), &AtomicBool::new(false), &mut |_| Ok(())))
                .await
                .unwrap_err(),
            if timeout {
                SchedulerError::AdmissionTimeout
            } else {
                SchedulerError::AdmissionQueueFull
            }
        );
        assert!(calls.try_recv().is_err());
        assert_eq!(
            snap(&s.resilience, "a").circuit_state,
            CircuitState::HalfOpen
        );
        drop(hold);
        clean(&s);
        succeed(s.clone(), &mut calls, fixed()).await;
    }
}
#[tokio::test]
async fn resilience_event_sink_closed_before_provider_releases_probe_at_each_stage() {
    for stage in ["selected", "queued", "admitted"] {
        let (s, c, _, mut calls) = harness(
            config(),
            AdmissionConfig {
                max_concurrency_per_provider: 1,
                ..AdmissionConfig::default()
            },
            None,
        );
        let held = if stage == "queued" {
            Some(
                s.admission
                    .acquire(
                        "a",
                        TrafficClass::ForegroundTask,
                        &AtomicBool::new(false),
                        &mut |_| Ok(()),
                    )
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        probe_ready(&s, &c);
        let result = s
            .run(fixed(), budget(), &AtomicBool::new(false), &mut |e| {
                s.resilience_snapshot();
                s.rate_snapshot();
                s.telemetry_snapshot();
                s.admission_snapshot();
                if matches!(
                    (stage, e),
                    ("selected", SchedulerEvent::Selected { .. })
                        | ("queued", SchedulerEvent::Queued { .. })
                        | ("admitted", SchedulerEvent::Admitted { .. })
                ) {
                    Err(SchedulerError::EventSinkClosed)
                } else {
                    Ok(())
                }
            })
            .await;
        assert_eq!(result.unwrap_err(), SchedulerError::EventSinkClosed);
        assert!(calls.try_recv().is_err());
        assert_eq!(factual(&s, "a"), 0);
        assert_eq!(
            snap(&s.resilience, "a").circuit_state,
            CircuitState::HalfOpen
        );
        drop(held);
        clean(&s);
    }
}
#[tokio::test]
async fn resilience_local_preflight_timeout_and_authentication_cannot_change_health() {
    for error in [
        ProviderError::Timeout,
        ProviderError::Unavailable {
            retry_after_ms: None,
        },
        ProviderError::Authentication,
    ] {
        let (s, c, _, mut calls) =
            harness(config(), AdmissionConfig::default(), Some(error.clone()));
        for probe in [false, true] {
            if probe {
                probe_ready(&s, &c);
            }
            assert_eq!(
                s.run_with_retry(
                    fixed(),
                    budget(),
                    no_retry(),
                    &AtomicBool::new(false),
                    &mut |_| Ok(())
                )
                .await
                .unwrap_err(),
                SchedulerError::Provider(error.clone())
            );
            assert_eq!(factual(&s, "a"), 0);
            let h = snap(&s.resilience, "a");
            assert_eq!(h.consecutive_eligible_failures, if probe { 3 } else { 0 });
            assert_eq!(
                h.circuit_state,
                if probe {
                    CircuitState::HalfOpen
                } else {
                    CircuitState::Closed
                }
            );
            assert!(calls.try_recv().is_err());
            clean(&s);
        }
    }
}
#[tokio::test]
async fn resilience_actual_scheduler_neutral_results_never_open_probe() {
    for error in neutral_errors() {
        let (s, c, _, mut calls) = default_harness();
        probe_ready(&s, &c);
        let (run, _events) = start(
            s.clone(),
            fixed(),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        bounded(calls.recv())
            .await
            .unwrap()
            .finish
            .send(Completion::error(error.clone()))
            .unwrap();
        let result = bounded(run).await.unwrap();
        assert!(result.is_err());
        assert_eq!(
            snap(&s.resilience, "a").circuit_state,
            CircuitState::HalfOpen,
            "{error:?}"
        );
        assert_eq!(snap(&s.resilience, "a").breaker_open_count, 1);
        assert_eq!(factual(&s, "a"), 1);
        clean(&s);
    }
}
#[tokio::test]
async fn resilience_running_call_is_not_preempted_by_peer_open_or_allowed_to_close_it() {
    let (s, _, _, mut calls) = harness(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        AdmissionConfig::default(),
        None,
    );
    let (run, _events) = start(
        s.clone(),
        fixed(),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let pending = bounded(calls.recv()).await.unwrap();
    fail_once(s.clone(), &mut calls, ProviderError::Timeout).await;
    assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Open);
    pending.finish.send(Completion::success()).unwrap();
    bounded(run).await.unwrap().unwrap();
    assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Open);
    assert_eq!(factual(&s, "a"), 2);
    clean(&s);
}
#[tokio::test]
async fn resilience_task_budget_limits_retries_and_never_counts_skipped_open_target() {
    let (s, _, _, mut calls) = default_harness();
    let (run, mut events) = start_budget(
        s.clone(),
        fixed(),
        Arc::new(AtomicBool::new(false)),
        policy(0),
        TaskBudget {
            max_provider_calls: 2,
            max_output_tokens: None,
        },
    );
    for n in 1..=2 {
        let call = bounded(calls.recv()).await.unwrap();
        assert_eq!(call.attempt, n);
        call.finish
            .send(Completion::error(ProviderError::Timeout))
            .unwrap();
    }
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    assert!(calls.try_recv().is_err());
    let mut retries = 0;
    while let Ok(e) = events.try_recv() {
        retries += usize::from(matches!(e, SchedulerEvent::Retry { .. }));
    }
    assert_eq!(retries, 1);
    assert_eq!(factual(&s, "a"), 2);
    clean(&s);
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lr8d-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> crate::persistence::database::Database {
        crate::persistence::database::Database::for_test(self.0.join("state.sqlite3"))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[tokio::test]
async fn resilience_rate_state_unavailable_releases_probe_without_failure() {
    let dir = Directory::new();
    let db = dir.db();
    let (tx, mut calls) = mpsc::unbounded_channel();
    let c = Arc::new(Clock::default());
    let s = Arc::new(
        Scheduler::with_resilience_config(
            registry(tx, None),
            AdmissionConfig::default(),
            c.clone(),
            Some(db.clone()),
            config(),
            Jitter::new(false),
        )
        .unwrap(),
    );
    s.rate.set_policy("a", daily(4)).unwrap();
    db.open().unwrap().execute_batch("CREATE TRIGGER deny_rate BEFORE UPDATE ON cognitive_rate_state BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    probe_ready(&s, &c);
    assert_eq!(
        s.run(fixed(), budget(), &AtomicBool::new(false), &mut |_| Ok(()))
            .await
            .unwrap_err(),
        SchedulerError::RateStateUnavailable
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(
        snap(&s.resilience, "a").circuit_state,
        CircuitState::HalfOpen
    );
    assert_eq!(snap(&s.resilience, "a").consecutive_eligible_failures, 3);
    clean(&s);
}
#[tokio::test]
async fn resilience_restart_clears_health_preserves_durable_daily_windows_and_uncertainty() {
    let dir = Directory::new();
    let db = dir.db();
    let c = Arc::new(Clock::default());
    let (tx, mut calls) = mpsc::unbounded_channel();
    let s = Arc::new(
        Scheduler::with_resilience_config(
            registry(tx, None),
            AdmissionConfig::default(),
            c.clone(),
            Some(db.clone()),
            config(),
            Jitter::new(false),
        )
        .unwrap(),
    );
    s.rate
        .set_policy(
            "a",
            RatePolicy {
                daily_budget: Some(DailyBudgetPolicy {
                    anchor_unix_ms: 0,
                    max_requests: Some(5),
                    max_accounted_tokens: Some(100),
                }),
                ..Default::default()
            },
        )
        .unwrap();
    fail_once(
        s.clone(),
        &mut calls,
        ProviderError::Unavailable {
            retry_after_ms: Some(100),
        },
    )
    .await;
    // An unknown-bound HTTP call makes durable token uncertainty through the
    // unchanged LR-8C boundary (without pretending it reported terminal usage).
    let generation = s.telemetry.context_generation("a").unwrap();
    let observation = s.telemetry.attempt("a");
    let reservation = s
        .rate
        .reserve("a", "m", generation, None, &AtomicBool::new(false))
        .unwrap();
    observation.attach_rate(reservation.handle());
    assert!(observation.started());
    observation.finished(Some(&ProviderError::Timeout));
    drop(reservation);
    drop(observation);
    let before = serde_json::to_value(s.rate_snapshot()).unwrap();
    let live = snap(&s.resilience, "a");
    assert_eq!(live.cooldown_remaining_ms, 100);
    assert_eq!(live.consecutive_eligible_failures, 1);
    drop(s);
    let (tx, _) = mpsc::unbounded_channel();
    let fresh = Scheduler::with_resilience_config(
        registry(tx, None),
        AdmissionConfig::default(),
        c,
        Some(db),
        config(),
        Jitter::new(false),
    )
    .unwrap();
    let health = snap(&fresh.resilience, "a");
    assert_eq!(
        (
            health.circuit_state,
            health.cooldown_remaining_ms,
            health.consecutive_eligible_failures,
            health.transition_count
        ),
        (CircuitState::Closed, 0, 0, 0)
    );
    let after = serde_json::to_value(fresh.rate_snapshot()).unwrap();
    assert_eq!(before, after);
    assert!(fresh.rate_snapshot()[0]
        .constraints
        .iter()
        .any(|b| b.unaccounted_token_calls > 0));
}
#[tokio::test]
async fn resilience_late_http_generation_error_is_factual_but_ignored_for_current_health() {
    for error in [
        ProviderError::Timeout,
        ProviderError::Unavailable {
            retry_after_ms: Some(50),
        },
    ] {
        let (s, _, _, mut calls) = default_harness();
        let (run, _events) = start(
            s.clone(),
            fixed(),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        let call = bounded(calls.recv()).await.unwrap();
        s.invalidate_rate_context("a");
        call.finish.send(Completion::error(error.clone())).unwrap();
        assert_eq!(
            bounded(run).await.unwrap().unwrap_err(),
            SchedulerError::Provider(error)
        );
        assert_eq!(factual(&s, "a"), 1);
        let h = snap(&s.resilience, "a");
        assert_eq!(
            (
                h.circuit_state,
                h.cooldown_remaining_ms,
                h.consecutive_eligible_failures
            ),
            (CircuitState::Closed, 0, 0)
        );
        assert_eq!(s.rate_snapshot()[0].context_generation, 1);
        clean(&s);
    }
}
#[tokio::test]
async fn resilience_cancellation_or_abort_while_probe_queued_releases_both_guards() {
    for abort in [false, true] {
        let (s, c, _, mut calls) = harness(
            config(),
            AdmissionConfig {
                max_concurrency_per_provider: 1,
                ..AdmissionConfig::default()
            },
            None,
        );
        let hold = s
            .admission
            .acquire(
                "a",
                TrafficClass::ForegroundTask,
                &AtomicBool::new(false),
                &mut |_| Ok(()),
            )
            .await
            .unwrap();
        probe_ready(&s, &c);
        let cancel = Arc::new(AtomicBool::new(false));
        let (run, mut events) = start(s.clone(), fixed(), cancel.clone(), no_retry());
        event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
        assert_eq!(snap(&s.resilience, "a").half_open_probes_active, 1);
        if abort {
            run.abort();
            assert!(bounded(run).await.unwrap_err().is_cancelled());
        } else {
            cancel.store(true, Ordering::Release);
            assert_eq!(
                bounded(run).await.unwrap().unwrap_err(),
                SchedulerError::Cancelled
            );
        }
        assert!(calls.try_recv().is_err());
        assert_eq!(factual(&s, "a"), 0);
        drop(hold);
        clean(&s);
        assert!(s.resilience.authorize("a", 0).is_some());
    }
}
#[tokio::test]
async fn resilience_queued_closed_attempt_rechecks_open_before_invoking_adapter() {
    let (s, _, _, mut calls) = harness(
        ResilienceConfig {
            failure_threshold: 1,
            ..config()
        },
        AdmissionConfig {
            max_concurrency_per_provider: 1,
            ..AdmissionConfig::default()
        },
        None,
    );
    let hold = s
        .admission
        .acquire(
            "a",
            TrafficClass::ForegroundTask,
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .await
        .unwrap();
    let (run, mut events) = start(
        s.clone(),
        fixed(),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
    open(&s.resilience, "a");
    drop(hold);
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::NoProvider
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "a"), 0);
    clean(&s);
}
#[tokio::test]
async fn resilience_post_http_core_budget_rejection_cannot_degrade_health() {
    let (s, c, _, mut calls) = default_harness();
    probe_ready(&s, &c);
    let (run, _events) = start_budget(
        s.clone(),
        fixed(),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
        TaskBudget {
            max_provider_calls: 1,
            max_output_tokens: Some(1),
        },
    );
    let call = bounded(calls.recv()).await.unwrap();
    call.finish.send(Completion::success()).unwrap();
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::BudgetExceeded
    );
    assert_eq!(snap(&s.resilience, "a").circuit_state, CircuitState::Closed);
    assert_eq!(snap(&s.resilience, "a").consecutive_eligible_failures, 0);
    clean(&s);
}

struct BeforeHttp {
    entered: mpsc::UnboundedSender<oneshot::Sender<()>>,
}
impl Provider for BeforeHttp {
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async { panic!("observed path required") })
    }
    fn execute_observed<'a>(
        &'a self,
        _: &'a ProviderRequest,
        c: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        o: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.entered.send(tx).unwrap();
            rx.await.unwrap();
            if !o.started_unless_cancelled(c) {
                return Err(ProviderError::Cancelled);
            }
            panic!("HTTP must be denied")
        })
    }
}
#[tokio::test]
async fn resilience_http_boundary_rechecks_health_and_era_after_adapter_preflight() {
    for gate in ["open", "cooldown", "rotation"] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let c = Arc::new(Clock::default());
        let mut r = ProviderRegistry::default();
        r.register(
            ProviderConfig {
                id: "a".into(),
                priority: 1,
                enabled: true,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(BeforeHttp { entered: tx }),
        )
        .unwrap();
        let s = Arc::new(
            Scheduler::with_resilience_config(
                r,
                AdmissionConfig::default(),
                c,
                None,
                config(),
                Jitter::new(false),
            )
            .unwrap(),
        );
        s.rate.set_policy("a", rpm(3)).unwrap();
        let (run, _events) = start(
            s.clone(),
            request(&["a"], ProviderSelection::Fixed("a".into())),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        let proceed = bounded(rx.recv()).await.unwrap();
        match gate {
            "open" => open(&s.resilience, "a"),
            "cooldown" => outcome(
                &s.resilience,
                "a",
                Some(ProviderError::RateLimited {
                    retry_after_ms: Some(100),
                }),
            ),
            _ => s.invalidate_rate_context("a"),
        };
        proceed.send(()).unwrap();
        assert_eq!(
            bounded(run).await.unwrap().unwrap_err(),
            if gate == "rotation" {
                SchedulerError::RateContextChanged
            } else {
                SchedulerError::NoProvider
            }
        );
        assert_eq!(factual(&s, "a"), 0);
        assert_eq!(s.rate_snapshot()[0].constraints[0].consumed, 0);
        clean(&s);
    }
}
#[derive(Default)]
struct Keys(Mutex<Option<Vec<u8>>>);
impl crate::security::secrets::UnlockKeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, crate::security::secrets::SecretError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn store(&self, v: &[u8]) -> Result<(), crate::security::secrets::SecretError> {
        *self.0.lock().unwrap() = Some(v.to_vec());
        Ok(())
    }
    fn delete(&self) -> Result<(), crate::security::secrets::SecretError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
#[test]
fn resilience_real_secret_observer_noop_and_rotation_respect_health_and_budgets() {
    use crate::security::secrets::{SecretKey, SecretStore};
    let dir = Directory::new();
    let store = SecretStore::with_key_store(dir.0.join("secrets"), Arc::new(Keys::default()));
    let (tx, _) = mpsc::unbounded_channel();
    let mut r = ProviderRegistry::default();
    for (id, priority) in [("groq", 1), ("cloudflare", 2)] {
        r.register(
            ProviderConfig {
                id: id.into(),
                priority,
                enabled: true,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(Gate {
                id: id.into(),
                tx: tx.clone(),
                preflight: None,
            }),
        )
        .unwrap();
    }
    let runtime = super::ProviderRuntime {
        scheduler: Arc::new(
            Scheduler::with_resilience_config(
                r,
                AdmissionConfig::default(),
                Arc::new(Clock::default()),
                None,
                config(),
                Jitter::new(false),
            )
            .unwrap(),
        ),
    };
    runtime.connect_credentials(&store);
    runtime.scheduler.rate.set_policy("groq", daily(5)).unwrap();
    store
        .set_secret(
            SecretKey::GroqApiKey,
            b"synthetic-private-credential-marker",
        )
        .unwrap();
    let gen = runtime
        .scheduler
        .telemetry
        .context_generation("groq")
        .unwrap();
    let p = runtime.scheduler.resilience.authorize("groq", gen).unwrap();
    p.finish(
        true,
        Some(&ProviderError::Unavailable {
            retry_after_ms: Some(100),
        }),
    );
    let before = serde_json::to_value(runtime.scheduler.resilience_snapshot()).unwrap();
    store
        .set_secret(
            SecretKey::GroqApiKey,
            b"synthetic-private-credential-marker",
        )
        .unwrap();
    assert_eq!(
        runtime.scheduler.telemetry.context_generation("groq"),
        Some(gen)
    );
    let after = runtime.scheduler.resilience_snapshot();
    assert_eq!(after[1].consecutive_eligible_failures, 1);
    assert!(after[1].cooldown_remaining_ms > 0);
    assert_eq!(
        before[1]["transitionCount"],
        serde_json::to_value(&after[1]).unwrap()["transitionCount"]
    );
    let rate_before =
        serde_json::to_value(runtime.scheduler.rate_snapshot()[1].constraints.clone()).unwrap();
    store
        .set_secret(SecretKey::GroqApiKey, b"replacement-synthetic-marker")
        .unwrap();
    let health = snap(&runtime.scheduler.resilience, "groq");
    assert_eq!(
        (
            health.circuit_state,
            health.cooldown_remaining_ms,
            health.consecutive_eligible_failures
        ),
        (CircuitState::Closed, 0, 0)
    );
    assert_eq!(
        runtime.scheduler.telemetry.context_generation("groq"),
        Some(gen + 1)
    );
    assert_eq!(
        rate_before,
        serde_json::to_value(runtime.scheduler.rate_snapshot()[1].constraints.clone()).unwrap()
    );
    for key in [
        SecretKey::CloudflareApiToken,
        SecretKey::CloudflareAccountId,
    ] {
        let gen = runtime
            .scheduler
            .telemetry
            .context_generation("cloudflare")
            .unwrap();
        runtime
            .scheduler
            .resilience
            .authorize("cloudflare", gen)
            .unwrap()
            .finish(
                true,
                Some(&ProviderError::RateLimited {
                    retry_after_ms: None,
                }),
            );
        store
            .set_secret(key, b"synthetic-private-context-marker")
            .unwrap();
        assert_eq!(
            snap(&runtime.scheduler.resilience, "cloudflare").cooldown_remaining_ms,
            0
        );
    }
    let json = serde_json::to_string(&runtime.scheduler.resilience_snapshot()).unwrap();
    for marker in [
        "synthetic-private-credential-marker",
        "replacement-synthetic-marker",
        "synthetic-private-context-marker",
    ] {
        assert!(!json.contains(marker));
    }
}
#[tokio::test]
async fn resilience_factual_retry_hint_is_never_operational_policy() {
    let (s, _, j, mut calls) = default_harness();
    s.telemetry.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerMinute,
        None,
        None,
        None,
        Provenance::ProviderHeader,
    );
    let observation = s.telemetry.attempt("a");
    assert!(observation.started());
    observation.retry_hint(Some(super::telemetry::Timing::DelayMs(50000)));
    observation.finished(None);
    assert_eq!(snap(&s.resilience, "a").cooldown_remaining_ms, 0);
    assert!(s.resilience.eligible("a"));
    succeed(s.clone(), &mut calls, fixed()).await;
    assert_eq!(j.calls.load(Ordering::SeqCst), 0);
    assert_eq!(snap(&s.resilience, "a").cooldown_remaining_ms, 0);
}
#[test]
fn resilience_probe_configuration_is_bounded_even_above_one() {
    let (m, c, _) = manager(
        ResilienceConfig {
            half_open_max_probes: 2,
            ..config()
        },
        true,
    );
    open(&m, "a");
    c.advance(30);
    let a = m.authorize("a", 0).unwrap();
    let b = m.authorize("a", 0).unwrap();
    assert!(m.authorize("a", 0).is_none());
    assert_eq!(snap(&m, "a").half_open_probes_active, 2);
    drop(a);
    assert_eq!(snap(&m, "a").half_open_probes_active, 1);
    let a = m.authorize("a", 0).unwrap();
    a.finish(true, Some(&ProviderError::Timeout));
    b.finish(true, None);
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::Open);
    assert_eq!(snap(&m, "a").half_open_probes_active, 0);
}
#[tokio::test]
async fn resilience_admitted_callback_cancellation_releases_probe_before_http() {
    let (s, c, _, mut calls) = default_harness();
    probe_ready(&s, &c);
    let cancel = AtomicBool::new(false);
    let result = s
        .run(fixed(), budget(), &cancel, &mut |e| {
            if matches!(e, SchedulerEvent::Admitted { .. }) {
                cancel.store(true, Ordering::Release);
            }
            Ok(())
        })
        .await;
    assert_eq!(result.unwrap_err(), SchedulerError::Cancelled);
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "a"), 0);
    assert_eq!(
        snap(&s.resilience, "a").circuit_state,
        CircuitState::HalfOpen
    );
    clean(&s);
}
#[tokio::test]
async fn resilience_real_groq_http_errors_open_only_after_factual_boundary() {
    use crate::security::secrets::{SecretKey, SecretStore};
    use std::io::{Read, Write};
    let dir = Directory::new();
    let store = Arc::new(SecretStore::with_key_store(
        dir.0.join("secrets"),
        Arc::new(Keys::default()),
    ));
    store
        .set_secret(SecretKey::GroqApiKey, b"synthetic-http-credential-marker")
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let sent = Arc::new(AtomicU64::new(0));
    let count = sent.clone();
    let server = std::thread::spawn(move || {
        for (status, hint) in [(408, ""), (503, "Retry-After: 1\r\n"), (503, "")] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = vec![];
            let mut buffer = [0u8; 4096];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]);
                    let len = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            count.fetch_add(1, Ordering::SeqCst);
            write!(socket,"HTTP/1.1 {status} Synthetic\r\n{hint}Content-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        }
    });
    let provider = Arc::new(
        super::groq::GroqProvider::new(
            super::groq::GroqConfig {
                endpoint,
                ..Default::default()
            },
            store,
        )
        .unwrap(),
    );
    let mut registry = ProviderRegistry::default();
    registry
        .register(
            ProviderConfig {
                id: "groq".into(),
                priority: 1,
                enabled: true,
                capabilities: ProviderCapabilities::text_stream(),
            },
            provider,
        )
        .unwrap();
    let c = Arc::new(Clock::default());
    let j = Jitter::new(false);
    let s = Scheduler::with_resilience_config(
        registry,
        AdmissionConfig::default(),
        c.clone(),
        None,
        config(),
        j.clone(),
    )
    .unwrap();
    let req = || {
        let mut r = request(&["groq"], ProviderSelection::Fixed("groq".into()));
        r.targets[0].invocation.model = super::groq::MODEL.into();
        r
    };
    for (n, error) in [
        (1, ProviderError::Timeout),
        (
            2,
            ProviderError::Unavailable {
                retry_after_ms: Some(1000),
            },
        ),
        (
            3,
            ProviderError::Unavailable {
                retry_after_ms: None,
            },
        ),
    ] {
        assert_eq!(
            s.run_with_retry(
                req(),
                budget(),
                no_retry(),
                &AtomicBool::new(false),
                &mut |_| Ok(())
            )
            .await
            .unwrap_err(),
            SchedulerError::Provider(error)
        );
        assert_eq!(factual(&s, "groq"), n);
        assert_eq!(snap(&s.resilience, "groq").consecutive_eligible_failures, n);
        if n == 2 {
            assert_eq!(snap(&s.resilience, "groq").cooldown_remaining_ms, 1000);
            c.advance(1000);
        }
    }
    assert_eq!(
        snap(&s.resilience, "groq").circuit_state,
        CircuitState::Open
    );
    assert_eq!(
        s.run(req(), budget(), &AtomicBool::new(false), &mut |_| panic!(
            "blocked call"
        ))
        .await
        .unwrap_err(),
        SchedulerError::NoProvider
    );
    assert_eq!(sent.load(Ordering::SeqCst), 3);
    assert_eq!(j.calls.load(Ordering::SeqCst), 0);
    server.join().unwrap();
    clean(&s);
}
#[tokio::test]
async fn resilience_preferred_gate_closed_while_queued_advances_in_authorized_order() {
    let (s, _, _, mut calls) = harness(
        config(),
        AdmissionConfig {
            max_concurrency_per_provider: 1,
            ..AdmissionConfig::default()
        },
        None,
    );
    let hold = s
        .admission
        .acquire(
            "a",
            TrafficClass::ForegroundTask,
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .await
        .unwrap();
    let (run, mut events) = start(
        s.clone(),
        request(&["a", "b", "c"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
    open(&s.resilience, "a");
    drop(hold);
    let call = bounded(calls.recv()).await.unwrap();
    assert_eq!(call.id, "b");
    call.finish.send(Completion::success()).unwrap();
    assert_eq!(bounded(run).await.unwrap().unwrap().provider_id, "b");
    assert_eq!(factual(&s, "a"), 0);
    assert_eq!(factual(&s, "b"), 1);
    assert_eq!(factual(&s, "c"), 0);
    clean(&s);
}
#[tokio::test]
async fn resilience_summary_background_operational_gate_returns_transient_contract() {
    let (s, _, _, mut calls) = default_harness();
    open(&s.resilience, "a");
    let mut r = fixed();
    r.traffic_class = TrafficClass::Background;
    assert_eq!(
        s.run(r, budget(), &AtomicBool::new(false), &mut |_| panic!(
            "summary gate must defer locally"
        ))
        .await
        .unwrap_err(),
        SchedulerError::NoProvider
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(factual(&s, "a"), 0);
    clean(&s);
}
#[test]
fn resilience_running_closed_failure_after_recovery_counts_in_current_closed() {
    let (m, c, _) = manager(config(), true);
    let old = m.authorize("a", 0).unwrap();
    open(&m, "a");
    c.advance(30);
    outcome(&m, "a", None);
    old.finish(true, Some(&ProviderError::Timeout));
    assert_eq!(snap(&m, "a").circuit_state, CircuitState::Closed);
    assert_eq!(snap(&m, "a").consecutive_eligible_failures, 1);
}
#[test]
fn resilience_running_closed_success_after_recovery_resets_current_closed() {
    let (m, c, _) = manager(config(), true);
    let old = m.authorize("a", 0).unwrap();
    open(&m, "a");
    c.advance(30);
    outcome(&m, "a", None);
    outcome(&m, "a", Some(ProviderError::Timeout));
    old.finish(true, None);
    assert_eq!(snap(&m, "a").consecutive_eligible_failures, 0);
}
