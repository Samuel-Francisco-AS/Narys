use super::contract::*;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex, OnceLock, Weak,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_RETAINED_EVENTS: usize = 1024;
pub const MAX_RETAINED_BYTES: usize = 2 * 1024 * 1024;
pub const CRITICAL_RESERVED_EVENTS: usize = 64;
pub const CRITICAL_RESERVED_BYTES: usize = 256 * 1024;
pub const STATE_RESERVED_EVENTS: usize = 128;
pub const STATE_RESERVED_BYTES: usize = 512 * 1024;
pub const MAX_SUBSCRIBERS: usize = 8;
pub const SUBSCRIBER_QUEUE_EVENTS: usize = 64;
pub const MAX_BATCH_EVENTS: usize = 128;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchLimits {
    events: usize,
    bytes: usize,
}
impl BatchLimits {
    /// The minimum byte budget guarantees that every valid event can advance
    /// a cursor, even when it is the first event in a batch.
    pub fn new(events: usize, bytes: usize) -> Result<Self, TraceError> {
        if events == 0
            || events > MAX_BATCH_EVENTS
            || !(MAX_EVENT_ESTIMATED_BYTES..=MAX_BATCH_BYTES).contains(&bytes)
        {
            return Err(TraceError::InvalidBatchLimits);
        }
        Ok(Self { events, bytes })
    }
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            events: MAX_BATCH_EVENTS,
            bytes: MAX_BATCH_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClassCounts {
    pub stream: u64,
    pub state: u64,
    pub critical: u64,
}
impl ClassCounts {
    fn increment(&mut self, class: RetentionClass) {
        let counter = match class {
            RetentionClass::Stream => &mut self.stream,
            RetentionClass::State => &mut self.state,
            RetentionClass::Critical => &mut self.critical,
        };
        *counter = counter.saturating_add(1);
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TraceStats {
    /// Successfully assigned identities, including history drops.
    pub published: u64,
    pub evicted: ClassCounts,
    pub dropped: ClassCounts,
    /// Delivery attempts (one event can be dropped for multiple observers).
    pub live_delivery_dropped: u64,
    pub subscribers_disconnected: u64,
    pub active_subscribers: usize,
    pub retained_events: usize,
    pub retained_bytes: usize,
    pub latest_sequence: u64,
    pub oldest_retained_sequence: Option<u64>,
    /// Exact suffix completeness without an unbounded list of lost ranges.
    pub highest_lost_sequence: u64,
}
#[derive(Debug)]
pub struct ReplayBatch {
    pub events: Vec<Arc<OperationalEvent>>,
    pub estimated_bytes: usize,
    /// All events in (requested after, latest] still exist in this snapshot.
    /// False means retention loss, not merely pagination.
    pub replay_complete: bool,
    pub has_more: bool,
    /// Last returned sequence if paginated; otherwise latest (also skips holes).
    pub next_after: u64,
    pub stats: TraceStats,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublishReceipt {
    pub sequence: u64,
    pub retained: bool,
}

#[derive(Default)]
struct LiveMetrics {
    dropped: AtomicU64,
    last_dropped_sequence: AtomicU64,
}
struct Observer {
    id: u64,
    sender: SyncSender<Arc<OperationalEvent>>,
    metrics: Arc<LiveMetrics>,
}
#[derive(Default)]
struct Storage {
    events: VecDeque<Arc<OperationalEvent>>,
    observers: Vec<Observer>,
    next_observer_id: u64,
    stats: TraceStats,
}

/// Single production bus, independent of window/presentation/task lifecycles.
/// A bounded internal mutex serializes identity, retention and try_send order.
/// No subscriber code/callback, await, I/O or inference runs under this lock.
pub struct OperationalTraceBus(Mutex<Storage>);
impl OperationalTraceBus {
    pub fn process_wide() -> Arc<Self> {
        static BUS: OnceLock<Arc<OperationalTraceBus>> = OnceLock::new();
        BUS.get_or_init(|| Arc::new(Self::new())).clone()
    }
    // Private: production cannot accidentally create competing sequence domains.
    fn new() -> Self {
        Self(Mutex::new(Storage::default()))
    }
    #[cfg(test)]
    pub(crate) fn isolated() -> Arc<Self> {
        Arc::new(Self::new())
    }

    #[cfg(test)]
    pub(crate) fn seed_test_sequence(&self, sequence: u64) {
        self.0.lock().unwrap().stats.latest_sequence = sequence;
    }

    pub fn publish(&self, draft: EventDraft) -> Result<PublishReceipt, TraceError> {
        let mut store = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let sequence = store
            .stats
            .latest_sequence
            .checked_add(1)
            .ok_or(TraceError::SequenceExhausted)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let event = Arc::new(OperationalEvent::observed(sequence, timestamp, draft));
        store.stats.latest_sequence = sequence;
        store.stats.published = store.stats.published.saturating_add(1);
        let retained = store.retain_event(event.clone());
        let mut dropped = 0_u64;
        let mut disconnected = 0_u64;
        store
            .observers
            .retain(|observer| match observer.sender.try_send(event.clone()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    dropped = dropped.saturating_add(1);
                    let _ = observer.metrics.dropped.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |n| Some(n.saturating_add(1)),
                    );
                    observer
                        .metrics
                        .last_dropped_sequence
                        .store(sequence, Ordering::Release);
                    true
                }
                Err(TrySendError::Disconnected(_)) => {
                    dropped = dropped.saturating_add(1);
                    disconnected = disconnected.saturating_add(1);
                    false
                }
            });
        store.stats.live_delivery_dropped =
            store.stats.live_delivery_dropped.saturating_add(dropped);
        store.stats.subscribers_disconnected = store
            .stats
            .subscribers_disconnected
            .saturating_add(disconnected);
        Ok(PublishReceipt { sequence, retained })
    }

    /// Starts after the current sequence; caller can replay from an older cursor.
    /// Registration and cursor sampling are atomic with publication.
    pub fn subscribe(self: &Arc<Self>) -> Result<LiveSubscriber, TraceError> {
        let mut store = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if store.observers.len() == MAX_SUBSCRIBERS {
            return Err(TraceError::SubscriberLimit);
        }
        let id = store
            .next_observer_id
            .checked_add(1)
            .ok_or(TraceError::SequenceExhausted)?;
        store.next_observer_id = id;
        let (sender, receiver) = mpsc::sync_channel(SUBSCRIBER_QUEUE_EVENTS);
        let metrics = Arc::new(LiveMetrics::default());
        store.observers.push(Observer {
            id,
            sender,
            metrics: metrics.clone(),
        });
        Ok(LiveSubscriber {
            id,
            bus: Arc::downgrade(self),
            receiver,
            metrics,
            cursor: store.stats.latest_sequence,
            pending: None,
        })
    }

    pub fn stats(&self) -> TraceStats {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .snapshot_stats()
    }

    pub fn replay(&self, after: u64, limits: BatchLimits) -> Result<ReplayBatch, TraceError> {
        let store = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if after > store.stats.latest_sequence {
            return Err(TraceError::FutureCursor);
        }
        let mut events = Vec::new();
        let mut bytes = 0;
        let mut has_more = false;
        for event in store.events.iter().filter(|event| event.sequence() > after) {
            if events.len() == limits.events || bytes + event.estimated_bytes() > limits.bytes {
                has_more = true;
                break;
            }
            bytes += event.estimated_bytes();
            events.push(event.clone());
        }
        let next_after = if has_more {
            events.last().map_or(after, |event| event.sequence())
        } else {
            store.stats.latest_sequence
        };
        Ok(ReplayBatch {
            events,
            estimated_bytes: bytes,
            replay_complete: store.stats.highest_lost_sequence <= after,
            has_more,
            next_after,
            stats: store.snapshot_stats(),
        })
    }
}

impl Storage {
    fn snapshot_stats(&self) -> TraceStats {
        let mut stats = self.stats.clone();
        stats.retained_events = self.events.len();
        stats.oldest_retained_sequence = self.events.front().map(|event| event.sequence());
        stats.active_subscribers = self.observers.len();
        stats
    }
    /// Reservations constrain lower-class occupancy even when higher classes
    /// are absent. CRITICAL can borrow the entire store. STREAM cannot evict
    /// STATE/CRITICAL; STATE cannot evict CRITICAL. FIFO within each victim class.
    fn retain_event(&mut self, event: Arc<OperationalEvent>) -> bool {
        let class = event.retention_class();
        let bytes = event.estimated_bytes();
        let mut counts = [0_usize; 3];
        let mut sizes = [0_usize; 3];
        for retained in &self.events {
            let index = retained.retention_class() as usize;
            counts[index] += 1;
            sizes[index] += retained.estimated_bytes();
        }
        counts[class as usize] += 1;
        sizes[class as usize] += bytes;
        loop {
            let ceiling = if counts[0]
                > MAX_RETAINED_EVENTS - CRITICAL_RESERVED_EVENTS - STATE_RESERVED_EVENTS
                || sizes[0] > MAX_RETAINED_BYTES - CRITICAL_RESERVED_BYTES - STATE_RESERVED_BYTES
            {
                Some(RetentionClass::Stream)
            } else if counts[0] + counts[1] > MAX_RETAINED_EVENTS - CRITICAL_RESERVED_EVENTS
                || sizes[0] + sizes[1] > MAX_RETAINED_BYTES - CRITICAL_RESERVED_BYTES
            {
                Some(RetentionClass::State)
            } else if counts.iter().sum::<usize>() > MAX_RETAINED_EVENTS
                || sizes.iter().sum::<usize>() > MAX_RETAINED_BYTES
            {
                Some(RetentionClass::Critical)
            } else {
                None
            };
            let Some(ceiling) = ceiling else {
                break;
            };
            let victim = [
                RetentionClass::Stream,
                RetentionClass::State,
                RetentionClass::Critical,
            ]
            .into_iter()
            .filter(|candidate| *candidate <= ceiling && *candidate <= class)
            .find_map(|candidate| {
                self.events
                    .iter()
                    .position(|e| e.retention_class() == candidate)
            });
            let Some(index) = victim else {
                self.stats.dropped.increment(class);
                self.stats.highest_lost_sequence =
                    self.stats.highest_lost_sequence.max(event.sequence());
                return false;
            };
            if let Some(victim) = self.events.remove(index) {
                let index = victim.retention_class() as usize;
                counts[index] -= 1;
                sizes[index] -= victim.estimated_bytes();
                self.stats.retained_bytes -= victim.estimated_bytes();
                self.stats.evicted.increment(victim.retention_class());
                self.stats.highest_lost_sequence =
                    self.stats.highest_lost_sequence.max(victim.sequence());
            }
        }
        self.stats.retained_bytes += bytes;
        self.events.push_back(event);
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveStatus {
    pub delivery_dropped: u64,
    pub last_dropped_sequence: u64,
}
#[derive(Debug)]
pub struct LiveBatch {
    pub events: Vec<Arc<OperationalEvent>>,
    pub estimated_bytes: usize,
    /// A discontinuity since the last delivered cursor. Tail losses are also
    /// visible through status even before a subsequent event is delivered.
    pub sequence_gap: bool,
    pub status: LiveStatus,
}
pub struct LiveSubscriber {
    id: u64,
    bus: Weak<OperationalTraceBus>,
    receiver: Receiver<Arc<OperationalEvent>>,
    metrics: Arc<LiveMetrics>,
    cursor: u64,
    // At most one lookahead event when a byte cap splits a live batch.
    pending: Option<Arc<OperationalEvent>>,
}
impl LiveSubscriber {
    /// Consumer-only wait for the first item; publishers remain exclusively try_send.
    pub fn wait(&mut self, timeout: Duration) -> bool {
        if self.pending.is_some() {
            return true;
        }
        self.pending = self.receiver.recv_timeout(timeout).ok();
        self.pending.is_some()
    }
    pub fn cursor(&self) -> u64 {
        self.cursor
    }
    pub fn status(&self) -> LiveStatus {
        LiveStatus {
            delivery_dropped: self.metrics.dropped.load(Ordering::Acquire),
            last_dropped_sequence: self.metrics.last_dropped_sequence.load(Ordering::Acquire),
        }
    }
    pub fn drain_batch(&mut self, limits: BatchLimits) -> LiveBatch {
        let mut events = Vec::new();
        let mut bytes = 0;
        let mut sequence_gap = false;
        while events.len() < limits.events {
            let Some(event) = self
                .pending
                .take()
                .or_else(|| self.receiver.try_recv().ok())
            else {
                break;
            };
            if bytes + event.estimated_bytes() > limits.bytes {
                self.pending = Some(event);
                break;
            }
            sequence_gap |= self.cursor.checked_add(1) != Some(event.sequence());
            self.cursor = event.sequence();
            bytes += event.estimated_bytes();
            events.push(event);
        }
        LiveBatch {
            events,
            estimated_bytes: bytes,
            sequence_gap,
            status: self.status(),
        }
    }
}
impl Drop for LiveSubscriber {
    fn drop(&mut self) {
        if let Some(bus) = self.bus.upgrade() {
            let mut store = bus.0.lock().unwrap_or_else(|p| p.into_inner());
            let before = store.observers.len();
            store.observers.retain(|observer| observer.id != self.id);
            if before != store.observers.len() {
                store.stats.subscribers_disconnected =
                    store.stats.subscribers_disconnected.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn disconnected_receiver_is_removed_without_publisher_error() {
        let bus = OperationalTraceBus::isolated();
        let mut subscriber = bus.subscribe().unwrap();
        // Simulate native receiver disappearance before registration cleanup.
        let (_, replacement) = mpsc::sync_channel(1);
        subscriber.receiver = replacement;
        let receipt = bus
            .publish(super::super::tests::stream("test", "x"))
            .unwrap();
        assert!(receipt.retained);
        let stats = bus.stats();
        assert_eq!(stats.active_subscribers, 0);
        assert_eq!(stats.subscribers_disconnected, 1);
        assert_eq!(stats.live_delivery_dropped, 1);
        drop(subscriber);
        assert_eq!(bus.stats().subscribers_disconnected, 1);
        assert!(bus
            .publish(super::super::tests::stream("test", "still-usable"))
            .is_ok());
    }

    #[test]
    fn cumulative_published_live_and_disconnect_counts_are_saturating() {
        let bus = OperationalTraceBus::isolated();
        let subscriber = bus.subscribe().unwrap();
        {
            let mut store = bus.0.lock().unwrap();
            store.stats.published = u64::MAX;
            store.stats.live_delivery_dropped = u64::MAX;
            store.stats.subscribers_disconnected = u64::MAX;
        }
        for _ in 0..SUBSCRIBER_QUEUE_EVENTS + 1 {
            bus.publish(super::super::tests::stream("test", "x"))
                .unwrap();
        }
        drop(subscriber);
        let stats = bus.stats();
        assert_eq!(stats.published, u64::MAX);
        assert_eq!(stats.live_delivery_dropped, u64::MAX);
        assert_eq!(stats.subscribers_disconnected, u64::MAX);
        assert_eq!(stats.latest_sequence, SUBSCRIBER_QUEUE_EVENTS as u64 + 1);
    }

    #[test]
    fn counters_saturate_and_sequence_exhaustion_is_explicit() {
        let mut counts = ClassCounts {
            stream: u64::MAX,
            state: u64::MAX,
            critical: u64::MAX,
        };
        for class in [
            RetentionClass::Stream,
            RetentionClass::State,
            RetentionClass::Critical,
        ] {
            counts.increment(class);
        }
        assert_eq!(counts.stream, u64::MAX);
        assert_eq!(counts.state, u64::MAX);
        assert_eq!(counts.critical, u64::MAX);
        let bus = OperationalTraceBus::isolated();
        bus.0.lock().unwrap().stats.latest_sequence = u64::MAX;
        assert_eq!(
            bus.publish(super::super::tests::stream("test", "x")),
            Err(TraceError::SequenceExhausted)
        );
    }
}
