//! Shared, provider-agnostic rate authority. Lock order: observation -> telemetry
//! -> rate -> SQLite. Admission never nests any of these locks. No lock crosses await.
use super::{
    telemetry::{
        Fact, Provenance, QuotaDimension, QuotaScope, QuotaSnapshot, Timing, MAX_FACT_VALUE,
    },
    types::SchedulerError,
};
use crate::persistence::database::Database;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Copy, Debug)]
pub struct ClockReading {
    pub monotonic_ms: u64,
    pub unix_ms: Option<u64>,
}
pub trait RateClock: Send + Sync {
    fn now(&self) -> ClockReading;
}
pub struct SystemRateClock(Instant);
impl Default for SystemRateClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl RateClock for SystemRateClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            monotonic_ms: self.0.elapsed().as_millis().min(u64::MAX as u128) as u64,
            unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .filter(|n| *n <= MAX_FACT_VALUE),
        }
    }
}

/// Proof supplied by the caller for this exact invocation, including protocol,
/// input, generated output and any billed reasoning. Never an optimistic estimate.
/// No universal tokenizer, byte heuristic or commercial model table is provided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenUpperBound(u64);
impl TokenUpperBound {
    pub fn explicit_total(tokens: u64) -> Result<Self, SchedulerError> {
        if tokens > MAX_FACT_VALUE {
            Err(SchedulerError::InvalidRatePolicy)
        } else {
            Ok(Self(tokens))
        }
    }
    pub fn total(self) -> u64 {
        self.0
    }
}

/// Fixed window equivalent to a bucket with a full refill only at declared boundaries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedWindow {
    pub period_ms: u64,
    pub anchor_unix_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalRateLimit {
    pub scope: QuotaScope,
    pub dimension: QuotaDimension,
    pub capacity: u64,
    pub window: FixedWindow,
}
/// Local consumption policy, never a statement about a remote subscription.
/// The anchor explicitly declares the UTC daily boundary; no local midnight default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DailyBudgetPolicy {
    pub anchor_unix_ms: u64,
    pub max_requests: Option<u64>,
    pub max_accounted_tokens: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RatePolicy {
    pub limits: Vec<LocalRateLimit>,
    pub daily_budget: Option<DailyBudgetPolicy>,
}
impl RatePolicy {
    fn expanded(&self) -> Result<Vec<(LocalRateLimit, ConstraintSource)>, SchedulerError> {
        if self.limits.len() > 64 {
            return Err(SchedulerError::InvalidRatePolicy);
        }
        let mut result = Vec::new();
        for limit in &self.limits {
            if limit.dimension == QuotaDimension::Concurrency
                || limit.capacity > MAX_FACT_VALUE
                || limit.window.period_ms == 0
                || limit.window.period_ms > MAX_FACT_VALUE
                || limit.window.anchor_unix_ms > MAX_FACT_VALUE
                || matches!(&limit.scope, QuotaScope::Model { model } if model.is_empty() || model.len() > 128 || model.trim() != model || model.chars().any(char::is_control))
                || result
                    .iter()
                    .any(|(other, _): &(LocalRateLimit, ConstraintSource)| {
                        other.scope == limit.scope && other.dimension == limit.dimension
                    })
            {
                return Err(SchedulerError::InvalidRatePolicy);
            }
            result.push((limit.clone(), ConstraintSource::LocalPolicy));
        }
        if let Some(daily) = &self.daily_budget {
            if daily.anchor_unix_ms > MAX_FACT_VALUE {
                return Err(SchedulerError::InvalidRatePolicy);
            }
            for (dimension, capacity) in [
                (QuotaDimension::RequestsPerDay, daily.max_requests),
                (QuotaDimension::TokensPerDay, daily.max_accounted_tokens),
            ] {
                if let Some(capacity) = capacity {
                    if capacity > MAX_FACT_VALUE {
                        return Err(SchedulerError::InvalidRatePolicy);
                    }
                    result.push((
                        LocalRateLimit {
                            scope: QuotaScope::Provider,
                            dimension,
                            capacity,
                            window: FixedWindow {
                                period_ms: DAY_MS,
                                anchor_unix_ms: daily.anchor_unix_ms,
                            },
                        },
                        ConstraintSource::DailyBudget,
                    ));
                }
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintSource {
    ExternalFact,
    LocalPolicy,
    DailyBudget,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateConstraintSnapshot {
    pub scope: QuotaScope,
    pub dimension: QuotaDimension,
    pub source: ConstraintSource,
    pub provenance: Option<Provenance>,
    pub external: Option<QuotaSnapshot>,
    pub capacity: Option<u64>,
    pub consumed: u64,
    pub reserved: u64,
    pub effective_remaining: Option<u64>,
    pub reset_unix_ms: Option<u64>,
    pub reset_in_ms: Option<u64>,
    pub saturated: bool,
    /// Unresolved token calls, including conservative recovery markers after crash.
    pub unaccounted_token_calls: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateSnapshot {
    pub provider_id: String,
    pub captured_at_unix_ms: Option<u64>,
    pub context_generation: u64,
    pub policy: RatePolicy,
    pub constraints: Vec<RateConstraintSnapshot>,
    pub pending_reservations: usize,
    pub local_blocks: u64,
    pub saturated: bool,
    pub persistence_failed: bool,
}

/// Local transport authorization order, never reservation/admission arrival order
/// and never a claim about provider-side processing or response completion order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HttpStartSequence(u64);

#[derive(Clone)]
struct Bucket {
    scope: QuotaScope,
    dimension: QuotaDimension,
    source: ConstraintSource,
    external: Option<QuotaSnapshot>,
    capacity: Option<u64>,
    consumed: u64,
    epoch: u64,
    deadline: Option<u64>,
    end_unix: Option<u64>,
    window: Option<FixedWindow>,
    saturated: bool,
    unaccounted: u64,
    observation_floor: Option<HttpStartSequence>,
}
#[derive(Clone)]
struct Charge {
    bucket: usize,
    epoch: u64,
    amount: u64,
    /// Owns one unresolved-call marker in this bucket/epoch, set before HTTP.
    uncertain: bool,
}
#[derive(Clone)]
struct Attempt {
    generation: u64,
    model: String,
    bound: Option<TokenUpperBound>,
    started: bool,
    http_start: Option<HttpStartSequence>,
    /// Earliest authorized start in this transitive overlap group. Retained
    /// until this attempt finishes, even after all its original peers finish.
    overlap_start: Option<HttpStartSequence>,
    total_usage: Option<u64>,
    usage_final: bool,
    charges: Vec<Charge>,
}
#[derive(Clone, Default)]
struct State {
    generation: u64,
    policy: RatePolicy,
    buckets: Vec<Bucket>,
    attempts: BTreeMap<u64, Attempt>,
    next_attempt: u64,
    next_http_start: u64,
    blocks: u64,
    saturated: bool,
    persistence_failed: bool,
}
fn add(to: &mut u64, amount: u64, saturated: &mut bool) {
    *saturated |= to.checked_add(amount).is_none_or(|n| n > MAX_FACT_VALUE);
    *to = to.saturating_add(amount).min(MAX_FACT_VALUE);
}
fn tokens(dim: QuotaDimension) -> bool {
    matches!(
        dim,
        QuotaDimension::TokensPerMinute | QuotaDimension::TokensPerDay
    )
}
fn applies(scope: &QuotaScope, model: &str) -> bool {
    matches!(scope, QuotaScope::Provider)
        || matches!(scope, QuotaScope::Model { model: m } if m == model)
}
fn number(fact: &Fact<u64>) -> Option<u64> {
    match fact {
        Fact::Known { value, .. } => Some(*value),
        Fact::Unknown => None,
    }
}
fn deadline(timing: Timing, now: ClockReading) -> Option<(u64, Option<u64>)> {
    let delay = match timing {
        Timing::DelayMs(n) if n <= MAX_FACT_VALUE => n,
        Timing::UnixMs(n) if n <= MAX_FACT_VALUE => n.saturating_sub(now.unix_ms?),
        _ => return None,
    };
    Some((
        now.monotonic_ms.checked_add(delay)?,
        match timing {
            Timing::UnixMs(n) => Some(n),
            Timing::DelayMs(_) => now
                .unix_ms
                .and_then(|n| n.checked_add(delay))
                .filter(|n| *n <= MAX_FACT_VALUE),
        },
    ))
}
fn window_end(window: &FixedWindow, now: u64) -> Option<u64> {
    if now < window.anchor_unix_ms {
        return Some(window.anchor_unix_ms);
    }
    let periods = (now - window.anchor_unix_ms) / window.period_ms;
    window
        .anchor_unix_ms
        .checked_add(periods.checked_add(1)?.checked_mul(window.period_ms)?)
        .filter(|n| *n <= MAX_FACT_VALUE)
}
impl State {
    fn reserved(&self, bucket: usize) -> u64 {
        self.attempts
            .values()
            .filter(|a| !a.started)
            .flat_map(|a| &a.charges)
            .filter(|c| c.bucket == bucket && c.epoch == self.buckets[bucket].epoch)
            .fold(0_u64, |sum, c| {
                sum.saturating_add(c.amount).min(MAX_FACT_VALUE)
            })
    }
    fn refresh(&mut self, now: ClockReading) -> Result<(), SchedulerError> {
        for i in 0..self.buckets.len() {
            let b = &mut self.buckets[i];
            if !b.deadline.is_some_and(|at| now.monotonic_ms >= at) {
                continue;
            }
            let epoch = b
                .epoch
                .checked_add(1)
                .ok_or(SchedulerError::RateStateUnavailable)?;
            if let Some(window) = &b.window {
                let periods = (now.monotonic_ms - b.deadline.unwrap()) / window.period_ms + 1;
                let elapsed = periods
                    .checked_mul(window.period_ms)
                    .ok_or(SchedulerError::RateStateUnavailable)?;
                let next_deadline = b.deadline.and_then(|at| at.checked_add(elapsed));
                let next_end = b
                    .end_unix
                    .and_then(|at| at.checked_add(elapsed))
                    .filter(|at| *at <= MAX_FACT_VALUE);
                if next_deadline.is_none() || next_end.is_none() {
                    return Err(SchedulerError::RateStateUnavailable);
                }
                b.deadline = next_deadline;
                b.end_unix = next_end;
            } else {
                b.capacity = b.external.as_ref().and_then(|q| number(&q.limit));
                b.deadline = None; // One observed reset, never a guessed periodic refill.
                b.end_unix = None;
            }
            b.epoch = epoch;
            b.consumed = 0;
            b.unaccounted = 0;
            b.saturated = false;
            for a in self.attempts.values_mut().filter(|a| !a.started) {
                for c in a.charges.iter_mut().filter(|c| c.bucket == i) {
                    c.epoch = b.epoch;
                }
            }
        }
        Ok(())
    }
}

/// Only local windows/policy are serializable for persistence. Remote facts,
/// generation, attempts, prompt/output content and telemetry never enter SQLite.
/// Model identifiers are persisted only when explicitly part of local policy.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedBucket {
    index: usize,
    end_unix: u64,
    debited: u64,
    saturated: bool,
    unaccounted: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Persisted {
    policy: RatePolicy,
    windows: Vec<PersistedBucket>,
}

pub struct RateLimitManager {
    states: Mutex<BTreeMap<String, State>>,
    clock: Arc<dyn RateClock>,
    database: Option<Database>,
}
impl RateLimitManager {
    pub fn new(
        ids: impl IntoIterator<Item = String>,
        clock: Arc<dyn RateClock>,
        database: Option<Database>,
    ) -> Result<Arc<Self>, SchedulerError> {
        let manager = Arc::new(Self {
            states: Mutex::new(ids.into_iter().map(|id| (id, State::default())).collect()),
            clock,
            database,
        });
        if let Some(db) = &manager.database {
            let conn = db
                .open()
                .map_err(|_| SchedulerError::RateStateUnavailable)?;
            let mut stmt = conn
                .prepare("SELECT provider_id,local_state FROM cognitive_rate_state")
                .map_err(|_| SchedulerError::RateStateUnavailable)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|_| SchedulerError::RateStateUnavailable)?;
            let mut states = manager.states.lock().unwrap_or_else(|p| p.into_inner());
            for row in rows {
                let (id, json) = row.map_err(|_| SchedulerError::RateStateUnavailable)?;
                let Some(s) = states.get_mut(&id) else {
                    continue;
                };
                let saved: Persisted = serde_json::from_str(&json)
                    .map_err(|_| SchedulerError::RateStateUnavailable)?;
                manager.install_policy(s, saved.policy)?;
                let now = manager.clock.now();
                if saved.windows.len() != s.buckets.len() {
                    return Err(SchedulerError::RateStateUnavailable);
                }
                for (index, saved) in saved.windows.into_iter().enumerate() {
                    let b = &mut s.buckets[index];
                    if saved.index != index
                        || saved.end_unix > MAX_FACT_VALUE
                        || saved.debited > MAX_FACT_VALUE
                        || saved.unaccounted > MAX_FACT_VALUE
                    {
                        return Err(SchedulerError::RateStateUnavailable);
                    }
                    // Wall time only maps persisted UTC deadline at process startup.
                    // A backward adjustment never erases consumption early.
                    if now.unix_ms.ok_or(SchedulerError::RateStateUnavailable)? < saved.end_unix {
                        b.consumed = saved.debited;
                        b.saturated = saved.saturated;
                        b.unaccounted = saved.unaccounted;
                        let (at, end) = deadline(Timing::UnixMs(saved.end_unix), now)
                            .ok_or(SchedulerError::RateStateUnavailable)?;
                        b.deadline = Some(at);
                        b.end_unix = end;
                    }
                }
            }
        }
        Ok(manager)
    }
    fn install_policy(&self, s: &mut State, policy: RatePolicy) -> Result<(), SchedulerError> {
        let expanded = policy.expanded()?;
        let now = self.clock.now();
        let mut buckets = Vec::new();
        for (limit, source) in expanded {
            let end = window_end(
                &limit.window,
                now.unix_ms.ok_or(SchedulerError::RateStateUnavailable)?,
            )
            .ok_or(SchedulerError::InvalidRatePolicy)?;
            let (at, _) =
                deadline(Timing::UnixMs(end), now).ok_or(SchedulerError::InvalidRatePolicy)?;
            let mut bucket = Bucket {
                scope: limit.scope,
                dimension: limit.dimension,
                source,
                external: None,
                capacity: Some(limit.capacity),
                consumed: 0,
                epoch: 0,
                deadline: Some(at),
                end_unix: Some(end),
                window: Some(limit.window),
                saturated: false,
                unaccounted: 0,
                observation_floor: None,
            };
            if let Some(old) = s.buckets.iter().find(|b| {
                b.source == source
                    && b.scope == bucket.scope
                    && b.dimension == bucket.dimension
                    && b.window == bucket.window
            }) {
                bucket.consumed = old.consumed;
                bucket.deadline = old.deadline;
                bucket.end_unix = old.end_unix;
                bucket.saturated = old.saturated;
                bucket.unaccounted = old.unaccounted;
            }
            buckets.push(bucket);
        }
        buckets.extend(
            s.buckets
                .iter()
                .filter(|b| b.source == ConstraintSource::ExternalFact)
                .cloned(),
        );
        s.policy = policy;
        s.buckets = buckets;
        Ok(())
    }
    fn persist(&self, id: &str, s: &State) -> Result<(), SchedulerError> {
        let Some(db) = &self.database else {
            return Ok(());
        };
        if s.policy.limits.is_empty() && s.policy.daily_budget.is_none() {
            return Ok(());
        }
        let windows = s
            .buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.source != ConstraintSource::ExternalFact)
            .map(|(i, b)| PersistedBucket {
                index: i,
                end_unix: b.end_unix.unwrap_or(0),
                debited: b.consumed.saturating_add(s.reserved(i)).min(MAX_FACT_VALUE),
                saturated: b.saturated,
                // Recovery markers for bounded in-flight calls protect even a
                // violated bound followed by storage failure. They need not hide
                // headroom in this process: its live bound remains enforceable.
                // On restart no live owner can reconcile them, so they become
                // uncertainty until a legitimate window reset.
                unaccounted: b
                    .unaccounted
                    .saturating_add(
                        s.attempts
                            .values()
                            .filter(|a| {
                                a.started
                                    && a.bound.is_some()
                                    && !a.usage_final
                                    && a.charges
                                        .iter()
                                        .any(|c| c.bucket == i && c.epoch == b.epoch)
                            })
                            .count() as u64,
                    )
                    .min(MAX_FACT_VALUE),
            })
            .collect();
        let json = serde_json::to_string(&Persisted {
            policy: s.policy.clone(),
            windows,
        })
        .map_err(|_| SchedulerError::RateStateUnavailable)?;
        let conn = db
            .open()
            .map_err(|_| SchedulerError::RateStateUnavailable)?;
        conn.execute("INSERT INTO cognitive_rate_state(provider_id,local_state) VALUES (?1,?2) ON CONFLICT(provider_id) DO UPDATE SET local_state=excluded.local_state", rusqlite::params![id, json]).map_err(|_| SchedulerError::RateStateUnavailable)?;
        Ok(())
    }
    /// Trusted Core configuration API. Production managers require durable storage
    /// for local policies; ephemeral test/DEV managers deliberately omit a database.
    pub fn set_policy(&self, id: &str, policy: RatePolicy) -> Result<(), SchedulerError> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let s = states.get_mut(id).ok_or(SchedulerError::NoProvider)?;
        if !s.attempts.is_empty() {
            return Err(SchedulerError::RatePolicyBusy);
        }
        let mut replacement = s.clone();
        replacement.refresh(self.clock.now())?;
        self.install_policy(&mut replacement, policy)?;
        // Explicit removal is a policy change, also persisted (not a restart bypass).
        if replacement.policy == RatePolicy::default() {
            if let Some(db) = &self.database {
                db.open()
                    .map_err(|_| SchedulerError::RateStateUnavailable)?
                    .execute(
                        "DELETE FROM cognitive_rate_state WHERE provider_id=?1",
                        [id],
                    )
                    .map_err(|_| SchedulerError::RateStateUnavailable)?;
            }
        } else {
            self.persist(id, &replacement)?;
        }
        replacement.persistence_failed = false;
        *s = replacement;
        Ok(())
    }
    /// Called under TelemetryStore's lock; old in-flight headers are rejected there.
    pub(super) fn invalidate(&self, id: &str, generation: u64) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = states.get_mut(id) {
            s.generation = generation;
            // Keep indices stable for local reservations. External entries become unknown.
            for b in s
                .buckets
                .iter_mut()
                .filter(|b| b.source == ConstraintSource::ExternalFact)
            {
                b.external = Some(QuotaSnapshot::default());
                b.capacity = None;
                b.consumed = 0;
                b.deadline = None;
                b.end_unix = None;
                if let Some(epoch) = b.epoch.checked_add(1) {
                    b.epoch = epoch;
                } else {
                    s.persistence_failed = true;
                }
                b.saturated = false;
                b.unaccounted = 0;
                b.observation_floor = None;
            }
        }
    }
    pub(super) fn observe_external(
        &self,
        id: &str,
        generation: u64,
        scope: QuotaScope,
        dimension: QuotaDimension,
        fact: QuotaSnapshot,
        attempt_id: Option<u64>,
    ) {
        if dimension == QuotaDimension::Concurrency {
            return;
        }
        let factual = |source| {
            matches!(
                source,
                Provenance::ProviderHeader | Provenance::ProviderResponse
            )
        };
        if [&fact.limit, &fact.remaining]
            .iter()
            .any(|value| matches!(value, Fact::Known { provenance, .. } if !factual(*provenance)))
            || matches!(&fact.reset, Fact::Known { provenance, .. } if !factual(*provenance))
        {
            return; // Local policy has its own contract; telemetry is not configuration.
        }
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let Some(s) = states.get_mut(id).filter(|s| s.generation == generation) else {
            return;
        };
        // Attempt ID is only a lookup key / charge owner. Its numeric order has
        // no relationship to HTTP start, since admission may reorder priorities.
        let (http_start, overlap_start) = match attempt_id {
            Some(n) => match s.attempts.get(&n).filter(|a| a.http_start.is_some()) {
                Some(a) => (a.http_start, a.overlap_start),
                None => return, // No live, authorized transport to correlate.
            },
            None => (None, None), // Trusted normalized fact independent of an invocation.
        };
        let now = self.clock.now();
        let existing = s.buckets.iter().position(|b| {
            b.source == ConstraintSource::ExternalFact
                && b.scope == scope
                && b.dimension == dimension
        });
        let tightening_only = existing.is_some_and(|i| {
            http_start
                .zip(s.buckets[i].observation_floor)
                .is_some_and(|(incoming, floor)| incoming <= floor)
                || overlap_start
                    .zip(s.buckets[i].observation_floor)
                    .is_some_and(|(group_start, floor)| group_start <= floor)
        });
        if tightening_only {
            let b = &mut s.buckets[existing.unwrap()];
            if let (Some(accepted), Fact::Known { value, .. }) = (b.deadline, &fact.reset) {
                if deadline(*value, now).is_none_or(|(incoming, _)| incoming > accepted) {
                    // Ambiguous ordering cannot prove when this window ends.
                    // Revoke automatic refill rather than extending/replaying a
                    // relative delay. Smaller/equal resets cannot accelerate it;
                    // absent resets do not invent it. Only fresh authority can
                    // restore a revoked deadline, including after quiescence.
                    b.deadline = None;
                    b.end_unix = None;
                    if let Some(retained) = &mut b.external {
                        retained.reset = Fact::Unknown;
                    }
                }
            }
        }
        // Restrict temporal authority BEFORE refresh: even if the old deadline
        // has elapsed, this incoming fact must not trigger an unsafe refill.
        if s.refresh(now).is_err() {
            s.persistence_failed = true;
            return;
        }
        let i = match existing {
            Some(i) => i,
            None => {
                s.buckets.push(Bucket {
                    scope,
                    dimension,
                    source: ConstraintSource::ExternalFact,
                    external: None,
                    capacity: None,
                    consumed: 0,
                    epoch: 0,
                    deadline: None,
                    end_unix: None,
                    window: None,
                    saturated: false,
                    unaccounted: 0,
                    observation_floor: None,
                });
                s.buckets.len() - 1
            }
        };
        if tightening_only {
            // Start order is not remote processing order. An earlier-started
            // response may expose a genuinely tighter shared external balance.
            // It can only tighten existing credit, in place, never refill, clear
            // uncertainty, replay reset, or rebase/double-debit live charges.
            // Once this scope has an observation within an overlap group (or
            // an independent barrier during it), every member is tightening-only.
            // Peer removal cannot make a later-started member fresh again.
            // A later proven refund must not lift credit above this ceiling:
            // only the non-refundable portion of consumption offsets it.
            let epoch = s.buckets[i].epoch;
            let refundable = if tokens(dimension) {
                s.attempts
                    .values()
                    .filter(|a| a.started)
                    .flat_map(|a| {
                        a.charges.iter().filter_map(move |c| {
                            (c.bucket == i && c.epoch == epoch)
                                .then_some(c.amount.saturating_sub(a.total_usage.unwrap_or(0)))
                        })
                    })
                    .fold(0u64, u64::saturating_add)
            } else {
                0
            };
            let b = &mut s.buckets[i];
            if let (Some(old), Some(ceiling)) =
                (b.capacity, number(&fact.remaining).or(number(&fact.limit)))
            {
                let tightened = old.min(
                    b.consumed
                        .saturating_sub(refundable)
                        .saturating_add(ceiling),
                );
                if tightened < old {
                    b.capacity = Some(tightened);
                    // Retain the provenance of the field actually tightening the
                    // ceiling. Temporal authority was restricted above without
                    // changing the observation floor or accounting epoch.
                    if let Some(retained) = &mut b.external {
                        if number(&fact.remaining).is_some() {
                            retained.remaining = fact.remaining;
                            if number(&retained.limit)
                                .zip(number(&retained.remaining))
                                .is_some_and(|(l, r)| r > l)
                            {
                                retained.limit = Fact::Unknown;
                            }
                        } else {
                            retained.limit = fact.limit;
                            if number(&retained.limit)
                                .zip(number(&retained.remaining))
                                .is_some_and(|(l, r)| r > l)
                            {
                                retained.remaining = Fact::Unknown;
                            }
                        }
                    }
                }
            }
            return;
        }
        let fresh_remaining = number(&fact.remaining);
        let fresh_limit = number(&fact.limit);
        let mut capacity = fresh_remaining.or(fresh_limit);
        let previous = &s.buckets[i];
        // Missing remaining is no evidence of refill. In particular, a partial
        // or absent header must not resurrect a previously exhausted ceiling.
        if fresh_remaining.is_none() {
            if let Some(old) = previous.capacity {
                capacity = Some(
                    fresh_limit.map_or(old.saturating_sub(previous.consumed), |limit| {
                        limit.min(old.saturating_sub(previous.consumed))
                    }),
                );
            }
            if fresh_limit.is_none() && matches!(fact.reset, Fact::Unknown) {
                return;
            }
        }
        // The first observation in an overlap group may establish its baseline,
        // but cannot increase prior known credit, even if its peers have ended.
        // A new isolated invocation may replace it. All live charges are retained.
        if overlap_start.is_some() {
            if let Some(old) = s.buckets[i].capacity {
                capacity = capacity.map(|new| new.min(old.saturating_sub(s.buckets[i].consumed)));
            }
        }
        let Some(epoch) = s.buckets[i].epoch.checked_add(1) else {
            s.persistence_failed = true;
            return;
        };
        let live_uncertainty = s
            .attempts
            .values()
            .flat_map(|a| &a.charges)
            .filter(|c| c.bucket == i && c.epoch == s.buckets[i].epoch && c.uncertain)
            .count() as u64;
        let b = &mut s.buckets[i];
        b.capacity = capacity;
        if fresh_remaining.is_some() && overlap_start.is_none() {
            // Fresh remaining replaces completed uncertainty, never tokens
            // still being generated after the headers of an active call.
            b.unaccounted = 0;
            b.saturated = false;
        } else if !b.saturated {
            b.unaccounted = b.unaccounted.saturating_sub(live_uncertainty);
        }
        // Preserve the provenance of the last usable field when a later partial
        // observation cannot replace that field. Timing is never reparsed/refilled
        // from an old delay merely because another field was observed again.
        let mut retained = fact.clone();
        if let Some(old) = &b.external {
            if matches!(retained.limit, Fact::Unknown) {
                retained.limit = old.limit.clone();
            }
            if matches!(retained.remaining, Fact::Unknown) {
                retained.remaining = old.remaining.clone();
            }
        }
        if number(&retained.limit)
            .zip(number(&retained.remaining))
            .is_some_and(|(limit, remaining)| remaining > limit)
        {
            // A fresh partial field supersedes an incompatible historical field.
            if fresh_remaining.is_some() {
                retained.limit = Fact::Unknown;
            } else {
                retained.remaining = Fact::Unknown;
            }
        }
        b.external = Some(retained);
        b.consumed = 0;
        b.epoch = epoch;
        // Independent facts establish a barrier for already-started invocations;
        // their late headers cannot erase this observation's authority either.
        b.observation_floor = Some(http_start.unwrap_or(HttpStartSequence(s.next_http_start)));
        let reset = match fact.reset {
            Fact::Known { value, .. } => deadline(value, now),
            Fact::Unknown => None,
        };
        b.deadline = reset.map(|r| r.0);
        b.end_unix = reset.and_then(|r| r.1);
        for (n, a) in s
            .attempts
            .iter_mut()
            .filter(|(_, a)| a.generation == generation && applies(&b.scope, &a.model))
        {
            // Remaining observed in the HTTP response includes this started
            // request. Tokens can still be generated after response headers.
            let amount = if tokens(dimension) {
                a.bound
                    .map_or(0, TokenUpperBound::total)
                    .max(a.total_usage.unwrap_or(0))
            } else if a.started && attempt_id == Some(*n) && fresh_remaining.is_some() {
                0
            } else {
                1
            };
            a.charges.retain(|c| c.bucket != i);
            a.charges.push(Charge {
                bucket: i,
                epoch: b.epoch,
                amount,
                uncertain: a.started && tokens(dimension) && a.bound.is_none() && !a.usage_final,
            });
            if a.started {
                add(&mut b.consumed, amount, &mut b.saturated);
                if tokens(dimension) && a.bound.is_none() && !a.usage_final {
                    add(&mut b.unaccounted, 1, &mut b.saturated);
                }
            }
        }
    }
    pub fn reserve(
        self: &Arc<Self>,
        id: &str,
        model: &str,
        generation: u64,
        bound: Option<TokenUpperBound>,
        cancelled: &AtomicBool,
    ) -> Result<RateReservation, SchedulerError> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if cancelled.load(Ordering::Acquire) {
            return Err(SchedulerError::Cancelled);
        }
        let s = states.get_mut(id).ok_or(SchedulerError::NoProvider)?;
        if s.persistence_failed {
            return Err(SchedulerError::RateStateUnavailable);
        }
        if s.generation != generation || generation == MAX_FACT_VALUE {
            return Err(SchedulerError::RateContextChanged);
        }
        s.refresh(self.clock.now())?;
        let mut charges = Vec::new();
        for (i, b) in s
            .buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| applies(&b.scope, model))
        {
            let amount = if tokens(b.dimension) {
                bound.map_or(0, TokenUpperBound::total)
            } else {
                1
            };
            if let Some(capacity) = b.capacity {
                if tokens(b.dimension) && bound.is_some() && b.unaccounted > 0 {
                    add(&mut s.blocks, 1, &mut s.saturated);
                    return Err(SchedulerError::RateStateUnavailable);
                }
                if (!tokens(b.dimension) || bound.is_some())
                    && amount
                        > capacity
                            .saturating_sub(b.consumed)
                            .saturating_sub(s.reserved(i))
                {
                    let error = if b.source == ConstraintSource::DailyBudget {
                        SchedulerError::DailyBudgetExceeded
                    } else {
                        SchedulerError::RateCapacityExceeded
                    };
                    add(&mut s.blocks, 1, &mut s.saturated);
                    return Err(error);
                }
            }
            charges.push(Charge {
                bucket: i,
                epoch: b.epoch,
                amount,
                uncertain: false,
            });
        }
        s.next_attempt = s
            .next_attempt
            .checked_add(1)
            .ok_or(SchedulerError::RateStateUnavailable)?;
        let n = s.next_attempt;
        s.attempts.insert(
            n,
            Attempt {
                generation,
                model: model.into(),
                bound,
                started: false,
                http_start: None,
                overlap_start: None,
                total_usage: None,
                usage_final: false,
                charges,
            },
        );
        // Write-ahead conservative debit: process death cannot erase a local
        // reservation. A graceful pre-HTTP rollback removes it durably.
        if let Err(e) = self.persist(id, s) {
            s.attempts.remove(&n);
            s.persistence_failed = true;
            return Err(e);
        }
        Ok(RateReservation {
            handle: RateAttemptHandle {
                manager: self.clone(),
                provider: id.into(),
                id: n,
            },
        })
    }
    fn started(
        &self,
        id: &str,
        n: u64,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), SchedulerError> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(SchedulerError::Cancelled);
        }
        let s = states.get_mut(id).ok_or(SchedulerError::NoProvider)?;
        if s.persistence_failed {
            return Err(SchedulerError::RateStateUnavailable);
        }
        s.refresh(self.clock.now())?;
        let before_commit = s.clone();
        let a = s
            .attempts
            .get_mut(&n)
            .ok_or(SchedulerError::RateStateUnavailable)?;
        if a.generation != s.generation || a.generation == MAX_FACT_VALUE {
            return Err(SchedulerError::RateContextChanged);
        }
        if a.started {
            return Ok(());
        }
        let sequence = s
            .next_http_start
            .checked_add(1)
            .ok_or(SchedulerError::RateStateUnavailable)?;
        // Revalidate pending reservation after quota observations/reset during queue.
        for c in &a.charges {
            let b = &s.buckets[c.bucket];
            if c.epoch == b.epoch
                && tokens(b.dimension)
                && a.bound.is_some()
                && b.capacity.is_some()
                && b.unaccounted > 0
            {
                add(&mut s.blocks, 1, &mut s.saturated);
                return Err(SchedulerError::RateStateUnavailable);
            }
            if c.epoch == b.epoch
                && b.capacity.is_some_and(|cap| {
                    (!tokens(b.dimension) || a.bound.is_some())
                        && b.consumed.saturating_add(c.amount) > cap
                })
            {
                add(&mut s.blocks, 1, &mut s.saturated);
                return Err(if b.source == ConstraintSource::DailyBudget {
                    SchedulerError::DailyBudgetExceeded
                } else {
                    SchedulerError::RateCapacityExceeded
                });
            }
        }
        a.started = true;
        for c in &mut a.charges {
            let b = &mut s.buckets[c.bucket];
            if c.epoch == b.epoch {
                add(&mut b.consumed, c.amount, &mut b.saturated);
                if tokens(b.dimension) && a.bound.is_none() {
                    add(&mut b.unaccounted, 1, &mut b.saturated);
                    c.uncertain = true;
                }
            }
        }
        // A queued reservation may have crossed a boundary. Persist its debit in
        // the actual HTTP window before permitting transport to start.
        let committed = self.persist(id, s).and_then(|()| {
            // Persistence is still local work. Cancellation that won during it
            // must not become a factual request or permit transport to start.
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                Err(SchedulerError::Cancelled)
            } else {
                Ok(())
            }
        });
        if let Err(e) = committed {
            // Restore the exact pre-commit state (including saturation/markers).
            // Disk may retain a conservative marker when cancellation won after
            // the write; graceful guard Drop durably removes that false positive.
            *s = before_commit;
            if e == SchedulerError::RateStateUnavailable {
                s.persistence_failed = true;
            }
            return Err(e);
        }
        // Publish order only after durable accounting and the final local
        // cancellation check authorized HTTP. No lock has been released here.
        s.next_http_start = sequence;
        s.attempts.get_mut(&n).unwrap().http_start = Some(HttpStartSequence(sequence));
        // Actual authorized starts, never pending reservations, create overlap.
        // Reuse historical group origin to carry transitive overlap across peer
        // Drop. Publish only after successful accounting/cancellation checks.
        let overlap_start = s
            .attempts
            .iter()
            .filter(|(peer, a)| **peer != n && a.started && a.generation == s.generation)
            .filter_map(|(_, a)| a.overlap_start.or(a.http_start))
            .min();
        if let Some(group_start) = overlap_start {
            for a in s
                .attempts
                .values_mut()
                .filter(|a| a.started && a.generation == s.generation)
            {
                a.overlap_start = Some(group_start);
            }
        }
        Ok(())
    }
    fn usage(&self, id: &str, n: u64, total: u64, final_sample: bool) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = states.get_mut(id) {
            let Some(a) = s.attempts.get_mut(&n).filter(|a| a.started) else {
                return;
            };
            if a.total_usage.is_some_and(|old| total < old) {
                return;
            }
            let newly_final = final_sample && !a.usage_final;
            a.usage_final |= final_sample;
            a.total_usage = Some(total);
            let mut changed = newly_final;
            let mut cleared = Vec::new();
            // A violated bound or previously unbounded measured usage must debit
            // immediately, before another task can reserve known spent capacity.
            // Refund remains deferred until the owning guard finishes.
            for c in &mut a.charges {
                let b = &mut s.buckets[c.bucket];
                if c.epoch == b.epoch && tokens(b.dimension) {
                    if total > c.amount {
                        add(&mut b.consumed, total - c.amount, &mut b.saturated);
                        c.amount = total;
                        changed = true;
                    }
                    if final_sample && c.uncertain && !b.saturated {
                        b.unaccounted = b.unaccounted.saturating_sub(1);
                        c.uncertain = false;
                        cleared.push(c.bucket);
                        changed = true;
                    }
                }
            }
            if changed && self.persist(id, s).is_err() {
                // Keep the larger factual debit in RAM. Restoring markers also
                // keeps RAM conservative if definitive reconciliation failed.
                for i in cleared {
                    let b = &mut s.buckets[i];
                    add(&mut b.unaccounted, 1, &mut b.saturated);
                    s.attempts
                        .get_mut(&n)
                        .unwrap()
                        .charges
                        .iter_mut()
                        .find(|c| c.bucket == i)
                        .unwrap()
                        .uncertain = true;
                }
                s.persistence_failed = true;
            }
        }
    }
    fn finish(&self, id: &str, n: u64) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let Some(s) = states.get_mut(id) else {
            return;
        };
        if s.refresh(self.clock.now()).is_err() {
            s.persistence_failed = true;
        }
        let Some(a) = s.attempts.remove(&n) else {
            return;
        };
        if a.started {
            for c in a.charges {
                let b = &mut s.buckets[c.bucket];
                if c.epoch != b.epoch {
                    continue;
                } // No credit into a later window/context.
                if tokens(b.dimension) {
                    if let Some(measured) = a.total_usage {
                        // A cumulative prefix is factual consumption but does not
                        // prove unused capacity. Only a definitive total can refund.
                        let actual = if a.usage_final {
                            measured
                        } else {
                            measured.max(c.amount)
                        };
                        // Checked/saturating accounting handles a violated upper-bound
                        // contract without wrap, and records its saturation explicitly.
                        if actual > c.amount {
                            add(&mut b.consumed, actual - c.amount, &mut b.saturated);
                        } else if !b.saturated {
                            b.consumed = b.consumed.saturating_sub(c.amount - actual);
                        }
                    }
                    // Unresolved markers were committed before transport and
                    // survive removal of their live owner. Never debit twice.
                }
            }
        }
        if self.persist(id, s).is_err() {
            s.persistence_failed = true;
        }
    }
    pub fn snapshots(&self) -> Vec<RateSnapshot> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let now = self.clock.now();
        states
            .iter_mut()
            .map(|(id, s)| {
                if s.refresh(now).is_err() {
                    s.persistence_failed = true;
                }
                RateSnapshot {
                    provider_id: id.clone(),
                    captured_at_unix_ms: now.unix_ms,
                    context_generation: s.generation,
                    policy: s.policy.clone(),
                    constraints: s
                        .buckets
                        .iter()
                        .enumerate()
                        .map(|(i, b)| {
                            let reserved = s.reserved(i);
                            let incomplete_tokens = tokens(b.dimension) && b.unaccounted > 0;
                            RateConstraintSnapshot {
                                scope: b.scope.clone(),
                                dimension: b.dimension,
                                source: b.source,
                                provenance: if b.source == ConstraintSource::ExternalFact {
                                    b.external.as_ref().and_then(|q| match q.remaining {
                                        Fact::Known { provenance, .. } => Some(provenance),
                                        _ => match q.limit {
                                            Fact::Known { provenance, .. } => Some(provenance),
                                            _ => None,
                                        },
                                    })
                                } else {
                                    Some(Provenance::UserConfiguration)
                                },
                                external: b.external.clone(),
                                capacity: b.capacity,
                                consumed: b.consumed,
                                reserved,
                                effective_remaining: b.capacity.filter(|_| !incomplete_tokens).map(
                                    |cap| cap.saturating_sub(b.consumed).saturating_sub(reserved),
                                ),
                                reset_unix_ms: b.end_unix,
                                reset_in_ms: b
                                    .deadline
                                    .map(|at| at.saturating_sub(now.monotonic_ms)),
                                saturated: b.saturated,
                                unaccounted_token_calls: b.unaccounted,
                            }
                        })
                        .collect(),
                    pending_reservations: s.attempts.len(),
                    local_blocks: s.blocks,
                    saturated: s.saturated,
                    persistence_failed: s.persistence_failed,
                }
            })
            .collect()
    }
}

/// Non-owning observation attachment. Only RateReservation owns rollback/reconcile.
#[derive(Clone)]
pub(super) struct RateAttemptHandle {
    manager: Arc<RateLimitManager>,
    provider: String,
    id: u64,
}
impl RateAttemptHandle {
    pub(super) fn started(&self, cancelled: Option<&AtomicBool>) -> Result<(), SchedulerError> {
        self.manager.started(&self.provider, self.id, cancelled)
    }
    pub(super) fn usage(&self, total: u64) {
        self.manager.usage(&self.provider, self.id, total, false);
    }
    pub(super) fn final_usage(&self, total: u64) {
        self.manager.usage(&self.provider, self.id, total, true);
    }
    pub(super) fn id(&self) -> u64 {
        self.id
    }
}
pub struct RateReservation {
    handle: RateAttemptHandle,
}
impl RateReservation {
    pub(super) fn handle(&self) -> RateAttemptHandle {
        self.handle.clone()
    }
}
impl Drop for RateReservation {
    fn drop(&mut self) {
        self.handle
            .manager
            .finish(&self.handle.provider, self.handle.id);
    }
}
