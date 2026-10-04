use super::{
    admission::{AdmissionConfig, TrafficClass},
    provider::{Provider, ProviderFuture},
    rate::*,
    registry::ProviderRegistry,
    scheduler::{Scheduler, SchedulerEvent},
    telemetry::*,
    types::*,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier, Mutex,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

struct FakeClock(Mutex<ClockReading>);
impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(ClockReading {
            monotonic_ms: 0,
            unix_ms: Some(1_000),
        })))
    }
    fn advance(&self, ms: u64) {
        let mut n = self.0.lock().unwrap();
        n.monotonic_ms += ms;
        n.unix_ms = n.unix_ms.map(|at| at + ms);
    }
    fn wall(&self, at: Option<u64>) {
        self.0.lock().unwrap().unix_ms = at;
    }
}
impl RateClock for FakeClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().unwrap()
    }
}

#[test]
fn cancellation_during_local_http_commit_rolls_back_without_factual_request() {
    struct CancellingClock {
        clock: Arc<FakeClock>,
        armed: AtomicBool,
        cancelled: Arc<AtomicBool>,
    }
    impl RateClock for CancellingClock {
        fn now(&self) -> ClockReading {
            if self.armed.swap(false, Ordering::AcqRel) {
                self.cancelled.store(true, Ordering::Release);
            }
            self.clock.now()
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(CancellingClock {
        clock: FakeClock::new(),
        armed: AtomicBool::new(false),
        cancelled: cancelled.clone(),
    });
    let rate = RateLimitManager::new(["a".into()], clock.clone(), None).unwrap();
    rate.set_policy("a", daily(Some(1), Some(80))).unwrap();
    let telemetry = TelemetryStore::with_rate(["a".into()], rate.clone());
    let observation = telemetry.attempt("a");
    let reservation = rate
        .reserve(
            "a",
            "m",
            observation.context_generation(),
            Some(TokenUpperBound::explicit_total(80).unwrap()),
            &cancelled,
        )
        .unwrap();
    observation.attach_rate(reservation.handle());
    // Cancellation wins inside local revalidation, after its initial check.
    // No executor timing or sleep is involved.
    clock.armed.store(true, Ordering::Release);
    assert!(!observation.started_unless_cancelled(&cancelled));
    assert_eq!(observation.rate_error(), Some(SchedulerError::Cancelled));
    drop(reservation);
    let snapshot = &rate.snapshots()[0];
    assert_eq!(snapshot.pending_reservations, 0);
    assert!(snapshot
        .constraints
        .iter()
        .all(|b| b.consumed == 0 && b.reserved == 0));
    assert!(matches!(
        telemetry.snapshots()[0].usage[&UsageDimension::Requests].observed,
        Fact::Known { value: 0, .. }
    ));
    cancelled.store(false, Ordering::Release);
    assert!(rate
        .reserve(
            "a",
            "m",
            0,
            Some(TokenUpperBound::explicit_total(80).unwrap()),
            &cancelled
        )
        .is_ok());
}
struct Harness {
    rate: Arc<RateLimitManager>,
    telemetry: TelemetryStore,
    clock: Arc<FakeClock>,
}
impl Harness {
    fn new() -> Self {
        let clock = FakeClock::new();
        let rate = RateLimitManager::new(["a".into(), "b".into()], clock.clone(), None).unwrap();
        let telemetry = TelemetryStore::with_rate(["a".into(), "b".into()], rate.clone());
        Self {
            rate,
            telemetry,
            clock,
        }
    }
    fn quota(
        &self,
        id: &str,
        scope: QuotaScope,
        dim: QuotaDimension,
        limit: Option<u64>,
        remaining: Option<u64>,
        reset: Option<Timing>,
    ) {
        self.telemetry.observe_quota(
            id,
            scope,
            dim,
            limit,
            remaining,
            reset,
            Provenance::ProviderHeader,
        );
    }
    fn reserve<'a>(
        &'a self,
        id: &'a str,
        model: &str,
        bound: Option<u64>,
    ) -> Result<(RateReservation, InvocationObservation<'a>), SchedulerError> {
        let obs = self.telemetry.attempt(id);
        let guard = self.rate.reserve(
            id,
            model,
            obs.context_generation(),
            bound.map(|n| TokenUpperBound::explicit_total(n).unwrap()),
            &AtomicBool::new(false),
        )?;
        obs.attach_rate(guard.handle());
        Ok((guard, obs))
    }
    fn bucket(&self, id: &str, dim: QuotaDimension) -> RateConstraintSnapshot {
        self.rate
            .snapshots()
            .into_iter()
            .find(|s| s.provider_id == id)
            .unwrap()
            .constraints
            .into_iter()
            .find(|c| c.dimension == dim)
            .unwrap()
    }
}
fn model(m: &str) -> QuotaScope {
    QuotaScope::Model { model: m.into() }
}
fn err<T>(r: Result<T, SchedulerError>) -> SchedulerError {
    r.err().expect("expected local denial")
}
fn local(dim: QuotaDimension, capacity: u64, period: u64) -> RatePolicy {
    RatePolicy {
        limits: vec![LocalRateLimit {
            scope: QuotaScope::Provider,
            dimension: dim,
            capacity,
            window: FixedWindow {
                period_ms: period,
                anchor_unix_ms: 0,
            },
        }],
        daily_budget: None,
    }
}
fn daily(requests: Option<u64>, tokens: Option<u64>) -> RatePolicy {
    RatePolicy {
        limits: vec![],
        daily_budget: Some(DailyBudgetPolicy {
            anchor_unix_ms: 0,
            max_requests: requests,
            max_accounted_tokens: tokens,
        }),
    }
}

#[test]
fn unknown_is_neither_unlimited_nor_zero_and_limit_only_never_guesses_reset() {
    let h = Harness::new();
    for _ in 0..4 {
        let (r, o) = h.reserve("a", "m", None).unwrap();
        assert!(o.started());
        drop(r);
    }
    assert!(h.rate.snapshots()[0].constraints.is_empty());
    for dim in [
        QuotaDimension::RequestsPerMinute,
        QuotaDimension::RequestsPerDay,
    ] {
        let h = Harness::new();
        h.quota("a", model("m"), dim, Some(2), None, None);
        for _ in 0..2 {
            let (r, o) = h.reserve("a", "m", None).unwrap();
            assert!(o.started());
            drop(r);
        }
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        );
        h.clock.advance(5 * 86_400_000);
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        );
        assert!(h.bucket("a", dim).reset_in_ms.is_none());
    }
}
#[test]
fn all_four_dimensions_reserve_known_capacity_and_zero_blocks() {
    for dim in [
        QuotaDimension::RequestsPerMinute,
        QuotaDimension::RequestsPerDay,
        QuotaDimension::TokensPerMinute,
        QuotaDimension::TokensPerDay,
    ] {
        let h = Harness::new();
        h.quota("a", QuotaScope::Provider, dim, Some(2), Some(1), None);
        let (r, o) = h.reserve("a", "m", Some(1)).unwrap();
        assert_eq!(h.bucket("a", dim).reserved, 1);
        assert_eq!(h.bucket("a", dim).consumed, 0);
        assert_eq!(
            err(h.reserve("a", "m", Some(1))),
            SchedulerError::RateCapacityExceeded
        );
        assert!(o.started());
        drop(r);
        assert_eq!(h.bucket("a", dim).effective_remaining, Some(0));
        assert_eq!(
            err(h.reserve("a", "m", Some(1))),
            SchedulerError::RateCapacityExceeded
        );
    }
}
#[test]
fn provider_and_model_constraints_combine_without_model_promotion() {
    let h = Harness::new();
    let dim = QuotaDimension::RequestsPerDay;
    h.quota("a", QuotaScope::Provider, dim, Some(3), Some(3), None);
    h.quota("a", model("A"), dim, Some(1), Some(1), None);
    let (a, oa) = h.reserve("a", "A", None).unwrap();
    assert!(oa.started());
    drop(a);
    assert_eq!(
        err(h.reserve("a", "A", None)),
        SchedulerError::RateCapacityExceeded
    );
    let (b, ob) = h.reserve("a", "B", None).unwrap();
    assert!(ob.started());
    drop(b);
    let (c, oc) = h.reserve("a", "B", None).unwrap();
    assert!(oc.started());
    drop(c);
    assert_eq!(
        err(h.reserve("a", "B", None)),
        SchedulerError::RateCapacityExceeded
    );
    assert!(h.reserve("b", "A", None).is_ok());
}
#[test]
fn simultaneous_reservations_are_atomic_at_remaining_one() {
    for (dim, amount) in [
        (QuotaDimension::RequestsPerMinute, None),
        (QuotaDimension::TokensPerMinute, Some(800)),
    ] {
        let h = Arc::new(Harness::new());
        h.quota(
            "a",
            model("m"),
            dim,
            Some(1_000),
            Some(if amount.is_some() { 1_000 } else { 1 }),
            None,
        );
        let barrier = Arc::new(Barrier::new(3));
        let held = Arc::new(Barrier::new(3));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let h = h.clone();
                let barrier = barrier.clone();
                let held = held.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let r = h.reserve("a", "m", amount);
                    let success = r.is_ok();
                    held.wait();
                    drop(r);
                    success
                })
            })
            .collect();
        barrier.wait();
        held.wait();
        assert_eq!(
            handles
                .into_iter()
                .map(|t| usize::from(t.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert_eq!(h.bucket("a", dim).reserved, 0);
    }
}
#[test]
fn cancelled_pure_reservation_and_drop_return_all_credit() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(1),
        Some(1),
        None,
    );
    assert_eq!(
        err(h.rate.reserve("a", "m", 0, None, &AtomicBool::new(true))),
        SchedulerError::Cancelled
    );
    let (r, _) = h.reserve("a", "m", None).unwrap();
    drop(r);
    assert_eq!(
        h.bucket("a", QuotaDimension::RequestsPerDay)
            .effective_remaining,
        Some(1)
    );
    assert!(h.reserve("a", "m", None).is_ok());
}
#[test]
fn token_reconciliation_less_equal_unknown_greater_and_absent_bound() {
    for (actual, expected) in [(Some(30), 30), (Some(80), 80), (None, 80), (Some(120), 120)] {
        let h = Harness::new();
        h.quota(
            "a",
            model("m"),
            QuotaDimension::TokensPerMinute,
            Some(1_000),
            Some(1_000),
            None,
        );
        let (r, o) = h.reserve("a", "m", Some(80)).unwrap();
        assert!(o.started());
        if let Some(actual) = actual {
            o.final_usage(ProviderUsage {
                total_tokens: Some(actual as u32),
                output_tokens_measured: true,
                ..ProviderUsage::default()
            });
        }
        drop(r);
        let b = h.bucket("a", QuotaDimension::TokensPerMinute);
        assert_eq!(b.consumed, expected);
        assert_eq!(b.effective_remaining, Some(1_000 - expected));
        assert_eq!(b.reserved, 0);
    }
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerDay,
        Some(0),
        Some(0),
        None,
    );
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        None
    );
    drop(r);
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .unaccounted_token_calls,
        1
    );
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    o.observed_usage([(UsageDimension::TotalTokens, Some(12))]);
    drop(r);
    assert_eq!(h.bucket("a", QuotaDimension::TokensPerDay).consumed, 12);
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        None
    );
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerDay,
        Some(100),
        Some(50),
        None,
    );
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        Some(50)
    );
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        None
    );
    o.final_usage(ProviderUsage {
        total_tokens: Some(20),
        output_tokens_measured: true,
        ..ProviderUsage::default()
    });
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        Some(30)
    );
    drop(r);
}
#[test]
fn token_overflow_saturates_and_never_refunds_uncertain_saturated_debits() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(MAX_FACT_VALUE),
        Some(MAX_FACT_VALUE),
        None,
    );
    let (r, o) = h.reserve("a", "m", Some(MAX_FACT_VALUE - 1)).unwrap();
    assert!(o.started());
    let (other, oo) = h.reserve("a", "m", None).unwrap();
    assert!(oo.started());
    oo.observed_usage([(UsageDimension::TotalTokens, Some(MAX_FACT_VALUE))]);
    drop(other);
    o.final_usage(ProviderUsage {
        total_tokens: Some(1),
        output_tokens_measured: true,
        ..ProviderUsage::default()
    });
    drop(r);
    let b = h.bucket("a", QuotaDimension::TokensPerMinute);
    assert!(b.saturated);
    assert_eq!(b.consumed, MAX_FACT_VALUE);
    assert_eq!(b.effective_remaining, None);
    assert_eq!(b.unaccounted_token_calls, 1);
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateStateUnavailable
    );
    assert!(TokenUpperBound::explicit_total(u64::MAX).is_err());
}
#[test]
fn factual_resets_delay_absolute_past_and_exact_boundaries_use_monotonic_time() {
    for reset in [Timing::DelayMs(10), Timing::UnixMs(1_010)] {
        let h = Harness::new();
        h.quota(
            "a",
            model("m"),
            QuotaDimension::RequestsPerDay,
            Some(1),
            Some(0),
            Some(reset),
        );
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        );
        h.clock.advance(9);
        h.clock.wall(Some(999_999));
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        );
        h.clock.advance(1);
        assert!(h.reserve("a", "m", None).is_ok());
        h.clock.advance(1);
        assert!(h.reserve("a", "m", None).is_ok());
        let (r, o) = h.reserve("a", "m", None).unwrap();
        assert!(o.started());
        drop(r);
        h.clock.advance(1_000);
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        ); // one-shot, no invented period
    }
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(1),
        Some(0),
        Some(Timing::UnixMs(900)),
    );
    assert!(h.reserve("a", "m", None).is_ok());
}
#[test]
fn invalid_timestamp_overflow_and_missing_wall_clock_never_synthesize_refill() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(1),
        Some(0),
        None,
    );
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(1),
        Some(0),
        Some(Timing::UnixMs(u64::MAX)),
    );
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateCapacityExceeded
    );
    *h.clock.0.lock().unwrap() = ClockReading {
        monotonic_ms: u64::MAX - 2,
        unix_ms: None,
    };
    for reset in [Timing::DelayMs(10), Timing::UnixMs(1_010)] {
        h.quota(
            "a",
            model("m"),
            QuotaDimension::RequestsPerDay,
            Some(1),
            Some(0),
            Some(reset),
        );
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        );
        assert!(h
            .bucket("a", QuotaDimension::RequestsPerDay)
            .reset_in_ms
            .is_none());
    }
}
#[test]
fn missing_or_partial_facts_cannot_refill_exhausted_unknown_reset() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerMinute,
        Some(1),
        Some(0),
        None,
    );
    for limit in [None, Some(1)] {
        h.quota(
            "a",
            model("m"),
            QuotaDimension::RequestsPerMinute,
            limit,
            None,
            None,
        );
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateCapacityExceeded
        );
    }
    // Explicit fresh remaining is evidence, unlike a missing header.
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerMinute,
        Some(1),
        Some(1),
        None,
    );
    assert!(h.reserve("a", "m", None).is_ok());
}
#[test]
fn local_fixed_window_is_explicit_and_pending_reservations_cross_boundaries_safely() {
    let h = Harness::new();
    h.rate
        .set_policy("a", local(QuotaDimension::RequestsPerDay, 1, 10))
        .unwrap();
    let (r, o) = h.reserve("a", "m", None).unwrap();
    h.clock.advance(10);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateCapacityExceeded
    ); // pending re-based
    assert!(o.started());
    drop(r);
    h.clock.advance(9);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateCapacityExceeded
    );
    h.clock.advance(1);
    assert!(h.reserve("a", "m", None).is_ok());
}
#[test]
fn old_window_usage_does_not_refund_or_debit_new_window() {
    let h = Harness::new();
    h.rate
        .set_policy("a", local(QuotaDimension::TokensPerDay, 100, 10))
        .unwrap();
    let (r, o) = h.reserve("a", "m", Some(100)).unwrap();
    assert!(o.started());
    h.clock.advance(10);
    let (other, oo) = h.reserve("a", "m", Some(100)).unwrap();
    assert!(oo.started());
    o.observed_usage([(UsageDimension::TotalTokens, Some(1))]);
    drop(r);
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        Some(0)
    );
    drop(other);
}
#[test]
fn invalid_policies_are_rejected_and_policy_change_does_not_zero_same_window() {
    let h = Harness::new();
    for policy in [
        local(QuotaDimension::Concurrency, 1, 10),
        local(QuotaDimension::RequestsPerMinute, 1, 0),
        local(QuotaDimension::TokensPerDay, u64::MAX, 10),
    ] {
        assert_eq!(
            err(h.rate.set_policy("a", policy)),
            SchedulerError::InvalidRatePolicy
        );
    }
    h.rate.set_policy("a", daily(Some(1), None)).unwrap();
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert_eq!(
        err(h.rate.set_policy("a", daily(Some(2), None))),
        SchedulerError::RatePolicyBusy
    );
    assert!(o.started());
    drop(r);
    h.rate.set_policy("a", daily(Some(1), None)).unwrap();
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::DailyBudgetExceeded
    );
    assert!(h.telemetry.snapshots()[0].quotas[0]
        .dimensions
        .values()
        .all(|q| matches!(q.limit, Fact::Unknown)));
}
#[test]
fn context_rotation_drops_old_quota_rejects_late_headers_and_preserves_history_and_peers() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(1),
        Some(1),
        None,
    );
    h.quota(
        "b",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(0),
        Some(0),
        None,
    );
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    o.observed_usage([(UsageDimension::TotalTokens, Some(5))]);
    h.telemetry.invalidate_provider_quotas("a");
    o.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(0),
        Some(0),
        None,
    );
    o.finished(Some(&ProviderError::Timeout));
    drop(r);
    assert!(h.reserve("a", "m", None).is_ok());
    assert_eq!(
        err(h.reserve("b", "m", None)),
        SchedulerError::RateCapacityExceeded
    );
    let snapshot = &h.telemetry.snapshots()[0];
    assert_eq!(snapshot.context_generation, 1);
    assert!(snapshot.quotas.iter().all(|s| s
        .dimensions
        .values()
        .all(|q| matches!(q.limit, Fact::Unknown))));
    assert!(matches!(
        snapshot.usage[&UsageDimension::Requests].observed,
        Fact::Known { value: 1, .. }
    ));
    assert!(matches!(
        snapshot.usage[&UsageDimension::TotalTokens].observed,
        Fact::Known { value: 5, .. }
    ));
    assert!(matches!(
        snapshot.last_outcome,
        Fact::Known {
            value: Outcome::Failed { code: "timeout" },
            ..
        }
    ));
}
#[test]
fn rotation_before_http_wins_and_local_daily_budget_survives_context_change() {
    let h = Harness::new();
    h.rate.set_policy("a", daily(Some(1), None)).unwrap();
    let (r, o) = h.reserve("a", "m", None).unwrap();
    h.telemetry.invalidate_provider_quotas("a");
    assert!(!o.started());
    assert_eq!(o.rate_error(), Some(SchedulerError::RateContextChanged));
    drop(r);
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    h.telemetry.invalidate_provider_quotas("a");
    drop(r);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::DailyBudgetExceeded
    );
}

struct Call {
    provider: String,
    attempt: u32,
    finish: oneshot::Sender<(Option<u32>, Option<ProviderError>)>,
}
struct Gate {
    id: String,
    bound: Option<TokenUpperBound>,
    preflight: bool,
    tx: mpsc::UnboundedSender<Call>,
}
impl Provider for Gate {
    fn token_upper_bound(&self, _: &ProviderRequest) -> Option<TokenUpperBound> {
        self.bound
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        panic!("observation required")
    }
    fn execute_observed<'a>(
        &'a self,
        r: &'a ProviderRequest,
        cancel: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        o: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            assert!(!cancel.load(Ordering::Acquire));
            if self.preflight {
                return Err(ProviderError::Authentication);
            }
            if !o.started_unless_cancelled(cancel) {
                return Err(ProviderError::Cancelled);
            }
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(Call {
                    provider: self.id.clone(),
                    attempt: r.attempt,
                    finish: tx,
                })
                .unwrap();
            let (total, error) = tokio::select! { result=rx=>result.unwrap(), _=super::transport::cancellation(cancel)=>return Err(ProviderError::Cancelled) };
            let usage = total.map_or(ProviderUsage::default(), |n| ProviderUsage {
                calls: 1,
                input_tokens: 0,
                output_tokens: n,
                total_tokens: Some(n),
                thought_tokens: None,
                output_tokens_measured: true,
            });
            if total.is_some() {
                o.final_usage(usage);
            }
            if let Some(error) = error {
                return Err(error);
            }
            Ok(ProviderResponse {
                text: "private-output-marker".into(),
                usage,
            })
        })
    }
}
fn registry(
    tx: mpsc::UnboundedSender<Call>,
    bound: Option<u64>,
    preflight: bool,
    ids: &[&str],
) -> ProviderRegistry {
    let mut reg = ProviderRegistry::default();
    for id in ids {
        reg.register(
            ProviderConfig {
                id: (*id).into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(Gate {
                id: (*id).into(),
                tx: tx.clone(),
                bound: bound.map(|n| TokenUpperBound::explicit_total(n).unwrap()),
                preflight,
            }),
        )
        .unwrap();
    }
    reg
}
fn scheduler(
    cfg: AdmissionConfig,
    bound: Option<u64>,
    preflight: bool,
) -> (Arc<Scheduler>, mpsc::UnboundedReceiver<Call>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let s = Scheduler::with_rate_config(
        registry(tx, bound, preflight, &["a", "b"]),
        cfg,
        FakeClock::new(),
        None,
    )
    .unwrap();
    (Arc::new(s), rx)
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
fn quota(s: &Scheduler, id: &str, dim: QuotaDimension, n: u64) {
    s.telemetry.observe_quota(
        id,
        model("m"),
        dim,
        Some(n),
        Some(n),
        None,
        Provenance::ProviderHeader,
    );
}
fn budget() -> TaskBudget {
    TaskBudget {
        max_provider_calls: 4,
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
async fn bounded<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), f)
        .await
        .expect("synchronization deadline")
}
type Run = tokio::task::JoinHandle<Result<TaskResult, SchedulerError>>;
fn start(
    s: Arc<Scheduler>,
    r: ProviderTaskRequest,
    c: Arc<AtomicBool>,
    retry: RetryPolicy,
) -> (Run, mpsc::UnboundedReceiver<SchedulerEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        tokio::spawn(async move {
            s.run_with_retry(r, budget(), retry, &c, &mut |e| {
                let _ = tx.send(e);
                Ok(())
            })
            .await
        }),
        rx,
    )
}
async fn event(
    rx: &mut mpsc::UnboundedReceiver<SchedulerEvent>,
    predicate: impl Fn(&SchedulerEvent) -> bool,
) -> SchedulerEvent {
    loop {
        let e = bounded(rx.recv()).await.unwrap();
        if predicate(&e) {
            return e;
        }
    }
}
fn clean(s: &Scheduler) {
    assert!(s
        .rate_snapshot()
        .iter()
        .all(|s| s.pending_reservations == 0));
    assert!(s
        .admission_snapshot()
        .iter()
        .all(|s| s.active_calls == 0 && s.queue_depth == 0));
}
fn constraint(s: &Scheduler, id: &str, dim: QuotaDimension) -> RateConstraintSnapshot {
    s.rate_snapshot()
        .into_iter()
        .find(|s| s.provider_id == id)
        .unwrap()
        .constraints
        .into_iter()
        .find(|b| b.dimension == dim)
        .unwrap()
}
async fn held(s: &Scheduler) -> super::admission::AdmissionPermit {
    s.admission
        .acquire(
            "a",
            TrafficClass::ForegroundTask,
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn scheduler_rate_block_is_local_before_admission_no_cooldown_retry_or_fallback() {
    for selection in [
        ProviderSelection::Fixed("a".into()),
        ProviderSelection::Preferred,
        ProviderSelection::Auto,
    ] {
        let (s, mut calls) = scheduler(AdmissionConfig::default(), None, false);
        quota(&s, "a", QuotaDimension::RequestsPerMinute, 0);
        let r = request(&["a", "b"], selection.clone());
        let before = s
            .ranked_provider_ids(&selection, &r.targets, &r.required_capabilities)
            .unwrap();
        let mut events = vec![];
        assert_eq!(
            s.run(r, budget(), &AtomicBool::new(false), &mut |e| {
                events.push(e);
                Ok(())
            })
            .await
            .unwrap_err(),
            SchedulerError::RateCapacityExceeded
        );
        assert!(calls.try_recv().is_err());
        assert_eq!(s.admission_snapshot()[0].total_admissions, 0);
        assert!(s.status().iter().all(|s| s.cooldown_ms == 0));
        assert!(!events.iter().any(|e| matches!(
            e,
            SchedulerEvent::Retry { .. }
                | SchedulerEvent::Fallback { .. }
                | SchedulerEvent::Queued { .. }
                | SchedulerEvent::Admitted { .. }
        )));
        let r = request(&["a", "b"], selection.clone());
        assert_eq!(
            before,
            s.ranked_provider_ids(&selection, &r.targets, &r.required_capabilities)
                .unwrap()
        );
        clean(&s);
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_scheduler_tasks_at_remaining_one_only_one_crosses_http() {
    let (s, mut calls) = scheduler(AdmissionConfig::default(), None, false);
    quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let (tx, mut results) = mpsc::unbounded_channel();
    let mut handles = vec![];
    for _ in 0..2 {
        let s = s.clone();
        let b = barrier.clone();
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            let result = s
                .run(
                    request(&["a"], ProviderSelection::Fixed("a".into())),
                    budget(),
                    &AtomicBool::new(false),
                    &mut |_| Ok(()),
                )
                .await;
            tx.send(result).unwrap();
        }));
    }
    barrier.wait().await;
    let call = bounded(calls.recv()).await.unwrap();
    let blocked = bounded(results.recv()).await.unwrap();
    assert_eq!(blocked.unwrap_err(), SchedulerError::RateCapacityExceeded);
    assert!(calls.try_recv().is_err());
    call.finish.send((None, None)).unwrap();
    assert!(bounded(results.recv()).await.unwrap().is_ok());
    for h in handles {
        h.await.unwrap();
    }
    clean(&s);
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).consumed,
        1
    );
}
#[tokio::test]
async fn admission_full_and_timeout_return_rate_reservation() {
    for (capacity, expected) in [
        (0, SchedulerError::AdmissionQueueFull),
        (1, SchedulerError::AdmissionTimeout),
    ] {
        let (s, mut calls) = scheduler(
            AdmissionConfig {
                max_concurrency_per_provider: 1,
                queue_capacity_per_provider: capacity,
                queue_timeout_ms: 1,
                ..AdmissionConfig::default()
            },
            Some(80),
            false,
        );
        quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
        quota(&s, "a", QuotaDimension::TokensPerMinute, 80);
        let permit = held(&s).await;
        assert_eq!(
            s.run(
                request(&["a", "b"], ProviderSelection::Preferred),
                budget(),
                &AtomicBool::new(false),
                &mut |_| Ok(())
            )
            .await
            .unwrap_err(),
            expected
        );
        assert_eq!(
            constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
            Some(1)
        );
        assert_eq!(
            constraint(&s, "a", QuotaDimension::TokensPerMinute).effective_remaining,
            Some(80)
        );
        assert!(calls.try_recv().is_err());
        drop(permit);
        clean(&s);
    }
}
#[tokio::test]
async fn cancellation_and_abort_while_queued_roll_back_without_provider() {
    for abort in [false, true] {
        let (s, mut calls) = scheduler(
            AdmissionConfig {
                max_concurrency_per_provider: 1,
                ..AdmissionConfig::default()
            },
            None,
            false,
        );
        quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
        let permit = held(&s).await;
        let cancel = Arc::new(AtomicBool::new(false));
        let (run, mut events) = start(
            s.clone(),
            request(&["a"], ProviderSelection::Fixed("a".into())),
            cancel.clone(),
            no_retry(),
        );
        event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
        assert_eq!(
            constraint(&s, "a", QuotaDimension::RequestsPerDay).reserved,
            1
        );
        if abort {
            run.abort();
            assert!(bounded(run).await.unwrap_err().is_cancelled());
        } else {
            cancel.store(true, Ordering::Release);
            drop(permit);
            assert_eq!(
                bounded(run).await.unwrap().unwrap_err(),
                SchedulerError::Cancelled
            );
            assert!(calls.try_recv().is_err());
            clean(&s);
            continue;
        }
        assert_eq!(
            constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
            Some(1)
        );
        assert!(calls.try_recv().is_err());
        drop(permit);
        clean(&s);
    }
}
#[tokio::test]
async fn sink_closed_queued_or_admitted_rolls_back_and_cancellation_admitted_wins() {
    for queued in [false, true] {
        let (s, mut calls) = scheduler(
            AdmissionConfig {
                max_concurrency_per_provider: 1,
                ..AdmissionConfig::default()
            },
            None,
            false,
        );
        quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
        let permit = if queued { Some(held(&s).await) } else { None };
        assert_eq!(
            s.run(
                request(&["a"], ProviderSelection::Fixed("a".into())),
                budget(),
                &AtomicBool::new(false),
                &mut |e| if matches!(
                    e,
                    SchedulerEvent::Queued { .. } | SchedulerEvent::Admitted { .. }
                ) {
                    Err(SchedulerError::EventSinkClosed)
                } else {
                    Ok(())
                }
            )
            .await
            .unwrap_err(),
            SchedulerError::EventSinkClosed
        );
        assert_eq!(
            constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
            Some(1)
        );
        assert!(calls.try_recv().is_err());
        drop(permit);
        clean(&s);
    }
    let (s, mut calls) = scheduler(AdmissionConfig::default(), None, false);
    quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
    let c = AtomicBool::new(false);
    assert_eq!(
        s.run(
            request(&["a"], ProviderSelection::Fixed("a".into())),
            budget(),
            &c,
            &mut |e| {
                if matches!(e, SchedulerEvent::Admitted { .. }) {
                    c.store(true, Ordering::Release);
                }
                Ok(())
            }
        )
        .await
        .unwrap_err(),
        SchedulerError::Cancelled
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
        Some(1)
    );
    assert!(calls.try_recv().is_err());
    clean(&s);
}
#[tokio::test]
async fn adapter_preflight_without_http_releases_permit_and_rate() {
    let (s, mut calls) = scheduler(AdmissionConfig::default(), Some(80), true);
    quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
    quota(&s, "a", QuotaDimension::TokensPerMinute, 80);
    assert_eq!(
        s.run(
            request(&["a"], ProviderSelection::Fixed("a".into())),
            budget(),
            &AtomicBool::new(false),
            &mut |_| Ok(())
        )
        .await
        .unwrap_err(),
        SchedulerError::Provider(ProviderError::Authentication)
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).consumed,
        0
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).reserved,
        0
    );
    clean(&s);
}
#[tokio::test]
async fn http_success_error_cancel_and_abort_keep_consumed_request() {
    for terminal in 0..4 {
        let (s, mut calls) = scheduler(AdmissionConfig::default(), Some(80), false);
        quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
        quota(&s, "a", QuotaDimension::TokensPerMinute, 80);
        let c = Arc::new(AtomicBool::new(false));
        let (run, _events) = start(
            s.clone(),
            request(&["a"], ProviderSelection::Fixed("a".into())),
            c.clone(),
            no_retry(),
        );
        let call = bounded(calls.recv()).await.unwrap();
        match terminal {
            0 => {
                call.finish.send((Some(30), None)).unwrap();
                assert!(bounded(run).await.unwrap().is_ok());
            }
            1 => {
                call.finish
                    .send((Some(30), Some(ProviderError::Fatal)))
                    .unwrap();
                assert_eq!(
                    bounded(run).await.unwrap().unwrap_err(),
                    SchedulerError::Provider(ProviderError::Fatal)
                );
            }
            2 => {
                c.store(true, Ordering::Release);
                assert_eq!(
                    bounded(run).await.unwrap().unwrap_err(),
                    SchedulerError::Cancelled
                );
            }
            _ => {
                run.abort();
                assert!(bounded(run).await.unwrap_err().is_cancelled());
            }
        }
        let b = constraint(&s, "a", QuotaDimension::TokensPerMinute);
        assert_eq!(b.consumed, if terminal < 2 { 30 } else { 80 });
        assert_eq!(
            constraint(&s, "a", QuotaDimension::RequestsPerDay).consumed,
            1
        );
        clean(&s);
    }
}
#[tokio::test]
async fn retries_reserve_each_attempt_and_backoff_holds_no_future_reservation() {
    let (s, mut calls) = scheduler(AdmissionConfig::default(), None, false);
    quota(&s, "a", QuotaDimension::RequestsPerDay, 2);
    let (run, _) = start(
        s.clone(),
        request(&["a"], ProviderSelection::Fixed("a".into())),
        Arc::new(AtomicBool::new(false)),
        RetryPolicy {
            enabled: true,
            max_retries: 1,
            initial_backoff_ms: 0,
        },
    );
    let one = bounded(calls.recv()).await.unwrap();
    assert_eq!(one.attempt, 1);
    one.finish
        .send((None, Some(ProviderError::Timeout)))
        .unwrap();
    let two = bounded(calls.recv()).await.unwrap();
    assert_eq!(two.attempt, 2);
    two.finish.send((None, None)).unwrap();
    assert_eq!(bounded(run).await.unwrap().unwrap().usage.retries, 1);
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).consumed,
        2
    );
    clean(&s);
    let (s, mut calls) = scheduler(AdmissionConfig::default(), None, false);
    quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
    let cancel = Arc::new(AtomicBool::new(false));
    let (run, mut events) = start(
        s.clone(),
        request(&["a"], ProviderSelection::Fixed("a".into())),
        cancel.clone(),
        RetryPolicy {
            enabled: true,
            max_retries: 1,
            initial_backoff_ms: 10_000,
        },
    );
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send((None, Some(ProviderError::Timeout)))
        .unwrap();
    event(&mut events, |e| matches!(e, SchedulerEvent::Retry { .. })).await;
    clean(&s);
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).reserved,
        0
    );
    cancel.store(true, Ordering::Release);
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::Cancelled
    );
}
#[tokio::test]
async fn retry_hits_rate_gate_and_remote_fallback_has_independent_ledger() {
    let (s, mut calls) = scheduler(AdmissionConfig::default(), None, false);
    quota(&s, "a", QuotaDimension::RequestsPerDay, 1);
    let (run, _) = start(
        s.clone(),
        request(&["a", "b"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        RetryPolicy {
            enabled: true,
            max_retries: 1,
            initial_backoff_ms: 0,
        },
    );
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send((None, Some(ProviderError::Timeout)))
        .unwrap();
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::RateCapacityExceeded
    );
    assert!(calls.try_recv().is_err());
    assert!(s.status().iter().all(|s| s.cooldown_ms == 0));
    clean(&s);
    let (s, mut calls) = scheduler(AdmissionConfig::default(), Some(80), false);
    for id in ["a", "b"] {
        quota(&s, id, QuotaDimension::RequestsPerDay, 1);
        quota(&s, id, QuotaDimension::TokensPerMinute, 80);
    }
    let (run, _) = start(
        s.clone(),
        request(&["a", "b"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let a = bounded(calls.recv()).await.unwrap();
    assert_eq!(a.provider, "a");
    a.finish
        .send((
            None,
            Some(ProviderError::Unavailable {
                retry_after_ms: None,
            }),
        ))
        .unwrap();
    let b = bounded(calls.recv()).await.unwrap();
    assert_eq!(b.provider, "b");
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).consumed,
        80
    );
    b.finish.send((Some(20), None)).unwrap();
    assert_eq!(bounded(run).await.unwrap().unwrap().provider_id, "b");
    assert_eq!(
        constraint(&s, "b", QuotaDimension::TokensPerMinute).consumed,
        20
    );
    clean(&s);
}
#[tokio::test]
async fn task_budget_is_distinct_from_daily_budget_and_snapshot_excludes_content() {
    let (s, mut calls) = scheduler(AdmissionConfig::default(), Some(80), false);
    s.rate.set_policy("a", daily(Some(1), Some(80))).unwrap();
    assert_eq!(
        s.run(
            request(&["a"], ProviderSelection::Fixed("a".into())),
            TaskBudget {
                max_provider_calls: 0,
                max_output_tokens: None
            },
            &AtomicBool::new(false),
            &mut |_| Ok(())
        )
        .await
        .unwrap_err(),
        SchedulerError::BudgetExceeded
    );
    let (run, _) = start(
        s.clone(),
        request(&["a"], ProviderSelection::Fixed("a".into())),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send((None, None))
        .unwrap();
    bounded(run).await.unwrap().unwrap();
    assert_eq!(
        s.run(
            request(&["a"], ProviderSelection::Fixed("a".into())),
            budget(),
            &AtomicBool::new(false),
            &mut |_| Ok(())
        )
        .await
        .unwrap_err(),
        SchedulerError::DailyBudgetExceeded
    );
    let json = serde_json::to_string(&(s.rate_snapshot(), s.telemetry_snapshot())).unwrap();
    for marker in [
        "private-prompt-marker",
        "private-output-marker",
        "private-reasoning-marker",
        "api_key",
        "account_id",
        "authorization",
        "x-ratelimit",
    ] {
        assert!(!json.contains(marker));
    }
    clean(&s);
}

struct TempDirectory(std::path::PathBuf);
impl TempDirectory {
    fn new() -> Self {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!("lr8c-{}-{n}", std::process::id())))
    }
    fn db(&self) -> crate::persistence::database::Database {
        crate::persistence::database::Database::new(self.0.clone())
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn durable(db: crate::persistence::database::Database, clock: Arc<FakeClock>) -> Harness {
    let rate = RateLimitManager::new(["a".into(), "b".into()], clock.clone(), Some(db)).unwrap();
    let telemetry = TelemetryStore::with_rate(["a".into(), "b".into()], rate.clone());
    Harness {
        rate,
        telemetry,
        clock,
    }
}
#[test]
fn restart_preserves_local_daily_requests_tokens_and_discards_remote_context() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    let mut policy = daily(Some(1), Some(80));
    policy.limits = local(QuotaDimension::RequestsPerMinute, 2, 60_000).limits;
    h.rate.set_policy("a", policy).unwrap();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(1),
        Some(1),
        None,
    );
    let (r, o) = h.reserve("a", "m", Some(80)).unwrap();
    assert!(o.started());
    o.final_usage(ProviderUsage {
        total_tokens: Some(30),
        output_tokens_measured: true,
        ..ProviderUsage::default()
    });
    drop(r);
    drop(o);
    drop(h);
    let h = durable(db.clone(), FakeClock::new());
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::DailyBudgetExceeded
    );
    let snapshots = h.rate.snapshots();
    let a = &snapshots[0];
    assert!(a
        .constraints
        .iter()
        .all(|c| c.source != ConstraintSource::ExternalFact));
    assert_eq!(
        a.constraints
            .iter()
            .find(|c| c.dimension == QuotaDimension::TokensPerDay)
            .unwrap()
            .consumed,
        30
    );
    assert_eq!(h.bucket("a", QuotaDimension::RequestsPerMinute).consumed, 1);
    assert!(h.telemetry.snapshots()[0].quotas[0]
        .dimensions
        .values()
        .all(|q| matches!(q.remaining, Fact::Unknown)));
    let stored: String = db
        .open()
        .unwrap()
        .query_row(
            "SELECT local_state FROM cognitive_rate_state WHERE provider_id='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for prohibited in [
        "remaining",
        "external",
        "context_generation",
        "api_key",
        "account_id",
        "private-prompt",
        "x-ratelimit",
    ] {
        assert!(!stored.contains(prohibited));
    }
}
#[test]
fn restart_cannot_erase_write_ahead_local_reservations_and_graceful_rollback_is_durable() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    h.rate.set_policy("a", daily(Some(1), Some(80))).unwrap();
    let (r, _) = h.reserve("a", "m", Some(80)).unwrap();
    // Loading while the guard is live simulates the disk state left by process death.
    let reopened = durable(db.clone(), FakeClock::new());
    assert_eq!(
        err(reopened.reserve("a", "m", Some(1))),
        SchedulerError::DailyBudgetExceeded
    );
    drop(reopened);
    drop(r);
    drop(h);
    let reopened = durable(db, FakeClock::new());
    assert!(reopened.reserve("a", "m", Some(80)).is_ok());
}
#[test]
fn restart_known_utc_boundary_and_backward_wall_clock_are_safe() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    h.rate.set_policy("a", daily(Some(1), None)).unwrap();
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    drop(r);
    drop(o);
    drop(h);
    let clock = FakeClock::new();
    clock.wall(Some(0));
    let h = durable(db.clone(), clock);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::DailyBudgetExceeded
    );
    drop(h);
    let clock = FakeClock::new();
    clock.wall(Some(86_400_000));
    let h = durable(db, clock);
    assert!(h.reserve("a", "m", None).is_ok());
}
#[test]
fn durable_failure_denies_before_http_and_failed_refund_never_creates_disk_credit() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    h.rate.set_policy("a", daily(Some(2), Some(80))).unwrap();
    let (r, o) = h.reserve("a", "m", Some(80)).unwrap();
    db.open().unwrap().execute_batch("CREATE TRIGGER reject_rate BEFORE UPDATE ON cognitive_rate_state BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(!o.started());
    assert_eq!(o.rate_error(), Some(SchedulerError::RateStateUnavailable));
    drop(r);
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateStateUnavailable
    );
    assert!(matches!(
        h.telemetry.snapshots()[0].usage[&UsageDimension::Requests].observed,
        Fact::Known { value: 0, .. }
    ));
    db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_rate")
        .unwrap();
    drop(o);
    drop(h);
    let h = durable(db.clone(), FakeClock::new());
    assert_eq!(h.bucket("a", QuotaDimension::TokensPerDay).consumed, 80);
    // Known usage exceeding the bound is conservatively recorded even on overflow.
    h.rate.set_policy("a", daily(Some(2), Some(160))).unwrap();
    let (r, o) = h.reserve("a", "m", Some(80)).unwrap();
    assert!(o.started());
    db.open().unwrap().execute_batch("CREATE TRIGGER reject_rate BEFORE UPDATE ON cognitive_rate_state BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    o.final_usage(ProviderUsage {
        total_tokens: Some(30),
        output_tokens_measured: true,
        ..ProviderUsage::default()
    });
    drop(r);
    assert!(h.rate.snapshots()[0].persistence_failed);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateStateUnavailable
    );
    db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_rate")
        .unwrap();
    drop(o);
    drop(h);
    let h = durable(db, FakeClock::new());
    assert_eq!(h.bucket("a", QuotaDimension::TokensPerDay).consumed, 160);
}
#[test]
fn malformed_persisted_local_state_is_a_typed_startup_failure() {
    let dir = TempDirectory::new();
    let db = dir.db();
    db.open()
        .unwrap()
        .execute(
            "INSERT INTO cognitive_rate_state(provider_id,local_state) VALUES('a','{}')",
            [],
        )
        .unwrap();
    assert_eq!(
        err(RateLimitManager::new(
            ["a".into()],
            FakeClock::new(),
            Some(db)
        )),
        SchedulerError::RateStateUnavailable
    );
}

#[derive(Default)]
struct Keys {
    value: Mutex<Option<Vec<u8>>>,
    unavailable: AtomicBool,
}
impl crate::security::secrets::UnlockKeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, crate::security::secrets::SecretError> {
        if self.unavailable.load(Ordering::Acquire) {
            Err(crate::security::secrets::SecretError::CredentialStoreUnavailable)
        } else {
            Ok(self.value.lock().unwrap().clone())
        }
    }
    fn store(&self, key: &[u8]) -> Result<(), crate::security::secrets::SecretError> {
        *self.value.lock().unwrap() = Some(key.to_vec());
        Ok(())
    }
    fn delete(&self) -> Result<(), crate::security::secrets::SecretError> {
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}
#[test]
fn successful_secret_mutations_invalidate_api_keys_cloudflare_token_and_account() {
    use crate::security::secrets::{SecretKey, SecretStore};
    let dir = TempDirectory::new();
    let keys = Arc::new(Keys::default());
    let store = SecretStore::with_key_store(dir.0.clone(), keys.clone());
    let (tx, _) = mpsc::unbounded_channel();
    let runtime = super::ProviderRuntime::new(registry(
        tx,
        None,
        false,
        &["gemini", "groq", "mistral", "cloudflare"],
    ));
    runtime.connect_credentials(&store);
    for (id, key) in [
        ("gemini", SecretKey::GeminiApiKey),
        ("groq", SecretKey::GroqApiKey),
        ("mistral", SecretKey::MistralApiKey),
        ("cloudflare", SecretKey::CloudflareApiToken),
        ("cloudflare", SecretKey::CloudflareAccountId),
    ] {
        quota(&runtime.scheduler, id, QuotaDimension::RequestsPerDay, 0);
        let before = runtime
            .scheduler
            .rate_snapshot()
            .into_iter()
            .find(|s| s.provider_id == id)
            .unwrap()
            .context_generation;
        assert_eq!(
            err(runtime
                .scheduler
                .rate
                .reserve(id, "m", before, None, &AtomicBool::new(false))),
            SchedulerError::RateCapacityExceeded
        );
        store
            .set_secret(key, b"synthetic-private-credential-marker")
            .unwrap();
        let after = runtime
            .scheduler
            .rate_snapshot()
            .into_iter()
            .find(|s| s.provider_id == id)
            .unwrap();
        assert_eq!(after.context_generation, before + 1);
        assert!(after.constraints.iter().all(|c| c.capacity.is_none()));
        assert!(runtime
            .scheduler
            .rate
            .reserve(
                id,
                "m",
                after.context_generation,
                None,
                &AtomicBool::new(false)
            )
            .is_ok());
        quota(&runtime.scheduler, id, QuotaDimension::RequestsPerDay, 0);
        store
            .set_secret(key, b"synthetic-private-credential-marker")
            .unwrap();
        let unchanged = runtime
            .scheduler
            .rate_snapshot()
            .into_iter()
            .find(|s| s.provider_id == id)
            .unwrap();
        assert_eq!(unchanged.context_generation, after.context_generation);
        assert_eq!(
            err(runtime.scheduler.rate.reserve(
                id,
                "m",
                unchanged.context_generation,
                None,
                &AtomicBool::new(false)
            )),
            SchedulerError::RateCapacityExceeded
        );
    }
    let before = runtime
        .scheduler
        .rate_snapshot()
        .into_iter()
        .find(|s| s.provider_id == "cloudflare")
        .unwrap()
        .context_generation;
    store
        .set_secrets(&[
            (SecretKey::CloudflareApiToken, b"new-token".to_vec()),
            (SecretKey::CloudflareAccountId, b"new-account".to_vec()),
        ])
        .unwrap();
    assert_eq!(
        runtime
            .scheduler
            .rate_snapshot()
            .into_iter()
            .find(|s| s.provider_id == "cloudflare")
            .unwrap()
            .context_generation,
        before + 1
    );
    store
        .delete_secrets(&[
            SecretKey::CloudflareApiToken,
            SecretKey::CloudflareAccountId,
        ])
        .unwrap();
    assert_eq!(
        runtime
            .scheduler
            .rate_snapshot()
            .into_iter()
            .find(|s| s.provider_id == "cloudflare")
            .unwrap()
            .context_generation,
        before + 2
    );
    keys.unavailable.store(true, Ordering::Release);
    assert!(store.set_secret(SecretKey::GroqApiKey, b"failed").is_err());
    assert_eq!(
        runtime
            .scheduler
            .rate_snapshot()
            .into_iter()
            .find(|s| s.provider_id == "groq")
            .unwrap()
            .context_generation,
        1
    );
    let json = serde_json::to_string(&(
        runtime.scheduler.rate_snapshot(),
        runtime.scheduler.telemetry_snapshot(),
    ))
    .unwrap();
    for marker in [
        "synthetic-private-credential-marker",
        "new-token",
        "new-account",
    ] {
        assert!(!json.contains(marker));
    }
    let weak = Arc::downgrade(&runtime.scheduler);
    drop(runtime);
    assert!(weak.upgrade().is_none()); // observer does not create an ownership cycle
}

#[test]
fn concurrent_late_headers_cannot_restore_exhausted_credit_or_refund_new_facts() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(2),
        Some(2),
        None,
    );
    let (r1, o1) = h.reserve("a", "m", None).unwrap();
    let (r2, o2) = h.reserve("a", "m", None).unwrap();
    assert!(o1.started());
    assert!(o2.started());
    o2.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(2),
        Some(0),
        None,
    );
    drop(r2);
    o1.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(2),
        Some(1),
        None,
    );
    drop(r1);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateCapacityExceeded
    );
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(100),
        None,
    );
    let (r, o) = h.reserve("a", "m", Some(80)).unwrap();
    assert!(o.started());
    h.telemetry.invalidate_provider_quotas("a");
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(0),
        Some(0),
        None,
    );
    o.observed_usage([(UsageDimension::TotalTokens, Some(1))]);
    drop(r);
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateCapacityExceeded
    );
}
#[test]
fn response_remaining_does_not_double_debit_its_already_started_request() {
    let h = Harness::new();
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    o.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(2),
        Some(1),
        None,
    );
    drop(r);
    assert_eq!(
        h.bucket("a", QuotaDimension::RequestsPerDay)
            .effective_remaining,
        Some(1)
    );
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    drop(r);
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateCapacityExceeded
    );
}

#[test]
fn cumulative_prefix_can_increase_debit_but_cannot_prove_a_refund() {
    for measured in [30, 120] {
        let h = Harness::new();
        h.quota(
            "a",
            model("m"),
            QuotaDimension::TokensPerMinute,
            Some(1_000),
            Some(1_000),
            None,
        );
        let (r, o) = h.reserve("a", "m", Some(80)).unwrap();
        assert!(o.started());
        o.observed_usage([(UsageDimension::TotalTokens, Some(measured))]);
        if measured == 120 {
            assert_eq!(h.bucket("a", QuotaDimension::TokensPerMinute).consumed, 120);
            assert_eq!(
                err(h.reserve("a", "m", Some(900))),
                SchedulerError::RateCapacityExceeded
            );
        }
        o.finished(Some(&ProviderError::Timeout));
        drop(r);
        assert_eq!(
            h.bucket("a", QuotaDimension::TokensPerMinute).consumed,
            measured.max(80)
        );
    }
}

#[test]
fn local_window_overflow_never_resets_consumption_before_validation() {
    let h = Harness::new();
    h.rate
        .set_policy("a", local(QuotaDimension::RequestsPerMinute, 1, 10))
        .unwrap();
    let (r, o) = h.reserve("a", "m", None).unwrap();
    assert!(o.started());
    drop(r);
    h.clock.0.lock().unwrap().monotonic_ms = u64::MAX;
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::RateStateUnavailable
    );
    let b = h.bucket("a", QuotaDimension::RequestsPerMinute);
    assert_eq!(b.consumed, 1);
    assert_eq!(b.effective_remaining, Some(0));
}

#[test]
fn local_policy_provenance_is_distinct_and_telemetry_configuration_is_not_external_capacity() {
    let h = Harness::new();
    h.telemetry.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerDay,
        Some(0),
        Some(0),
        None,
        Provenance::UserConfiguration,
    );
    assert!(h.reserve("a", "m", None).is_ok());
    assert!(h.rate.snapshots()[0].constraints.is_empty());
    h.rate.set_policy("a", daily(Some(0), None)).unwrap();
    assert_eq!(
        err(h.reserve("a", "m", None)),
        SchedulerError::DailyBudgetExceeded
    );
    let b = h.bucket("a", QuotaDimension::RequestsPerDay);
    assert_eq!(b.source, ConstraintSource::DailyBudget);
    assert_eq!(b.provenance, Some(Provenance::UserConfiguration));
    assert!(b.external.is_none());
}

#[tokio::test]
async fn credential_era_invalidates_legacy_cooldown_and_rejects_late_old_429() {
    use crate::security::secrets::{CredentialContextObserver, SecretKey};
    for rotate_before_completion in [false, true] {
        let (tx, mut calls) = mpsc::unbounded_channel();
        let s = Arc::new(Scheduler::new(registry(tx, None, false, &["groq"])));
        quota(&s, "groq", QuotaDimension::RequestsPerDay, 1);
        let (run, _) = start(
            s.clone(),
            request(&["groq"], ProviderSelection::Fixed("groq".into())),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        let call = bounded(calls.recv()).await.unwrap();
        if rotate_before_completion {
            s.credentials_changed(&[SecretKey::GroqApiKey]);
        }
        call.finish
            .send((
                None,
                Some(ProviderError::RateLimited {
                    retry_after_ms: Some(10_000),
                }),
            ))
            .unwrap();
        assert!(matches!(
            bounded(run).await.unwrap(),
            Err(SchedulerError::Provider(ProviderError::RateLimited { .. }))
        ));
        if !rotate_before_completion {
            assert!(s.status()[0].cooldown_ms > 0);
            s.credentials_changed(&[SecretKey::GroqApiKey]);
        }
        assert_eq!(s.status()[0].cooldown_ms, 0);
        let (run, _) = start(
            s.clone(),
            request(&["groq"], ProviderSelection::Fixed("groq".into())),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        bounded(calls.recv())
            .await
            .unwrap()
            .finish
            .send((None, None))
            .unwrap();
        assert!(bounded(run).await.unwrap().is_ok());
        clean(&s);
    }
}

fn total_usage(total: u32) -> ProviderUsage {
    ProviderUsage {
        total_tokens: Some(total),
        output_tokens_measured: true,
        ..ProviderUsage::default()
    }
}
fn reject_storage(db: &crate::persistence::database::Database) {
    db.open().unwrap().execute_batch("CREATE TRIGGER reject_rate BEFORE UPDATE ON cognitive_rate_state BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
}
fn restore_storage(db: &crate::persistence::database::Database) {
    db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_rate")
        .unwrap();
}
fn assert_uncertain(h: &Harness, consumed: u64) {
    let b = h.bucket("a", QuotaDimension::TokensPerDay);
    assert_eq!(b.consumed, consumed);
    assert_eq!(b.unaccounted_token_calls, 1);
    assert_eq!(b.effective_remaining, None);
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateStateUnavailable
    );
}

#[test]
fn crash_with_live_unbounded_http_guard_preserves_durable_uncertainty() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    h.rate.set_policy("a", daily(None, Some(100))).unwrap();
    let (guard, obs) = h.reserve("a", "m", None).unwrap();
    assert!(obs.started());
    assert_uncertain(&h, 0);
    // Reopen while guard is alive: neither finish nor Drop can rescue this crash.
    let restarted = durable(db.clone(), FakeClock::new());
    assert_uncertain(&restarted, 0);
    drop(restarted);
    drop(guard);
    assert_uncertain(&durable(db, FakeClock::new()), 0);
}

#[test]
fn unbounded_definitive_usage_is_durable_even_before_guard_drop_and_idempotent() {
    for total in [0, 30, 120] {
        let dir = TempDirectory::new();
        let db = dir.db();
        let h = durable(db.clone(), FakeClock::new());
        h.rate.set_policy("a", daily(None, Some(100))).unwrap();
        let (guard, obs) = h.reserve("a", "m", None).unwrap();
        assert!(obs.started());
        obs.usage(total_usage(total));
        assert_uncertain(&h, total as u64);
        obs.final_usage(total_usage(total));
        obs.final_usage(total_usage(total));
        for state in [&h, &durable(db.clone(), FakeClock::new())] {
            let b = state.bucket("a", QuotaDimension::TokensPerDay);
            assert_eq!(b.consumed, total as u64);
            assert_eq!(b.unaccounted_token_calls, 0);
            assert_eq!(
                b.effective_remaining,
                Some(100_u64.saturating_sub(total as u64))
            );
        }
        drop(guard);
        let restarted = durable(db, FakeClock::new());
        assert_eq!(
            restarted.bucket("a", QuotaDimension::TokensPerDay).consumed,
            total as u64
        );
        assert_eq!(
            restarted
                .bucket("a", QuotaDimension::TokensPerDay)
                .unaccounted_token_calls,
            0
        );
    }
}

#[test]
fn cumulative_usage_and_no_usage_preserve_uncertainty_after_finish_and_restart() {
    for total in [None, Some(12)] {
        let dir = TempDirectory::new();
        let db = dir.db();
        let h = durable(db.clone(), FakeClock::new());
        h.rate.set_policy("a", daily(None, Some(100))).unwrap();
        let (guard, obs) = h.reserve("a", "m", None).unwrap();
        assert!(obs.started());
        if let Some(total) = total {
            obs.usage(total_usage(total));
        }
        obs.finished(Some(&ProviderError::Timeout));
        drop(guard);
        assert_uncertain(&h, total.unwrap_or(0) as u64);
        assert_uncertain(&durable(db, FakeClock::new()), total.unwrap_or(0) as u64);
    }
}

#[test]
fn unbounded_preflight_and_cancelled_reservations_leave_no_durable_uncertainty() {
    for cancel in [false, true] {
        let dir = TempDirectory::new();
        let db = dir.db();
        let h = durable(db.clone(), FakeClock::new());
        h.rate.set_policy("a", daily(None, Some(100))).unwrap();
        let (guard, obs) = h.reserve("a", "m", None).unwrap();
        if cancel {
            assert!(!obs.started_unless_cancelled(&AtomicBool::new(true)));
            assert_eq!(obs.rate_error(), Some(SchedulerError::Cancelled));
        }
        drop(guard);
        let b = durable(db, FakeClock::new()).bucket("a", QuotaDimension::TokensPerDay);
        assert_eq!(b.unaccounted_token_calls, 0);
        assert_eq!(b.effective_remaining, Some(100));
    }
}

#[test]
fn unresolved_local_tokens_deny_stale_headroom_at_reserve_and_queued_http_revalidation() {
    for policy in [
        daily(None, Some(100)),
        local(QuotaDimension::TokensPerDay, 100, 60_000),
    ] {
        let h = Harness::new();
        h.rate.set_policy("a", policy).unwrap();
        let (queued, queued_obs) = h.reserve("a", "m", Some(80)).unwrap();
        let (unbounded, obs) = h.reserve("a", "m", None).unwrap();
        assert!(obs.started());
        obs.usage(total_usage(12));
        assert_eq!(
            err(h.reserve("a", "m", Some(1))),
            SchedulerError::RateStateUnavailable
        );
        assert!(!queued_obs.started());
        assert_eq!(
            queued_obs.rate_error(),
            Some(SchedulerError::RateStateUnavailable)
        );
        drop(queued);
        drop(unbounded);
        assert_uncertain(&h, 12);
        // No bound means no fabricated token enforcement, even while unknown.
        assert!(h.reserve("a", "m", None).is_ok());
    }
}

#[test]
fn external_uncertainty_requires_fresh_remaining_or_legitimate_reset() {
    for reset in [false, true] {
        let h = Harness::new();
        h.quota(
            "a",
            model("m"),
            QuotaDimension::TokensPerDay,
            Some(100),
            Some(100),
            reset.then_some(Timing::DelayMs(10)),
        );
        let (guard, obs) = h.reserve("a", "m", None).unwrap();
        assert!(obs.started());
        obs.usage(total_usage(12));
        drop(guard);
        assert_uncertain(&h, 12);
        if reset {
            h.clock.advance(9);
            assert_uncertain(&h, 12);
            h.clock.advance(1);
        } else {
            h.quota(
                "a",
                model("m"),
                QuotaDimension::TokensPerDay,
                Some(100),
                None,
                None,
            );
            assert_eq!(
                h.bucket("a", QuotaDimension::TokensPerDay)
                    .effective_remaining,
                None
            );
            assert_eq!(
                err(h.reserve("a", "m", Some(1))),
                SchedulerError::RateStateUnavailable
            );
            h.clock.advance(100_000); // Unknown reset cannot refill anything.
            assert_eq!(
                h.bucket("a", QuotaDimension::TokensPerDay)
                    .effective_remaining,
                None
            );
            h.quota(
                "a",
                model("m"),
                QuotaDimension::TokensPerDay,
                Some(100),
                Some(88),
                None,
            );
        }
        let b = h.bucket("a", QuotaDimension::TokensPerDay);
        assert_eq!(b.unaccounted_token_calls, 0);
        assert!(h.reserve("a", "m", Some(1)).is_ok());
    }
}

#[test]
fn active_unbounded_external_call_remains_uncertain_after_fresh_headers() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerDay,
        Some(100),
        Some(100),
        None,
    );
    let (guard, obs) = h.reserve("a", "m", None).unwrap();
    assert!(obs.started());
    for remaining in [90, 88] {
        h.quota(
            "a",
            model("m"),
            QuotaDimension::TokensPerDay,
            Some(100),
            Some(remaining),
            None,
        );
        assert_uncertain(&h, 0);
    }
    obs.final_usage(total_usage(12));
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .unaccounted_token_calls,
        0
    );
    drop(guard);
    assert_eq!(h.bucket("a", QuotaDimension::TokensPerDay).consumed, 12);
}

#[test]
fn credential_rotation_preserves_local_uncertainty_and_only_exact_reset_clears_it() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    h.rate
        .set_policy("a", local(QuotaDimension::TokensPerDay, 100, 10))
        .unwrap();
    let (old, old_obs) = h.reserve("a", "m", None).unwrap();
    assert!(old_obs.started());
    h.telemetry.invalidate_provider_quotas("a");
    assert_uncertain(&h, 0);
    assert_uncertain(&durable(db.clone(), FakeClock::new()), 0);
    h.clock.advance(9);
    assert_uncertain(&h, 0);
    h.clock.advance(1);
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .effective_remaining,
        Some(100)
    );
    let (new, new_obs) = h.reserve("a", "m", None).unwrap();
    assert!(new_obs.started());
    old_obs.final_usage(total_usage(30));
    drop(old); // Old epoch cannot debit/refund/clear the new call's marker.
    assert_uncertain(&h, 0);
    new_obs.final_usage(total_usage(20));
    drop(new);
    assert_eq!(h.bucket("a", QuotaDimension::TokensPerDay).consumed, 20);
    let clock = FakeClock::new();
    clock.advance(10);
    let b = durable(db, clock).bucket("a", QuotaDimension::TokensPerDay);
    assert_eq!(b.unaccounted_token_calls, 0);
    assert_eq!(b.effective_remaining, Some(80));
}

#[test]
fn unbounded_storage_failure_at_write_ahead_or_started_never_allows_http() {
    for fail_started in [false, true] {
        let dir = TempDirectory::new();
        let db = dir.db();
        let h = durable(db.clone(), FakeClock::new());
        h.rate.set_policy("a", daily(None, Some(100))).unwrap();
        if fail_started {
            let (guard, obs) = h.reserve("a", "m", None).unwrap();
            reject_storage(&db);
            assert!(!obs.started());
            assert_eq!(obs.rate_error(), Some(SchedulerError::RateStateUnavailable));
            drop(guard);
        } else {
            reject_storage(&db);
            assert_eq!(
                err(h.reserve("a", "m", None)),
                SchedulerError::RateStateUnavailable
            );
        }
        assert!(h.rate.snapshots()[0].persistence_failed);
        assert_eq!(
            err(h.reserve("a", "m", None)),
            SchedulerError::RateStateUnavailable
        );
        restore_storage(&db);
        let b = durable(db, FakeClock::new()).bucket("a", QuotaDimension::TokensPerDay);
        // HTTP was prohibited: the previous durable zero is safe.
        assert_eq!(b.effective_remaining, Some(100));
        assert_eq!(b.unaccounted_token_calls, 0);
    }
}

#[test]
fn unbounded_storage_failure_at_prefix_final_or_drop_never_recovers_known_credit() {
    for phase in ["prefix", "final", "final_zero", "drop"] {
        let dir = TempDirectory::new();
        let db = dir.db();
        let h = durable(db.clone(), FakeClock::new());
        h.rate.set_policy("a", daily(None, Some(100))).unwrap();
        let (guard, obs) = h.reserve("a", "m", None).unwrap();
        assert!(obs.started());
        reject_storage(&db);
        match phase {
            "prefix" => obs.usage(total_usage(120)),
            "final" => obs.final_usage(total_usage(120)),
            "final_zero" => obs.final_usage(total_usage(0)),
            _ => (),
        }
        if phase != "drop" {
            assert!(h.rate.snapshots()[0].persistence_failed);
            assert_eq!(
                err(h.reserve("a", "m", None)),
                SchedulerError::RateStateUnavailable
            );
            assert_eq!(
                h.bucket("a", QuotaDimension::TokensPerDay).consumed,
                if phase == "final_zero" { 0 } else { 120 }
            );
        }
        // Disk contains the pre-HTTP marker even while the owner is still alive.
        assert_uncertain(&durable(db.clone(), FakeClock::new()), 0);
        drop(guard);
        assert!(h.rate.snapshots()[0].persistence_failed);
        restore_storage(&db);
        assert_uncertain(&durable(db, FakeClock::new()), 0);
    }
}

#[test]
fn failed_violated_bound_persistence_has_durable_recovery_marker() {
    let dir = TempDirectory::new();
    let db = dir.db();
    let h = durable(db.clone(), FakeClock::new());
    h.rate.set_policy("a", daily(None, Some(100))).unwrap();
    let (guard, obs) = h.reserve("a", "m", Some(30)).unwrap();
    assert!(obs.started());
    reject_storage(&db);
    obs.final_usage(total_usage(120));
    assert!(h.rate.snapshots()[0].persistence_failed);
    assert_eq!(h.bucket("a", QuotaDimension::TokensPerDay).consumed, 120);
    assert_uncertain(&durable(db.clone(), FakeClock::new()), 30);
    drop(guard);
    restore_storage(&db);
    assert_uncertain(&durable(db, FakeClock::new()), 30);
}

// Shared fixture attachment for production-adapter lifecycle tests. It supplies
// an explicit bound only when the test asks for one; production adapters stay None.
pub(super) fn adapter_token_accounting(id: &str) -> (Arc<RateLimitManager>, TelemetryStore) {
    let rate =
        RateLimitManager::new([id.into()], Arc::new(SystemRateClock::default()), None).unwrap();
    rate.set_policy(id, daily(None, Some(100))).unwrap();
    let telemetry = TelemetryStore::with_rate([id.into()], rate.clone());
    (rate, telemetry)
}
pub(super) fn adapter_token_reservation(
    rate: &Arc<RateLimitManager>,
    obs: &InvocationObservation<'_>,
    id: &str,
    bound: Option<u64>,
) -> RateReservation {
    let guard = rate
        .reserve(
            id,
            "fixture",
            0,
            bound.map(|n| TokenUpperBound::explicit_total(n).unwrap()),
            &AtomicBool::new(false),
        )
        .unwrap();
    obs.attach_rate(guard.handle());
    guard
}

#[test]
fn cancellation_during_unbounded_durable_commit_rolls_back_marker_on_graceful_drop() {
    struct CancelOnClock {
        clock: Arc<FakeClock>,
        armed: AtomicBool,
        cancelled: Arc<AtomicBool>,
    }
    impl RateClock for CancelOnClock {
        fn now(&self) -> ClockReading {
            if self.armed.swap(false, Ordering::AcqRel) {
                self.cancelled.store(true, Ordering::Release);
            }
            self.clock.now()
        }
    }
    let dir = TempDirectory::new();
    let db = dir.db();
    let cancelled = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(CancelOnClock {
        clock: FakeClock::new(),
        armed: AtomicBool::new(false),
        cancelled: cancelled.clone(),
    });
    let rate = RateLimitManager::new(["a".into()], clock.clone(), Some(db.clone())).unwrap();
    rate.set_policy("a", daily(None, Some(100))).unwrap();
    let telemetry = TelemetryStore::with_rate(["a".into()], rate.clone());
    let obs = telemetry.attempt("a");
    let guard = rate.reserve("a", "m", 0, None, &cancelled).unwrap();
    obs.attach_rate(guard.handle());
    clock.armed.store(true, Ordering::Release);
    assert!(!obs.started_unless_cancelled(&cancelled));
    assert_eq!(obs.rate_error(), Some(SchedulerError::Cancelled));
    assert!(matches!(
        telemetry.snapshots()[0].usage[&UsageDimension::Requests].observed,
        Fact::Known { value: 0, .. }
    ));
    // Crash at this point may retain a false-positive marker. No fictitious credit.
    assert_uncertain(&durable(db.clone(), FakeClock::new()), 0);
    drop(guard);
    let b = durable(db, FakeClock::new()).bucket("a", QuotaDimension::TokensPerDay);
    assert_eq!(b.unaccounted_token_calls, 0);
    assert_eq!(b.effective_remaining, Some(100));
}

#[test]
fn definitive_usage_clears_only_its_own_marker_and_reset_overflow_clears_nothing() {
    let h = Harness::new();
    h.rate
        .set_policy("a", local(QuotaDimension::TokensPerDay, 100, 10))
        .unwrap();
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", None).unwrap();
    assert!(first.started());
    assert!(second.started());
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .unaccounted_token_calls,
        2
    );
    first.final_usage(total_usage(30));
    first.final_usage(total_usage(30));
    drop(one);
    second.usage(total_usage(40));
    assert_uncertain(&h, 70);
    // Epoch/deadline validation precedes clearing uncertainty.
    h.clock.0.lock().unwrap().monotonic_ms = u64::MAX;
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateStateUnavailable
    );
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerDay)
            .unaccounted_token_calls,
        1
    );
    drop(two);
}

// Header scripts acknowledge each normalized observation, keeping scheduling and
// response order explicit. No network, sleeps or Tokio completion-order inference.
struct OrderedHeaderCall {
    label: String,
    commands: mpsc::UnboundedSender<OrderedHeaderCommand>,
}
enum OrderedHeaderCommand {
    Quotas {
        requests: u64,
        tokens: u64,
        reset: Option<Timing>,
        ack: oneshot::Sender<()>,
    },
    Finish,
}
struct OrderedHeaderProvider(mpsc::UnboundedSender<OrderedHeaderCall>);
impl Provider for OrderedHeaderProvider {
    fn token_upper_bound(&self, _: &ProviderRequest) -> Option<TokenUpperBound> {
        Some(TokenUpperBound::explicit_total(10).unwrap())
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        panic!("observed invocation required")
    }
    fn execute_observed<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        observation: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if !observation.started_unless_cancelled(cancelled) {
                return Err(ProviderError::Cancelled);
            }
            let (tx, mut rx) = mpsc::unbounded_channel();
            self.0
                .send(OrderedHeaderCall {
                    label: request.input.clone(),
                    commands: tx,
                })
                .unwrap();
            while let Some(command) = rx.recv().await {
                match command {
                    OrderedHeaderCommand::Quotas {
                        requests,
                        tokens,
                        reset,
                        ack,
                    } => {
                        observation.quota(
                            model("m"),
                            QuotaDimension::RequestsPerDay,
                            Some(100),
                            Some(requests),
                            reset,
                        );
                        observation.quota(
                            model("m"),
                            QuotaDimension::TokensPerMinute,
                            Some(100),
                            Some(tokens),
                            reset,
                        );
                        ack.send(()).unwrap();
                    }
                    OrderedHeaderCommand::Finish => {
                        let usage = total_usage(10);
                        observation.final_usage(usage);
                        return Ok(ProviderResponse {
                            text: "fixture".into(),
                            usage,
                        });
                    }
                }
            }
            panic!("header script abandoned")
        })
    }
}
fn ordering_scheduler(cap: usize) -> (Arc<Scheduler>, mpsc::UnboundedReceiver<OrderedHeaderCall>) {
    ordering_scheduler_with_clock(cap, FakeClock::new())
}
fn ordering_scheduler_with_clock(
    cap: usize,
    clock: Arc<FakeClock>,
) -> (Arc<Scheduler>, mpsc::UnboundedReceiver<OrderedHeaderCall>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut registry = ProviderRegistry::default();
    registry
        .register(
            ProviderConfig {
                id: "a".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(OrderedHeaderProvider(tx)),
        )
        .unwrap();
    let scheduler = Arc::new(
        Scheduler::with_rate_config(
            registry,
            AdmissionConfig {
                max_concurrency_per_provider: cap,
                ..AdmissionConfig::default()
            },
            clock,
            None,
        )
        .unwrap(),
    );
    quota(&scheduler, "a", QuotaDimension::RequestsPerDay, 100);
    quota(&scheduler, "a", QuotaDimension::TokensPerMinute, 100);
    (scheduler, rx)
}
fn ordering_request(class: TrafficClass, label: &str) -> ProviderTaskRequest {
    let mut r = request(&["a"], ProviderSelection::Fixed("a".into()));
    r.traffic_class = class;
    r.input = label.into();
    r
}
async fn headers(call: &OrderedHeaderCall, requests: u64, tokens: u64, reset: Option<Timing>) {
    let (ack, rx) = oneshot::channel();
    call.commands
        .send(OrderedHeaderCommand::Quotas {
            requests,
            tokens,
            reset,
            ack,
        })
        .unwrap();
    bounded(rx).await.unwrap();
}

#[tokio::test]
async fn rate_ordering_priority_overtaking_uses_http_start_not_reservation_order() {
    let (s, mut calls) = ordering_scheduler(1);
    // Hold admission without creating a rate reservation: the two tasks below
    // receive reservation #1 Background and #2 ForegroundInteractive exactly.
    let permit = held(&s).await;
    let (background, mut bg_events) = start(
        s.clone(),
        ordering_request(TrafficClass::Background, "background"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    event(&mut bg_events, |e| {
        matches!(e, SchedulerEvent::Queued { .. })
    })
    .await;
    assert_eq!(s.rate_snapshot()[0].pending_reservations, 1);
    let (foreground, mut fg_events) = start(
        s.clone(),
        ordering_request(TrafficClass::ForegroundInteractive, "foreground"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    event(&mut fg_events, |e| {
        matches!(e, SchedulerEvent::Queued { .. })
    })
    .await;
    assert_eq!(s.rate_snapshot()[0].pending_reservations, 2);
    let admission = &s.admission_snapshot()[0];
    assert_eq!(admission.max_concurrency, 1);
    assert_eq!(admission.queued_by_class[&TrafficClass::Background], 1);
    assert_eq!(
        admission.queued_by_class[&TrafficClass::ForegroundInteractive],
        1
    );
    assert!(calls.try_recv().is_err());
    drop(permit);
    let first = bounded(calls.recv()).await.unwrap();
    assert_eq!(first.label, "foreground");
    headers(&first, 9, 90, None).await;
    first.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(foreground).await.unwrap().is_ok());
    let second = bounded(calls.recv()).await.unwrap();
    assert_eq!(second.label, "background");
    headers(&second, 4, 40, None).await;
    second.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(background).await.unwrap().is_ok());
    let requests = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    assert_eq!(requests.capacity, Some(4));
    assert_eq!(requests.consumed, 0); // Its request is already reflected in remaining.
    assert_eq!(requests.effective_remaining, Some(4));
    let tokens = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(tokens.capacity, Some(40));
    assert_eq!(tokens.consumed, 10);
    assert_eq!(tokens.effective_remaining, Some(30));
    assert_eq!(tokens.reset_in_ms, None);
    clean(&s);
}

#[tokio::test]
async fn rate_ordering_older_http_response_only_tightens_without_reset_or_double_debit() {
    for older_is_stricter in [false, true] {
        let (s, mut calls) = ordering_scheduler(2);
        let (background, _) = start(
            s.clone(),
            ordering_request(TrafficClass::Background, "older"),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        let older = bounded(calls.recv()).await.unwrap();
        let (foreground, _) = start(
            s.clone(),
            ordering_request(TrafficClass::ForegroundInteractive, "newer"),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        let newer = bounded(calls.recv()).await.unwrap();
        headers(&newer, 4, 40, Some(Timing::DelayMs(10))).await;
        newer.commands.send(OrderedHeaderCommand::Finish).unwrap();
        assert!(bounded(foreground).await.unwrap().is_ok());
        let before_r = constraint(&s, "a", QuotaDimension::RequestsPerDay);
        let before_t = constraint(&s, "a", QuotaDimension::TokensPerMinute);
        assert_eq!(before_r.effective_remaining, Some(3));
        assert_eq!(before_t.effective_remaining, Some(20));
        // The old response arrives last. Its immediate reset must not revive
        // credit; a smaller balance must still constrain external shared quota.
        headers(
            &older,
            if older_is_stricter { 1 } else { 9 },
            if older_is_stricter { 10 } else { 90 },
            Some(Timing::DelayMs(0)),
        )
        .await;
        let after_r = constraint(&s, "a", QuotaDimension::RequestsPerDay);
        let after_t = constraint(&s, "a", QuotaDimension::TokensPerMinute);
        assert_eq!(after_r.consumed, before_r.consumed);
        assert_eq!(after_t.consumed, before_t.consumed);
        assert_eq!(after_r.reset_in_ms, Some(10));
        assert_eq!(after_t.reset_in_ms, Some(10));
        assert_eq!(
            after_r.effective_remaining,
            Some(if older_is_stricter { 1 } else { 3 })
        );
        assert_eq!(
            after_t.effective_remaining,
            Some(if older_is_stricter { 0 } else { 20 })
        );
        older.commands.send(OrderedHeaderCommand::Finish).unwrap();
        assert!(bounded(background).await.unwrap().is_ok());
        assert_eq!(
            constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
            after_r.effective_remaining
        );
        assert_eq!(
            constraint(&s, "a", QuotaDimension::TokensPerMinute).effective_remaining,
            after_t.effective_remaining
        );
        clean(&s);
    }
}

#[test]
fn rate_ordering_reversed_reservations_and_independent_fact_barrier() {
    let h = Harness::new();
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", None).unwrap();
    assert!(first.context_generation() == second.context_generation());
    assert!(second.started());
    second.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(9),
        None,
    );
    drop(two);
    assert!(first.started());
    first.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(4),
        None,
    );
    drop(one);
    assert_eq!(
        h.bucket("a", QuotaDimension::RequestsPerDay)
            .effective_remaining,
        Some(4)
    );

    let (guard, old) = h.reserve("a", "m", None).unwrap();
    assert!(old.started());
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(2),
        None,
    );
    old.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(9),
        Some(Timing::DelayMs(0)),
    );
    drop(guard);
    let b = h.bucket("a", QuotaDimension::RequestsPerDay);
    assert_eq!(b.effective_remaining, Some(1));
    assert_eq!(b.reset_in_ms, None);
    // A detached observation has no live authorized invocation to correlate.
    old.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(100),
        None,
    );
    assert_eq!(
        h.bucket("a", QuotaDimension::RequestsPerDay)
            .effective_remaining,
        Some(1)
    );
}

#[test]
fn rate_ordering_stale_partial_headers_cannot_refill_or_clear_token_uncertainty() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(100),
        None,
    );
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", Some(10)).unwrap();
    assert!(second.started());
    second.quota(
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(40),
        None,
    );
    second.final_usage(total_usage(10));
    drop(two);
    assert!(first.started());
    first.quota(
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(20),
        None,
    );
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerMinute)
            .unaccounted_token_calls,
        1
    );
    // Same start sequence cannot clear uncertainty or invent refill/reset via a
    // second partial observation. No newer HTTP authorization has occurred.
    first.quota(
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        None,
        Some(Timing::DelayMs(0)),
    );
    drop(one);
    let b = h.bucket("a", QuotaDimension::TokensPerMinute);
    assert_eq!(b.effective_remaining, None);
    assert_eq!(b.unaccounted_token_calls, 1);
    assert_eq!(b.reset_in_ms, None);
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateStateUnavailable
    );
    assert!(h.reserve("a", "other-model", Some(10)).is_ok());
    assert!(h.reserve("b", "m", Some(10)).is_ok());
}

#[test]
fn rate_ordering_older_token_ceiling_survives_proven_refund() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerDay,
        Some(100),
        Some(100),
        None,
    );
    let (one, first) = h.reserve("a", "m", Some(10)).unwrap();
    let (two, second) = h.reserve("a", "m", Some(10)).unwrap();
    assert!(first.started());
    assert!(second.started());
    second.quota(
        model("m"),
        QuotaDimension::TokensPerDay,
        Some(100),
        Some(40),
        None,
    );
    second.final_usage(total_usage(10));
    drop(two);
    first.quota(
        model("m"),
        QuotaDimension::TokensPerDay,
        Some(100),
        Some(10),
        None,
    );
    let before = h.bucket("a", QuotaDimension::TokensPerDay);
    assert_eq!(before.consumed, 20); // Tightening never repeats a debit.
    assert_eq!(before.effective_remaining, Some(0));
    first.final_usage(total_usage(0));
    drop(one);
    let after = h.bucket("a", QuotaDimension::TokensPerDay);
    assert_eq!(after.consumed, 10); // Only the proven unused bound was refunded.
    assert_eq!(after.effective_remaining, Some(10));
    assert!(h.reserve("a", "m", Some(10)).is_ok());
    assert_eq!(
        err(h.reserve("a", "m", Some(11))),
        SchedulerError::RateCapacityExceeded
    );
}

#[tokio::test]
async fn rate_overlap_history_survives_peer_finish_and_isolated_call_recovers_fresh_authority() {
    let (s, mut calls) = ordering_scheduler(2);
    let (a, _) = start(
        s.clone(),
        ordering_request(TrafficClass::Background, "A"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let first = bounded(calls.recv()).await.unwrap(); // A crossed started.
    let (b, _) = start(
        s.clone(),
        ordering_request(TrafficClass::ForegroundInteractive, "B"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let second = bounded(calls.recv()).await.unwrap(); // B crossed started while A was live.
    assert_eq!(s.admission_snapshot()[0].active_calls, 2);
    headers(&first, 4, 40, None).await;
    first.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(a).await.unwrap().is_ok());
    assert_eq!(s.rate_snapshot()[0].pending_reservations, 1);
    assert_eq!(s.admission_snapshot()[0].active_calls, 1);
    let requests = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    let tokens = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(requests.capacity, Some(4));
    assert_eq!(requests.consumed, 1);
    assert_eq!(requests.effective_remaining, Some(3));
    assert_eq!(tokens.capacity, Some(40));
    assert_eq!(tokens.consumed, 20);
    assert_eq!(tokens.effective_remaining, Some(20));

    headers(&second, 9, 90, None).await;
    let after_requests = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    let after_tokens = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(after_requests.capacity, requests.capacity);
    assert_eq!(after_requests.consumed, requests.consumed);
    assert_eq!(
        after_requests.effective_remaining,
        requests.effective_remaining
    );
    assert_eq!(after_tokens.capacity, tokens.capacity);
    assert_eq!(after_tokens.consumed, tokens.consumed);
    assert_eq!(after_tokens.effective_remaining, tokens.effective_remaining);
    assert_eq!(after_requests.reset_in_ms, None);
    assert_eq!(after_tokens.reset_in_ms, None);
    second.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(b).await.unwrap().is_ok());
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).consumed,
        20
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).consumed,
        1
    );
    clean(&s);

    let (c, _) = start(
        s.clone(),
        ordering_request(TrafficClass::ForegroundInteractive, "C"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let isolated = bounded(calls.recv()).await.unwrap();
    assert_eq!(isolated.label, "C");
    assert_eq!(s.admission_snapshot()[0].active_calls, 1);
    headers(&isolated, 9, 90, None).await;
    isolated
        .commands
        .send(OrderedHeaderCommand::Finish)
        .unwrap();
    assert!(bounded(c).await.unwrap().is_ok());
    let requests = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    let tokens = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(requests.capacity, Some(9));
    assert_eq!(requests.consumed, 0);
    assert_eq!(requests.effective_remaining, Some(9));
    assert_eq!(tokens.capacity, Some(90));
    assert_eq!(tokens.consumed, 10);
    assert_eq!(tokens.effective_remaining, Some(80));
    assert_eq!(requests.reset_in_ms, None);
    assert_eq!(tokens.reset_in_ms, None);
    clean(&s);
}

#[tokio::test]
async fn rate_overlap_later_started_header_still_tightens_after_peer_finishes() {
    let (s, mut calls) = ordering_scheduler(2);
    let (a, _) = start(
        s.clone(),
        ordering_request(TrafficClass::Background, "A"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let first = bounded(calls.recv()).await.unwrap();
    let (b, _) = start(
        s.clone(),
        ordering_request(TrafficClass::ForegroundInteractive, "B"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let second = bounded(calls.recv()).await.unwrap();
    headers(&first, 9, 90, None).await;
    first.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(a).await.unwrap().is_ok());
    let before_r = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    let before_t = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(before_r.effective_remaining, Some(8));
    assert_eq!(before_t.capacity, Some(80)); // Existing overlap clamp retains prior known credit.
    assert_eq!(before_t.effective_remaining, Some(60));
    headers(&second, 4, 40, Some(Timing::DelayMs(0))).await;
    let after_r = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    let after_t = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(after_r.effective_remaining, Some(4));
    assert_eq!(after_t.effective_remaining, Some(30));
    assert_eq!(after_r.consumed, before_r.consumed);
    assert_eq!(after_t.consumed, before_t.consumed);
    assert_eq!(after_r.reset_in_ms, None);
    assert_eq!(after_t.reset_in_ms, None);
    second.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(b).await.unwrap().is_ok());
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
        Some(4)
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).effective_remaining,
        Some(30)
    );
    clean(&s);
}

#[test]
fn rate_overlap_transitive_group_survives_original_peers_but_not_quiescence() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(100),
        None,
    );
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", None).unwrap();
    assert!(first.started());
    assert!(second.started());
    first.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(4),
        None,
    );
    drop(one);
    let (three, third) = h.reserve("a", "m", None).unwrap();
    assert!(third.started()); // B still live: C inherits A/B's overlap origin.
    drop(two);
    let before = h.bucket("a", QuotaDimension::RequestsPerDay);
    third.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(9),
        None,
    );
    let after = h.bucket("a", QuotaDimension::RequestsPerDay);
    assert_eq!(after.capacity, before.capacity);
    assert_eq!(after.consumed, before.consumed);
    assert_eq!(after.effective_remaining, Some(2));
    drop(three);
    // No active peer: D does not inherit the historical group.
    let (four, fourth) = h.reserve("a", "m", None).unwrap();
    assert!(fourth.started());
    fourth.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(9),
        None,
    );
    drop(four);
    assert_eq!(
        h.bucket("a", QuotaDimension::RequestsPerDay)
            .effective_remaining,
        Some(9)
    );
}

#[test]
fn rate_overlap_preserves_completed_uncertainty_and_does_not_replay_reset() {
    let h = Harness::new();
    h.quota(
        "a",
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(100),
        None,
    );
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", None).unwrap();
    assert!(first.started());
    assert!(second.started());
    first.quota(
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(40),
        Some(Timing::DelayMs(10)),
    );
    drop(one); // Incomplete terminal accounting; marker must survive its owner.
    let before = h.bucket("a", QuotaDimension::TokensPerMinute);
    assert_eq!(before.unaccounted_token_calls, 2);
    assert_eq!(before.effective_remaining, None);
    second.quota(
        model("m"),
        QuotaDimension::TokensPerMinute,
        Some(100),
        Some(90),
        Some(Timing::DelayMs(0)),
    );
    let after = h.bucket("a", QuotaDimension::TokensPerMinute);
    assert_eq!(after.capacity, before.capacity);
    assert_eq!(after.unaccounted_token_calls, 2);
    assert_eq!(after.effective_remaining, None);
    assert_eq!(after.reset_in_ms, Some(10));
    second.final_usage(total_usage(10));
    drop(two);
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerMinute)
            .unaccounted_token_calls,
        1
    );
    assert_eq!(
        err(h.reserve("a", "m", Some(1))),
        SchedulerError::RateStateUnavailable
    );
    h.clock.advance(9);
    assert_eq!(
        h.bucket("a", QuotaDimension::TokensPerMinute)
            .unaccounted_token_calls,
        1
    );
    h.clock.advance(1); // Previously accepted factual reset remains valid.
    let reset = h.bucket("a", QuotaDimension::TokensPerMinute);
    assert_eq!(reset.unaccounted_token_calls, 0);
    assert_eq!(reset.effective_remaining, Some(100));
    assert!(h.reserve("a", "m", Some(10)).is_ok());
}

#[test]
fn rate_overlap_old_credential_era_does_not_join_current_isolated_calls() {
    let h = Harness::new();
    let (old_guard, old) = h.reserve("a", "m", None).unwrap();
    assert!(old.started());
    old.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(1),
        None,
    );
    h.telemetry.invalidate_provider_quotas("a");
    for remaining in [4, 9] {
        let (guard, current) = h.reserve("a", "m", None).unwrap();
        assert!(current.context_generation() > old.context_generation());
        assert!(current.started()); // Only an old-era peer remains live.
        current.quota(
            model("m"),
            QuotaDimension::RequestsPerDay,
            Some(100),
            Some(remaining),
            None,
        );
        drop(guard);
        assert_eq!(
            h.bucket("a", QuotaDimension::RequestsPerDay)
                .effective_remaining,
            Some(remaining)
        );
    }
    old.quota(
        model("m"),
        QuotaDimension::RequestsPerDay,
        Some(100),
        Some(0),
        Some(Timing::DelayMs(0)),
    );
    drop(old_guard);
    let current = h.bucket("a", QuotaDimension::RequestsPerDay);
    assert_eq!(current.effective_remaining, Some(9));
    assert_eq!(current.reset_in_ms, None);
}

#[tokio::test]
async fn rate_overlap_later_reset_revokes_early_refill_and_isolated_call_recovers() {
    let clock = FakeClock::new();
    let (s, mut calls) = ordering_scheduler_with_clock(2, clock.clone());
    let (a, _) = start(
        s.clone(),
        ordering_request(TrafficClass::Background, "A"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let first = bounded(calls.recv()).await.unwrap();
    let (b, _) = start(
        s.clone(),
        ordering_request(TrafficClass::ForegroundInteractive, "B"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let second = bounded(calls.recv()).await.unwrap();
    assert_eq!(s.admission_snapshot()[0].active_calls, 2);
    headers(&first, 40, 40, Some(Timing::DelayMs(10))).await;
    first.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(a).await.unwrap().is_ok());
    assert_eq!(s.rate_snapshot()[0].pending_reservations, 1);
    headers(&second, 30, 30, Some(Timing::DelayMs(100))).await;
    second.commands.send(OrderedHeaderCommand::Finish).unwrap();
    assert!(bounded(b).await.unwrap().is_ok());
    let requests = constraint(&s, "a", QuotaDimension::RequestsPerDay);
    let tokens = constraint(&s, "a", QuotaDimension::TokensPerMinute);
    assert_eq!(requests.effective_remaining, Some(30));
    assert_eq!(tokens.effective_remaining, Some(20));
    assert_eq!(requests.consumed, 1);
    assert_eq!(tokens.consumed, 20);
    clock.advance(10);
    // Audited HEAD incorrectly restores capacity 100 here.
    for before in [&requests, &tokens] {
        let after = constraint(&s, "a", before.dimension);
        assert_eq!(after.capacity, before.capacity);
        assert_eq!(after.consumed, before.consumed);
        assert_eq!(after.effective_remaining, before.effective_remaining);
        assert_eq!(after.reset_in_ms, None);
        assert!(matches!(after.external.unwrap().reset, Fact::Unknown));
    }
    clock.advance(90); // Option B does not authorize refill at B's deadline either.
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).effective_remaining,
        Some(20)
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
        Some(30)
    );
    clean(&s);

    let (c, _) = start(
        s.clone(),
        ordering_request(TrafficClass::ForegroundInteractive, "C"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let isolated = bounded(calls.recv()).await.unwrap();
    headers(&isolated, 90, 90, Some(Timing::DelayMs(50))).await;
    isolated
        .commands
        .send(OrderedHeaderCommand::Finish)
        .unwrap();
    assert!(bounded(c).await.unwrap().is_ok());
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).reset_in_ms,
        Some(50)
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).effective_remaining,
        Some(80)
    );
    assert_eq!(
        constraint(&s, "a", QuotaDimension::RequestsPerDay).effective_remaining,
        Some(90)
    );
    clock.advance(49);
    assert_eq!(
        constraint(&s, "a", QuotaDimension::TokensPerMinute).effective_remaining,
        Some(80)
    );
    clock.advance(1);
    for dim in [
        QuotaDimension::RequestsPerDay,
        QuotaDimension::TokensPerMinute,
    ] {
        let reset = constraint(&s, "a", dim);
        assert_eq!(reset.effective_remaining, Some(100));
        assert_eq!(reset.consumed, 0);
        assert_eq!(reset.reset_in_ms, None);
    }
    clean(&s);
}

#[test]
fn rate_ambiguous_later_delay_or_unix_reset_preserves_uncertainty_before_refresh() {
    for reset in [Timing::DelayMs(100), Timing::UnixMs(1_100)] {
        for arrival in [0, 10, 11] {
            let h = Harness::new();
            let dim = QuotaDimension::TokensPerMinute;
            h.quota("a", model("m"), dim, Some(100), Some(100), None);
            let (one, first) = h.reserve("a", "m", None).unwrap();
            let (two, second) = h.reserve("a", "m", None).unwrap();
            assert!(first.started());
            assert!(second.started());
            first.quota(
                model("m"),
                dim,
                Some(100),
                Some(40),
                Some(Timing::DelayMs(10)),
            );
            drop(one);
            h.clock.advance(arrival); // No refresh between clock advance and incoming fact.
            second.quota(model("m"), dim, Some(100), Some(30), Some(reset));
            let restricted = h.bucket("a", dim);
            assert_eq!(restricted.capacity, Some(30));
            assert_eq!(restricted.consumed, 0);
            assert_eq!(restricted.unaccounted_token_calls, 2);
            assert_eq!(restricted.effective_remaining, None);
            assert_eq!(restricted.reset_in_ms, None);
            assert!(matches!(restricted.external.unwrap().reset, Fact::Unknown));
            h.clock.advance(100);
            assert_eq!(h.bucket("a", dim).unaccounted_token_calls, 2);
            assert_eq!(
                err(h.reserve("a", "m", Some(1))),
                SchedulerError::RateStateUnavailable
            );
            second.final_usage(total_usage(10));
            drop(two);
            let finished = h.bucket("a", dim);
            assert_eq!(finished.unaccounted_token_calls, 1); // Only the owned marker clears.
            assert_eq!(finished.consumed, 10); // No epoch change or double debit.
            assert_eq!(finished.effective_remaining, None);
        }
    }
}

#[test]
fn rate_ambiguous_smaller_or_equal_reset_never_accelerates_accepted_deadline() {
    for reset in [
        Timing::DelayMs(0),
        Timing::DelayMs(5),
        Timing::DelayMs(10),
        Timing::UnixMs(1_005),
        Timing::UnixMs(1_010),
    ] {
        let h = Harness::new();
        let dim = QuotaDimension::RequestsPerDay;
        h.quota("a", model("m"), dim, Some(100), Some(100), None);
        let (one, first) = h.reserve("a", "m", None).unwrap();
        let (two, second) = h.reserve("a", "m", None).unwrap();
        assert!(first.started());
        assert!(second.started());
        first.quota(
            model("m"),
            dim,
            Some(100),
            Some(40),
            Some(Timing::DelayMs(10)),
        );
        drop(one);
        second.quota(model("m"), dim, Some(100), Some(30), Some(reset));
        drop(two);
        assert_eq!(h.bucket("a", dim).reset_in_ms, Some(10));
        assert_eq!(h.bucket("a", dim).effective_remaining, Some(30));
        h.clock.advance(9);
        assert_eq!(h.bucket("a", dim).effective_remaining, Some(30));
        h.clock.advance(1);
        assert_eq!(h.bucket("a", dim).effective_remaining, Some(100));
    }
}

#[test]
fn rate_ambiguous_partial_headers_never_invent_or_reparse_a_reset() {
    for accepted in [None, Some(Timing::DelayMs(10))] {
        let h = Harness::new();
        let dim = QuotaDimension::RequestsPerDay;
        h.quota("a", model("m"), dim, Some(100), Some(100), None);
        let (one, first) = h.reserve("a", "m", None).unwrap();
        let (two, second) = h.reserve("a", "m", None).unwrap();
        assert!(first.started());
        assert!(second.started());
        first.quota(model("m"), dim, Some(100), Some(40), accepted);
        drop(one);
        h.clock.advance(2);
        second.quota(model("m"), dim, None, Some(30), None);
        assert_eq!(h.bucket("a", dim).reset_in_ms, accepted.map(|_| 8));
        second.quota(model("m"), dim, None, None, None);
        assert_eq!(h.bucket("a", dim).reset_in_ms, accepted.map(|_| 8));
        if accepted.is_none() {
            second.quota(model("m"), dim, None, None, Some(Timing::DelayMs(100)));
            assert_eq!(h.bucket("a", dim).reset_in_ms, None);
        }
        drop(two);
        h.clock.advance(100);
        assert_eq!(
            h.bucket("a", dim).effective_remaining,
            Some(if accepted.is_some() { 100 } else { 30 })
        );
    }
}

#[test]
fn rate_ambiguous_duplicate_relative_reset_cannot_extend_or_restore_deadline() {
    let h = Harness::new();
    let dim = QuotaDimension::RequestsPerDay;
    h.quota("a", model("m"), dim, Some(100), Some(100), None);
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", None).unwrap();
    assert!(first.started());
    assert!(second.started());
    first.quota(
        model("m"),
        dim,
        Some(100),
        Some(40),
        Some(Timing::DelayMs(10)),
    );
    drop(one);
    second.quota(
        model("m"),
        dim,
        Some(100),
        Some(30),
        Some(Timing::DelayMs(10)),
    );
    assert_eq!(h.bucket("a", dim).reset_in_ms, Some(10));
    for _ in 0..12 {
        h.clock.advance(1);
        // Same attempt, exact same fact: interpreted later, it is no longer
        // evidence for the earlier deadline. Revoke once, never re-arm/extend.
        second.quota(
            model("m"),
            dim,
            Some(100),
            Some(30),
            Some(Timing::DelayMs(10)),
        );
        let snapshot = h.bucket("a", dim);
        assert_eq!(snapshot.reset_in_ms, None);
        assert_eq!(snapshot.reset_unix_ms, None);
        assert_eq!(snapshot.effective_remaining, Some(30));
        assert_eq!(snapshot.consumed, 1);
    }
    second.quota(
        model("m"),
        dim,
        Some(100),
        Some(30),
        Some(Timing::DelayMs(0)),
    );
    assert_eq!(h.bucket("a", dim).reset_in_ms, None);
    drop(two);
    h.clock.advance(100);
    assert_eq!(h.bucket("a", dim).effective_remaining, Some(30));
}

#[test]
fn rate_independent_fact_restores_revoked_temporal_authority_during_overlap() {
    let h = Harness::new();
    let dim = QuotaDimension::RequestsPerDay;
    h.quota("a", model("m"), dim, Some(100), Some(100), None);
    let (one, first) = h.reserve("a", "m", None).unwrap();
    let (two, second) = h.reserve("a", "m", None).unwrap();
    assert!(first.started());
    assert!(second.started());
    first.quota(
        model("m"),
        dim,
        Some(100),
        Some(40),
        Some(Timing::DelayMs(10)),
    );
    drop(one);
    second.quota(
        model("m"),
        dim,
        Some(100),
        Some(30),
        Some(Timing::DelayMs(100)),
    );
    assert_eq!(h.bucket("a", dim).reset_in_ms, None);
    h.quota(
        "a",
        model("m"),
        dim,
        Some(100),
        Some(60),
        Some(Timing::DelayMs(30)),
    );
    assert_eq!(h.bucket("a", dim).reset_in_ms, Some(30));
    assert_eq!(h.bucket("a", dim).effective_remaining, Some(59)); // Live B's request retained.
    drop(two);
    h.clock.advance(29);
    assert_eq!(h.bucket("a", dim).effective_remaining, Some(59));
    h.clock.advance(1);
    assert_eq!(h.bucket("a", dim).effective_remaining, Some(100));
}

#[test]
fn rate_old_credential_reset_cannot_revoke_or_accelerate_current_deadline() {
    let h = Harness::new();
    let dim = QuotaDimension::RequestsPerDay;
    let (old_guard, old) = h.reserve("a", "m", None).unwrap();
    assert!(old.started());
    old.quota(
        model("m"),
        dim,
        Some(100),
        Some(40),
        Some(Timing::DelayMs(10)),
    );
    h.telemetry.invalidate_provider_quotas("a");
    let (guard, current) = h.reserve("a", "m", None).unwrap();
    assert!(current.started());
    current.quota(
        model("m"),
        dim,
        Some(100),
        Some(9),
        Some(Timing::DelayMs(20)),
    );
    drop(guard);
    for reset in [
        Timing::DelayMs(0),
        Timing::DelayMs(100),
        Timing::UnixMs(1_100),
    ] {
        old.quota(model("m"), dim, Some(100), Some(0), Some(reset));
        assert_eq!(h.bucket("a", dim).reset_in_ms, Some(20));
        assert_eq!(h.bucket("a", dim).effective_remaining, Some(9));
    }
    drop(old_guard);
    h.clock.advance(10);
    assert_eq!(h.bucket("a", dim).effective_remaining, Some(9));
    h.clock.advance(10);
    assert_eq!(h.bucket("a", dim).effective_remaining, Some(100));
}

#[test]
fn rate_ambiguous_reset_without_safe_conversion_revokes_auto_refill() {
    for overflow in [false, true] {
        let h = Harness::new();
        if overflow {
            h.clock.0.lock().unwrap().monotonic_ms = u64::MAX - 20;
        }
        let dim = QuotaDimension::RequestsPerDay;
        h.quota("a", model("m"), dim, Some(100), Some(100), None);
        let (one, first) = h.reserve("a", "m", None).unwrap();
        let (two, second) = h.reserve("a", "m", None).unwrap();
        assert!(first.started());
        assert!(second.started());
        first.quota(
            model("m"),
            dim,
            Some(100),
            Some(40),
            Some(Timing::DelayMs(10)),
        );
        drop(one);
        let reset = if overflow {
            Timing::DelayMs(100)
        } else {
            h.clock.wall(None);
            Timing::UnixMs(1_100)
        };
        second.quota(model("m"), dim, Some(100), Some(30), Some(reset));
        assert_eq!(h.bucket("a", dim).reset_in_ms, None);
        drop(two);
        h.clock.advance(10);
        assert_eq!(h.bucket("a", dim).effective_remaining, Some(30));
    }
}
