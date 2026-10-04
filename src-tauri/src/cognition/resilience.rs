//! Transient provider health: one authority, independent of quota and models.
//! Deadlines are monotonic and never persisted. No callback executes under a lock.
use super::{
    rate::RateClock,
    telemetry::MAX_FACT_VALUE,
    types::{ProviderError, RetryPolicy, SchedulerError},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct ResilienceConfig {
    pub failure_threshold: u64,
    pub open_duration_ms: u64,
    pub half_open_max_probes: u64,
    pub max_retry_backoff_ms: u64,
}
impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            open_duration_ms: 30_000,
            half_open_max_probes: 1,
            max_retry_backoff_ms: 30_000,
        }
    }
}
impl ResilienceConfig {
    pub fn validate(self) -> Result<Self, &'static str> {
        if self.failure_threshold == 0
            || self.open_duration_ms == 0
            || self.half_open_max_probes == 0
            || [
                self.failure_threshold,
                self.open_duration_ms,
                self.half_open_max_probes,
                self.max_retry_backoff_ms,
            ]
            .iter()
            .any(|v| *v > MAX_FACT_VALUE)
        {
            return Err("resilience_config_invalid");
        }
        Ok(self)
    }
}

/// Choose an inclusive value. The manager also clamps implementations to bounds.
pub trait JitterSource: Send + Sync {
    fn choose(&self, lower: u64, upper: u64) -> u64;
}
/// Runtime-only xorshift PRNG seeded with the existing OS entropy dependency.
/// Entropy failure uses process/instance diversification, never user data.
pub struct RuntimeJitter(AtomicU64);
impl Default for RuntimeJitter {
    fn default() -> Self {
        let mut bytes = [0u8; 8];
        let seed = if getrandom::fill(&mut bytes).is_ok() {
            u64::from_ne_bytes(bytes)
        } else {
            static FALLBACK_SEQUENCE: AtomicU64 = AtomicU64::new(1);
            let sequence = FALLBACK_SEQUENCE
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    Some(n.saturating_add(1))
                })
                .unwrap_or_else(|n| n);
            0x9e3779b97f4a7c15 ^ sequence.rotate_left(23) ^ u64::from(std::process::id())
        };
        Self(AtomicU64::new(if seed == 0 {
            0x9e3779b97f4a7c15
        } else {
            seed
        }))
    }
}
impl JitterSource for RuntimeJitter {
    fn choose(&self, lower: u64, upper: u64) -> u64 {
        // Bounds originate in validated local config, so width cannot overflow.
        let width = upper - lower + 1;
        let rejection = u64::MAX - u64::MAX % width;
        loop {
            let mut old = self.0.load(Ordering::Relaxed);
            let value = loop {
                let mut next = old;
                next ^= next << 13;
                next ^= next >> 7;
                next ^= next << 17;
                match self
                    .0
                    .compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed)
                {
                    Ok(_) => break next,
                    Err(actual) => old = actual,
                }
            };
            if value < rejection {
                return lower + value % width;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionReason {
    FailureThresholdTimeout,
    FailureThresholdUnavailable,
    OpenDurationElapsed,
    ProbeTimeout,
    ProbeUnavailable,
    ProbeSucceeded,
    CredentialContextChanged,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResilienceSnapshot {
    pub provider_id: String,
    pub circuit_state: CircuitState,
    pub consecutive_eligible_failures: u64,
    pub configured_threshold: u64,
    pub open_remaining_ms: u64,
    pub half_open_probes_active: u64,
    pub half_open_max_probes: u64,
    pub cooldown_remaining_ms: u64,
    pub transition_count: u64,
    pub last_transition_reason: Option<TransitionReason>,
    pub breaker_open_count: u64,
    pub half_open_count: u64,
    pub recovery_count: u64,
    pub saturated: bool,
}
#[derive(Clone, Copy)]
struct Deadline {
    at: u64,
    overflow: bool,
}
impl Deadline {
    fn new(now: u64, ms: u64) -> Self {
        Self {
            at: now.saturating_add(ms),
            overflow: now.checked_add(ms).is_none(),
        }
    }
    fn active(self, now: u64) -> bool {
        self.overflow || now < self.at
    }
    fn remaining(self, now: u64) -> u64 {
        if self.overflow {
            MAX_FACT_VALUE
        } else {
            self.at.saturating_sub(now).min(MAX_FACT_VALUE)
        }
    }
}
struct State {
    generation: u64,
    circuit: CircuitState,
    epoch: Arc<()>,
    failures: u64,
    open: Option<Deadline>,
    cooldown: Option<Deadline>,
    probes: u64,
    transitions: u64,
    reason: Option<TransitionReason>,
    opens: u64,
    half_opens: u64,
    recoveries: u64,
    saturated: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            generation: 0,
            circuit: CircuitState::Closed,
            epoch: Arc::new(()),
            failures: 0,
            open: None,
            cooldown: None,
            probes: 0,
            transitions: 0,
            reason: None,
            opens: 0,
            half_opens: 0,
            recoveries: 0,
            saturated: false,
        }
    }
}
fn increment(value: &mut u64, saturated: &mut bool) {
    *saturated |= *value >= MAX_FACT_VALUE;
    *value = value.saturating_add(1).min(MAX_FACT_VALUE);
}
impl State {
    fn transition(&mut self, to: CircuitState, reason: TransitionReason) {
        if self.circuit != to {
            increment(&mut self.transitions, &mut self.saturated);
            match to {
                CircuitState::Open => increment(&mut self.opens, &mut self.saturated),
                CircuitState::HalfOpen => increment(&mut self.half_opens, &mut self.saturated),
                CircuitState::Closed if self.circuit == CircuitState::HalfOpen => {
                    increment(&mut self.recoveries, &mut self.saturated)
                }
                _ => {}
            }
        }
        self.circuit = to;
        self.reason = Some(reason);
        self.epoch = Arc::new(());
        self.probes = 0;
    }
    fn eligible(&self, now: u64, max_probes: u64) -> bool {
        if self.cooldown.is_some_and(|d| d.active(now)) {
            return false;
        }
        match self.circuit {
            CircuitState::Closed => true,
            CircuitState::Open => self.open.is_some_and(|d| !d.active(now)),
            CircuitState::HalfOpen => self.probes < max_probes,
        }
    }
}

pub struct ResilienceManager {
    states: Mutex<BTreeMap<String, State>>,
    config: ResilienceConfig,
    clock: Arc<dyn RateClock>,
    jitter: Arc<dyn JitterSource>,
}
impl ResilienceManager {
    pub fn new(
        ids: impl IntoIterator<Item = String>,
        config: ResilienceConfig,
        clock: Arc<dyn RateClock>,
        jitter: Arc<dyn JitterSource>,
    ) -> Result<Arc<Self>, &'static str> {
        Ok(Arc::new(Self {
            states: Mutex::new(ids.into_iter().map(|id| (id, State::default())).collect()),
            config: config.validate()?,
            clock,
            jitter,
        }))
    }
    /// Read-only gate used after ranking. It neither transitions nor spends probes.
    pub fn eligible(&self, id: &str) -> bool {
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        states.get(id).is_some_and(|s| {
            s.generation < MAX_FACT_VALUE
                && s.eligible(
                    self.clock.now().monotonic_ms,
                    self.config.half_open_max_probes,
                )
        })
    }
    pub(super) fn invalidate(&self, id: &str, generation: u64) {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = states.get_mut(id) {
            s.generation = generation;
            s.transition(
                CircuitState::Closed,
                TransitionReason::CredentialContextChanged,
            );
            s.failures = 0;
            s.open = None;
            s.cooldown = None;
        }
    }
    /// Atomic check, transition and bounded probe ownership, before rate/admission.
    pub(super) fn authorize(
        self: &Arc<Self>,
        id: &str,
        generation: u64,
    ) -> Option<ResiliencePermit> {
        let mut states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let s = states.get_mut(id)?;
        let now = self.clock.now().monotonic_ms;
        if s.generation != generation
            || generation >= MAX_FACT_VALUE
            || !s.eligible(now, self.config.half_open_max_probes)
        {
            return None;
        }
        if s.circuit == CircuitState::Open {
            s.transition(
                CircuitState::HalfOpen,
                TransitionReason::OpenDurationElapsed,
            );
            s.open = None;
        }
        let probe = s.circuit == CircuitState::HalfOpen;
        if probe {
            s.probes += 1;
        }
        Some(ResiliencePermit {
            handle: ResilienceAttemptHandle {
                manager: self.clone(),
                id: id.into(),
                generation,
                epoch: s.epoch.clone(),
                probe,
            },
        })
    }
    pub fn backoff_ms(&self, retry: RetryPolicy, number: u32) -> u64 {
        let base = retry
            .backoff_ms(number)
            .min(self.config.max_retry_backoff_ms);
        if base == 0 {
            return 0;
        }
        let lower = base / 2 + base % 2;
        self.jitter.choose(lower, base).clamp(lower, base)
    }
    pub(super) async fn backoff(
        &self,
        ms: u64,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<(), SchedulerError> {
        // Small waits avoid huge Instant addition, preserve 25ms cancellation
        // polling, and allow injected clocks to advance without physical sleeps.
        let start = self.clock.now().monotonic_ms;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(SchedulerError::Cancelled);
            }
            let elapsed = self.clock.now().monotonic_ms.saturating_sub(start);
            if elapsed >= ms {
                return Ok(());
            }
            tokio::select! {
                _ = self.clock.sleep_ms((ms - elapsed).min(25)) => {},
                _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {},
            }
        }
    }
    pub fn snapshots(&self) -> Vec<ResilienceSnapshot> {
        let states = self.states.lock().unwrap_or_else(|p| p.into_inner());
        let now = self.clock.now().monotonic_ms;
        states
            .iter()
            .map(|(id, s)| ResilienceSnapshot {
                provider_id: id.clone(),
                circuit_state: s.circuit,
                consecutive_eligible_failures: s.failures,
                configured_threshold: self.config.failure_threshold,
                open_remaining_ms: s.open.map_or(0, |d| d.remaining(now)),
                half_open_probes_active: s.probes,
                half_open_max_probes: self.config.half_open_max_probes,
                cooldown_remaining_ms: s.cooldown.map_or(0, |d| d.remaining(now)),
                transition_count: s.transitions,
                last_transition_reason: s.reason,
                breaker_open_count: s.opens,
                half_open_count: s.half_opens,
                recovery_count: s.recoveries,
                saturated: s.saturated,
            })
            .collect()
    }
}

/// Non-owning boundary handle: cloning it cannot retain a probe slot.
#[derive(Clone)]
pub(super) struct ResilienceAttemptHandle {
    manager: Arc<ResilienceManager>,
    id: String,
    generation: u64,
    epoch: Arc<()>,
    probe: bool,
}
impl ResilienceAttemptHandle {
    pub(super) fn revalidate(&self) -> Result<(), SchedulerError> {
        let states = self
            .manager
            .states
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let s = states.get(&self.id).ok_or(SchedulerError::NoProvider)?;
        if s.generation != self.generation {
            return Err(SchedulerError::RateContextChanged);
        }
        if s.cooldown
            .is_some_and(|d| d.active(self.manager.clock.now().monotonic_ms))
            || if self.probe {
                !Arc::ptr_eq(&s.epoch, &self.epoch) || s.circuit != CircuitState::HalfOpen
            } else {
                s.circuit != CircuitState::Closed
            }
        {
            return Err(SchedulerError::NoProvider);
        }
        Ok(())
    }
}
pub(super) struct ResiliencePermit {
    handle: ResilienceAttemptHandle,
}
impl ResiliencePermit {
    pub(super) fn handle(&self) -> ResilienceAttemptHandle {
        self.handle.clone()
    }
    /// Provider result before Core validation. Evidence comes only from LR-8A.
    pub(super) fn finish(self, started: bool, error: Option<&ProviderError>) {
        let h = &self.handle;
        let mut states = h.manager.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = states.get_mut(&h.id) {
            if s.generation == h.generation {
                let now = h.manager.clock.now().monotonic_ms;
                // Operational errors remain distinct from factual retryHint.
                let cooldown = match error {
                    Some(ProviderError::RateLimited { retry_after_ms }) => {
                        Some(retry_after_ms.unwrap_or(3_000).max(1))
                    }
                    Some(ProviderError::Unavailable {
                        retry_after_ms: Some(ms),
                    }) => Some((*ms).max(1)),
                    _ => None,
                };
                if let Some(ms) = cooldown {
                    let d = Deadline::new(now, ms);
                    s.saturated |= d.overflow || ms > MAX_FACT_VALUE;
                    if s.cooldown
                        .map_or(true, |old| !old.overflow && (d.overflow || d.at > old.at))
                    {
                        s.cooldown = Some(d);
                    }
                }
                // A running Closed call can complete after a recovery: its
                // factual outcome then belongs to Closed. It cannot substitute
                // for a HalfOpen probe or recover Open. Probes retain epoch scope.
                if started
                    && (Arc::ptr_eq(&s.epoch, &h.epoch)
                        || (!h.probe && s.circuit == CircuitState::Closed))
                {
                    let failure = match error {
                        Some(ProviderError::Timeout) => Some(false),
                        Some(ProviderError::Unavailable { .. }) => Some(true),
                        _ => None,
                    };
                    if let Some(unavailable) = failure {
                        increment(&mut s.failures, &mut s.saturated);
                        if s.circuit == CircuitState::HalfOpen
                            || s.failures >= h.manager.config.failure_threshold
                        {
                            let reason = match (h.probe, unavailable) {
                                (true, true) => TransitionReason::ProbeUnavailable,
                                (true, false) => TransitionReason::ProbeTimeout,
                                (false, true) => TransitionReason::FailureThresholdUnavailable,
                                (false, false) => TransitionReason::FailureThresholdTimeout,
                            };
                            s.transition(CircuitState::Open, reason);
                            let deadline = Deadline::new(now, h.manager.config.open_duration_ms);
                            s.saturated |= deadline.overflow;
                            s.open = Some(deadline);
                        }
                    } else if error.is_none() {
                        s.failures = 0;
                        if s.circuit == CircuitState::HalfOpen {
                            s.transition(CircuitState::Closed, TransitionReason::ProbeSucceeded);
                        }
                    }
                }
            }
        }
        // Release the lock before Drop releases any still-current probe.
        drop(states);
    }
}
impl Drop for ResiliencePermit {
    fn drop(&mut self) {
        let h = &self.handle;
        if !h.probe {
            return;
        }
        let mut states = h.manager.states.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = states.get_mut(&h.id) {
            if s.generation == h.generation && Arc::ptr_eq(&s.epoch, &h.epoch) {
                s.probes = s.probes.saturating_sub(1);
            }
        }
    }
}
