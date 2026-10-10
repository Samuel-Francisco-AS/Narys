//! Differential oracle frozen from the authorized LR-9A storage (d11d5834).
//! Recounts independently; never trusts the implementation's class counters.
use super::*;
#[derive(Default)]
struct Reference {
    events: VecDeque<Arc<OperationalEvent>>,
    stats: TraceStats,
}
impl Reference {
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
fn assert_counters(store: &Storage) {
    let mut counts = [0; 3];
    let mut sizes = [0; 3];
    for e in &store.events {
        let i = e.retention_class() as usize;
        counts[i] += 1;
        sizes[i] += e.estimated_bytes();
    }
    assert_eq!(store.retained_count, counts);
    assert_eq!(store.retained_class_bytes, sizes);
    assert_eq!(store.stats.retained_bytes, sizes.iter().sum::<usize>());
    assert!(
        store.events.len() <= MAX_RETAINED_EVENTS
            && store.stats.retained_bytes <= MAX_RETAINED_BYTES
    );
}
#[test]
fn lr9e_incremental_counters_and_frozen_oracle_preserve_exact_retention_replay_and_losses() {
    for pattern in 0..4 {
        let bus = OperationalTraceBus::isolated();
        let mut reference = Reference::default();
        let mut seed = 0x9e_u64;
        for sequence in 1..=6000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let class = if pattern < 3 {
                pattern
            } else {
                (seed >> 32) as usize % 3
            };
            let text = "x".repeat((seed as usize % 8193).min(MAX_TEXT_BYTES));
            let mut draft = super::super::tests::stream("oracle", &text);
            let provenance = OperationalEvent::observed(0, 0, draft.clone())
                .provenance()
                .clone();
            if class == 1 {
                draft = EventDraft::new(
                    provenance,
                    OperationalKind::State {
                        kind: StateKind::Checkpoint,
                        code: TraceId::new("state").unwrap(),
                        detail: TraceText::new(&text).unwrap(),
                    },
                )
                .unwrap();
            } else if class == 2 {
                draft = EventDraft::new(
                    provenance,
                    OperationalKind::Critical {
                        kind: CriticalKind::Failed,
                        code: TraceId::new("critical").unwrap(),
                        message: TraceText::new(&text).unwrap(),
                    },
                )
                .unwrap();
            }
            let event = Arc::new(OperationalEvent::observed(sequence, 0, draft.clone()));
            reference.stats.latest_sequence = sequence;
            reference.stats.published += 1;
            let retained = reference.retain_event(event);
            assert_eq!(
                bus.publish(draft).unwrap(),
                PublishReceipt { sequence, retained }
            );
            let store = bus.0.lock().unwrap();
            assert_counters(&store);
            assert_eq!(store.stats.evicted, reference.stats.evicted);
            assert_eq!(store.stats.dropped, reference.stats.dropped);
            assert_eq!(
                store.stats.highest_lost_sequence,
                reference.stats.highest_lost_sequence
            );
            assert_eq!(store.stats.retained_bytes, reference.stats.retained_bytes);
            assert_eq!(store.events.len(), reference.events.len());
            for (actual, expected) in store.events.iter().zip(&reference.events) {
                assert_eq!(actual.sequence(), expected.sequence());
                assert_eq!(actual.kind(), expected.kind());
                assert_eq!(actual.provenance(), expected.provenance());
            }
            drop(store);
            if sequence % 37 == 0 {
                let mut cursor = 0;
                let mut replay = vec![];
                loop {
                    let b = bus.replay(cursor, BatchLimits::default()).unwrap();
                    assert_eq!(
                        b.replay_complete,
                        reference.stats.highest_lost_sequence <= cursor
                    );
                    cursor = b.next_after;
                    replay.extend(b.events.into_iter().map(|e| e.sequence()));
                    if !b.has_more {
                        break;
                    }
                }
                assert_eq!(cursor, sequence);
                assert_eq!(
                    replay,
                    reference
                        .events
                        .iter()
                        .map(|e| e.sequence())
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
