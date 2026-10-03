//! Local runtime capacity only. No quota, routing, provider brands or request content.
use super::{telemetry::MAX_FACT_VALUE, types::SchedulerError};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficClass {
    Background,
    ForegroundTask,
    ForegroundInteractive,
}

/// These are Luna's local limits, never claims about a provider's commercial capacity.
#[derive(Clone, Copy, Debug)]
pub struct AdmissionConfig {
    pub max_concurrency_per_provider: usize,
    pub queue_capacity_per_provider: usize,
    pub queue_timeout_ms: u64,
    pub max_priority_bypasses: u32,
}
impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            max_concurrency_per_provider: 2,
            queue_capacity_per_provider: 64,
            queue_timeout_ms: 60_000,
            max_priority_bypasses: 8,
        }
    }
}
impl AdmissionConfig {
    fn valid(self) -> bool {
        self.max_concurrency_per_provider > 0
            && self.max_concurrency_per_provider as u128 <= MAX_FACT_VALUE as u128
            && self.queue_capacity_per_provider as u128 <= MAX_FACT_VALUE as u128
            && self.queue_timeout_ms > 0
            && self.queue_timeout_ms <= MAX_FACT_VALUE
    }
}
// Existing cancellation contract is AtomicBool, without a wake handle. Bound detection
// latency just as Scheduler backoff does; release/queue changes wake immediately.
const CANCELLATION_POLL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionSnapshot {
    pub provider_id: String,
    pub max_concurrency: usize,
    pub active_calls: usize,
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub queued_by_class: BTreeMap<TrafficClass, usize>,
    pub total_admissions: u64,
    pub total_waited: u64,
    pub queue_delay_total_ms: u64,
    pub queue_delay_samples: u64,
    pub queue_delay_recent_ms: Option<u64>,
    pub queue_full_count: u64,
    pub queue_timeout_count: u64,
    pub counters_saturated: bool,
}
struct Ticket {
    id: Arc<()>,
    class: TrafficClass,
    bypasses: u32,
    entered: Instant,
}
#[derive(Default)]
struct State {
    active: usize,
    queue: VecDeque<Ticket>,
    admissions: u64,
    waited: u64,
    delay_total: u64,
    delay_samples: u64,
    delay_recent: Option<u64>,
    full: u64,
    timeouts: u64,
    saturated: bool,
}
fn add(counter: &mut u64, amount: u64) -> bool {
    let sum = counter.saturating_add(amount);
    *counter = sum.min(MAX_FACT_VALUE);
    sum > MAX_FACT_VALUE
}
fn millis(elapsed: Duration) -> u64 {
    elapsed.as_millis().min(MAX_FACT_VALUE as u128) as u64
}
impl State {
    /// Queue order is arrival order. The oldest protected ticket wins; otherwise
    /// highest class wins, with the oldest ticket breaking ties. Older peers in a
    /// class have at least as many bypasses, so protection never breaks class FIFO.
    fn winner(&self, limit: u32) -> Option<usize> {
        self.queue
            .iter()
            .position(|t| t.bypasses >= limit)
            .or_else(|| {
                self.queue
                    .iter()
                    .enumerate()
                    .max_by(|(ai, a), (bi, b)| a.class.cmp(&b.class).then_with(|| bi.cmp(ai)))
                    .map(|(i, _)| i)
            })
    }
    fn admit(&mut self) {
        self.active += 1;
        self.saturated |= add(&mut self.admissions, 1);
    }
}
struct ProviderState {
    config: AdmissionConfig,
    state: Mutex<State>,
    changed: Notify,
}
pub struct AdmissionController {
    providers: BTreeMap<String, Arc<ProviderState>>,
}
pub struct AdmissionPermit {
    provider: Arc<ProviderState>,
    pub queue_delay_ms: u64,
}
impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        {
            let mut state = self
                .provider
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            // Each permit is owned once and cannot be cloned.
            state.active -= 1;
        }
        self.provider.changed.notify_waiters();
    }
}
/// Also removes the ticket if the waiting future is dropped/aborted or a sink fails.
struct WaitingTicket {
    provider: Arc<ProviderState>,
    id: Arc<()>,
}
impl Drop for WaitingTicket {
    fn drop(&mut self) {
        {
            let mut state = self
                .provider
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            state.queue.retain(|t| !Arc::ptr_eq(&t.id, &self.id));
        }
        self.provider.changed.notify_waiters();
    }
}
impl AdmissionController {
    pub fn new(
        ids: impl IntoIterator<Item = String>,
        config: AdmissionConfig,
    ) -> Result<Self, &'static str> {
        if !config.valid() {
            return Err("admission_config_invalid");
        }
        Ok(Self {
            providers: ids
                .into_iter()
                .map(|id| {
                    (
                        id,
                        Arc::new(ProviderState {
                            config,
                            state: Mutex::new(State::default()),
                            changed: Notify::new(),
                        }),
                    )
                })
                .collect(),
        })
    }
    pub fn snapshots(&self) -> Vec<AdmissionSnapshot> {
        self.providers
            .iter()
            .map(|(id, provider)| {
                let s = provider.state.lock().unwrap_or_else(|p| p.into_inner());
                let mut classes = BTreeMap::from([
                    (TrafficClass::Background, 0),
                    (TrafficClass::ForegroundTask, 0),
                    (TrafficClass::ForegroundInteractive, 0),
                ]);
                for t in &s.queue {
                    *classes.get_mut(&t.class).unwrap() += 1;
                }
                AdmissionSnapshot {
                    provider_id: id.clone(),
                    max_concurrency: provider.config.max_concurrency_per_provider,
                    active_calls: s.active,
                    queue_depth: s.queue.len(),
                    queue_capacity: provider.config.queue_capacity_per_provider,
                    queued_by_class: classes,
                    total_admissions: s.admissions,
                    total_waited: s.waited,
                    queue_delay_total_ms: s.delay_total,
                    queue_delay_samples: s.delay_samples,
                    queue_delay_recent_ms: s.delay_recent,
                    queue_full_count: s.full,
                    queue_timeout_count: s.timeouts,
                    counters_saturated: s.saturated,
                }
            })
            .collect()
    }
    pub async fn acquire(
        &self,
        id: &str,
        class: TrafficClass,
        cancelled: &AtomicBool,
        on_queued: &mut (dyn FnMut(usize) -> Result<(), SchedulerError> + Send),
    ) -> Result<AdmissionPermit, SchedulerError> {
        let provider = self
            .providers
            .get(id)
            .ok_or(SchedulerError::NoProvider)?
            .clone();
        let ticket_id = Arc::new(());
        let (waiting, depth, entered) = {
            let mut s = provider.state.lock().unwrap_or_else(|p| p.into_inner());
            if cancelled.load(Ordering::Acquire) {
                return Err(SchedulerError::Cancelled);
            }
            // Never let an arriving call jump existing waiters, even with a free slot.
            if s.active < provider.config.max_concurrency_per_provider && s.queue.is_empty() {
                s.admit();
                return Ok(AdmissionPermit {
                    provider: provider.clone(),
                    queue_delay_ms: 0,
                });
            }
            if s.queue.len() >= provider.config.queue_capacity_per_provider {
                s.saturated |= add(&mut s.full, 1);
                return Err(SchedulerError::AdmissionQueueFull);
            }
            let entered = Instant::now();
            s.queue.push_back(Ticket {
                id: ticket_id.clone(),
                class,
                bypasses: 0,
                entered,
            });
            s.saturated |= add(&mut s.waited, 1);
            (
                WaitingTicket {
                    provider: provider.clone(),
                    id: ticket_id.clone(),
                },
                s.queue.len(),
                entered,
            )
        };
        // No user callback, notification or await while holding the state lock.
        on_queued(depth)?;
        provider.changed.notify_waiters();
        loop {
            let changed = provider.changed.notified();
            tokio::pin!(changed);
            // Register before checking state to avoid losing a concurrent release.
            changed.as_mut().enable();
            {
                let mut s = provider.state.lock().unwrap_or_else(|p| p.into_inner());
                if cancelled.load(Ordering::Acquire) {
                    return Err(SchedulerError::Cancelled);
                }
                if entered.elapsed() >= Duration::from_millis(provider.config.queue_timeout_ms) {
                    s.saturated |= add(&mut s.timeouts, 1);
                    return Err(SchedulerError::AdmissionTimeout);
                }
                if s.active < provider.config.max_concurrency_per_provider {
                    if let Some(index) = s.winner(provider.config.max_priority_bypasses) {
                        if Arc::ptr_eq(&s.queue[index].id, &ticket_id) {
                            let ticket = s.queue.remove(index).unwrap();
                            for older in s.queue.iter_mut().take(index) {
                                if older.class < ticket.class {
                                    older.bypasses = older.bypasses.saturating_add(1);
                                }
                            }
                            let delay = millis(ticket.entered.elapsed());
                            s.admit();
                            s.saturated |= add(&mut s.delay_total, delay);
                            s.saturated |= add(&mut s.delay_samples, 1);
                            s.delay_recent = Some(delay);
                            let permit = AdmissionPermit {
                                provider: provider.clone(),
                                queue_delay_ms: delay,
                            };
                            drop(s);
                            drop(waiting);
                            return Ok(permit);
                        }
                    }
                }
            }
            let remaining = Duration::from_millis(provider.config.queue_timeout_ms)
                .saturating_sub(entered.elapsed());
            tokio::select! {
                _ = &mut changed => {},
                _ = tokio::time::sleep(remaining.min(CANCELLATION_POLL)) => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_config_is_rejected_and_zero_queue_or_bypasses_are_valid() {
        for config in [
            AdmissionConfig {
                max_concurrency_per_provider: 0,
                ..AdmissionConfig::default()
            },
            AdmissionConfig {
                queue_timeout_ms: 0,
                ..AdmissionConfig::default()
            },
            AdmissionConfig {
                queue_timeout_ms: MAX_FACT_VALUE + 1,
                ..AdmissionConfig::default()
            },
        ] {
            assert!(AdmissionController::new(["a".into()], config).is_err());
        }
        assert!(AdmissionController::new(
            ["a".into()],
            AdmissionConfig {
                queue_capacity_per_provider: 0,
                max_priority_bypasses: 0,
                ..AdmissionConfig::default()
            }
        )
        .is_ok());
    }
    #[test]
    fn counters_saturate_at_json_bound_and_snapshot_counts_classes_coherently() {
        let c = AdmissionController::new(["a".into()], AdmissionConfig::default()).unwrap();
        {
            let mut s = c.providers["a"].state.lock().unwrap();
            s.admissions = MAX_FACT_VALUE;
            s.admit();
            s.delay_total = MAX_FACT_VALUE;
            s.saturated |= add(&mut s.delay_total, u64::MAX);
            for class in [
                TrafficClass::Background,
                TrafficClass::ForegroundTask,
                TrafficClass::ForegroundInteractive,
            ] {
                s.queue.push_back(Ticket {
                    id: Arc::new(()),
                    class,
                    bypasses: 0,
                    entered: Instant::now(),
                });
            }
        }
        let snapshot = &c.snapshots()[0];
        assert_eq!(snapshot.total_admissions, MAX_FACT_VALUE);
        assert_eq!(snapshot.queue_delay_total_ms, MAX_FACT_VALUE);
        assert!(snapshot.counters_saturated);
        assert_eq!(snapshot.active_calls, 1);
        assert_eq!(snapshot.queue_depth, 3);
        assert_eq!(snapshot.queued_by_class.values().sum::<usize>(), 3);
        assert!(snapshot.queued_by_class.values().all(|n| *n == 1));
    }
}
