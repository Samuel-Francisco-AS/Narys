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
        SchedulerError::RateCapacityExceeded
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
