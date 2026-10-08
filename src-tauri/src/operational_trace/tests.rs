use super::*;
use crate::luna::task::TaskId;
use std::sync::{Arc, Barrier};
use std::thread;

fn id(value: &str) -> TraceId {
    TraceId::new(value).unwrap()
}
fn text(value: &str) -> TraceText {
    TraceText::new(value).unwrap()
}
fn provenance(source: &str) -> Provenance {
    Provenance {
        source: TraceSource {
            source_type: SourceType::Worker,
            id: id(source),
            instance: Some(id("synthetic")),
        },
        task_id: None,
        subtask_id: None,
        correlation_id: Some(id("item-1")),
        coalescing_key: Some(id("item-1")),
    }
}
pub(super) fn stream(source: &str, value: &str) -> EventDraft {
    EventDraft::new(
        provenance(source),
        OperationalKind::TextDelta {
            channel: TextChannel::Stdout,
            text: text(value),
        },
    )
    .unwrap()
}
fn critical(source: &str) -> EventDraft {
    EventDraft::new(
        provenance(source),
        OperationalKind::Critical {
            kind: CriticalKind::Completed,
            code: id("synthetic-completed"),
            message: text("completed"),
        },
    )
    .unwrap()
}
fn state(source: &str) -> EventDraft {
    EventDraft::new(
        provenance(source),
        OperationalKind::State {
            kind: StateKind::Checkpoint,
            code: id("synthetic-checkpoint"),
            detail: text("checkpoint"),
        },
    )
    .unwrap()
}
fn snapshot(bus: &OperationalTraceBus) -> ReplayBatch {
    bus.replay(0, BatchLimits::default()).unwrap()
}
fn all_retained(bus: &OperationalTraceBus) -> Vec<Arc<OperationalEvent>> {
    // Test helper is capped by the same global retention limit.
    let mut result = Vec::new();
    let mut cursor = 0;
    loop {
        let batch = bus.replay(cursor, BatchLimits::default()).unwrap();
        result.extend(batch.events);
        assert!(result.len() <= MAX_RETAINED_EVENTS);
        cursor = batch.next_after;
        if !batch.has_more {
            return result;
        }
    }
}
fn bounded(stats: &TraceStats) {
    assert!(stats.retained_events <= MAX_RETAINED_EVENTS);
    assert!(stats.retained_bytes <= MAX_RETAINED_BYTES);
    assert!(stats.active_subscribers <= MAX_SUBSCRIBERS);
}

#[test]
fn identifiers_validate_source_instance_correlation_codes_and_keys() {
    for invalid in ["", "white space", "line\nbreak", "á", "id;command", "id\0"] {
        assert_eq!(TraceId::new(invalid), Err(TraceError::InvalidIdentifier));
    }
    assert!(TraceId::new(&"a".repeat(MAX_IDENTIFIER_BYTES)).is_ok());
    assert_eq!(
        TraceId::new(&"a".repeat(MAX_IDENTIFIER_BYTES + 1)),
        Err(TraceError::InvalidIdentifier)
    );
    for valid in ["worker-1", "task_graph:unit_2", "provider/model.v1"] {
        assert!(TraceId::new(valid).is_ok());
    }
}

#[test]
fn utf8_payload_boundary_is_bytes_and_never_truncated() {
    let at_limit = "🦀".repeat(MAX_TEXT_BYTES / 4);
    assert_eq!(TraceText::new(&at_limit).unwrap().as_str(), at_limit);
    assert_eq!(
        TraceText::new(&(at_limit + "a")),
        Err(TraceError::TextTooLarge)
    );
    assert!(TraceText::new("").is_ok());
}

#[test]
fn task_identity_preserves_core_contract_and_taskless_events_work() {
    let bus = OperationalTraceBus::isolated();
    assert_eq!(bus.publish(stream("human", "output")).unwrap().sequence, 1);
    for invalid in [0, 9_007_199_254_740_992, u64::MAX] {
        let mut p = provenance("worker");
        p.task_id = Some(TaskId(invalid));
        assert_eq!(
            EventDraft::new(p, state_kind()),
            Err(TraceError::InvalidTaskId)
        );
    }
    for valid in [1, 9_007_199_254_740_991] {
        let mut p = provenance("worker");
        p.task_id = Some(TaskId(valid));
        assert!(EventDraft::new(p, state_kind()).is_ok());
    }
}
fn state_kind() -> OperationalKind {
    OperationalKind::State {
        kind: StateKind::Started,
        code: id("started"),
        detail: text("started"),
    }
}

#[test]
fn class_is_derived_from_closed_kind_without_a_priority_flag() {
    let bus = OperationalTraceBus::isolated();
    for draft in [
        stream("worker", "delta"),
        state("worker"),
        critical("worker"),
    ] {
        bus.publish(draft).unwrap();
    }
    let batch = snapshot(&bus);
    assert_eq!(
        batch
            .events
            .iter()
            .map(|e| e.retention_class())
            .collect::<Vec<_>>(),
        [
            RetentionClass::Stream,
            RetentionClass::State,
            RetentionClass::Critical
        ]
    );
}

#[test]
fn publish_without_subscriber_is_ordered_and_timestamped() {
    let bus = OperationalTraceBus::isolated();
    for sequence in 1..=20 {
        assert_eq!(
            bus.publish(stream("worker", "fragment")).unwrap(),
            PublishReceipt {
                sequence,
                retained: true
            }
        );
    }
    let batch = snapshot(&bus);
    assert!(batch.replay_complete && !batch.has_more);
    assert_eq!(batch.next_after, 20);
    assert_eq!(batch.stats.published, 20);
    assert_eq!(batch.stats.active_subscribers, 0);
    assert!(batch.events.iter().all(|e| e.observed_at_unix_ms() > 0));
    assert!(batch
        .events
        .windows(2)
        .all(|w| w[0].sequence() + 1 == w[1].sequence()));
}

#[test]
fn live_registration_starts_at_current_sequence_and_delivers_batches() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(state("worker")).unwrap();
    let mut subscriber = bus.subscribe().unwrap();
    assert_eq!(subscriber.cursor(), 1);
    for _ in 0..10 {
        bus.publish(stream("worker", "delta")).unwrap();
    }
    let batch = subscriber.drain_batch(BatchLimits::new(3, MAX_BATCH_BYTES).unwrap());
    assert_eq!(batch.events.len(), 3);
    assert_eq!(batch.events[0].sequence(), 2);
    assert!(!batch.sequence_gap);
    assert_eq!(batch.status.delivery_dropped, 0);
    assert_eq!(subscriber.cursor(), 4);
    assert_eq!(
        subscriber.drain_batch(BatchLimits::default()).events.len(),
        7
    );
}

#[test]
fn disappearing_subscriber_releases_slot_and_does_not_break_publish() {
    let bus = OperationalTraceBus::isolated();
    let subscriber = bus.subscribe().unwrap();
    drop(subscriber);
    assert_eq!(bus.stats().active_subscribers, 0);
    assert_eq!(bus.stats().subscribers_disconnected, 1);
    assert!(bus.publish(critical("worker")).unwrap().retained);
    let mut replacement = bus.subscribe().unwrap();
    bus.publish(state("worker")).unwrap();
    assert_eq!(
        replacement.drain_batch(BatchLimits::default()).events.len(),
        1
    );
}

#[test]
fn subscriber_count_is_bounded_and_slots_are_reusable() {
    let bus = OperationalTraceBus::isolated();
    let mut subscribers: Vec<_> = (0..MAX_SUBSCRIBERS)
        .map(|_| bus.subscribe().unwrap())
        .collect();
    assert!(matches!(bus.subscribe(), Err(TraceError::SubscriberLimit)));
    subscribers.pop();
    assert!(bus.subscribe().is_ok());
    assert_eq!(bus.stats().active_subscribers, MAX_SUBSCRIBERS - 1);
}

#[test]
fn undrained_subscriber_cannot_backpressure_thread_and_tail_loss_is_visible() {
    let bus = OperationalTraceBus::isolated();
    let mut subscriber = bus.subscribe().unwrap();
    let producer = bus.clone();
    // Joining without ever draining proves completion without consumer progress;
    // no fragile elapsed-time assertion or concurrent receiver is required.
    thread::spawn(move || {
        for _ in 0..4096 {
            producer.publish(stream("slow", "delta")).unwrap();
        }
    })
    .join()
    .unwrap();
    assert_eq!(
        subscriber.status().delivery_dropped,
        4096 - SUBSCRIBER_QUEUE_EVENTS as u64
    );
    assert_eq!(subscriber.status().last_dropped_sequence, 4096);
    assert_eq!(
        bus.stats().live_delivery_dropped,
        subscriber.status().delivery_dropped
    );
    let batch = subscriber.drain_batch(BatchLimits::default());
    assert_eq!(batch.events.len(), SUBSCRIBER_QUEUE_EVENTS);
    assert_eq!(subscriber.cursor(), SUBSCRIBER_QUEUE_EVENTS as u64);
    bus.publish(stream("slow", "after-gap")).unwrap();
    let batch = subscriber.drain_batch(BatchLimits::default());
    assert!(batch.sequence_gap);
    assert_eq!(batch.events[0].sequence(), 4097);
    assert!(
        !bus.replay(SUBSCRIBER_QUEUE_EVENTS as u64, BatchLimits::default())
            .unwrap()
            .replay_complete
    );
    bounded(&bus.stats());
}

#[test]
fn healthy_subscriber_is_independent_of_slow_peer_and_live_loss_can_replay() {
    let bus = OperationalTraceBus::isolated();
    let slow = bus.subscribe().unwrap();
    let mut healthy = bus.subscribe().unwrap();
    for _ in 0..100 {
        bus.publish(state("worker")).unwrap();
        assert_eq!(healthy.drain_batch(BatchLimits::default()).events.len(), 1);
    }
    assert_eq!(
        slow.status().delivery_dropped,
        100 - SUBSCRIBER_QUEUE_EVENTS as u64
    );
    assert_eq!(healthy.status().delivery_dropped, 0);
    let recovered = bus
        .replay(SUBSCRIBER_QUEUE_EVENTS as u64, BatchLimits::default())
        .unwrap();
    assert!(recovered.replay_complete);
    assert_eq!(recovered.events.len(), 100 - SUBSCRIBER_QUEUE_EVENTS);
}

#[test]
fn replay_event_pagination_is_bounded_and_not_confused_with_loss() {
    let bus = OperationalTraceBus::isolated();
    for _ in 0..100 {
        bus.publish(state("worker")).unwrap();
    }
    let limits = BatchLimits::new(7, MAX_BATCH_BYTES).unwrap();
    let first = bus.replay(0, limits).unwrap();
    assert!(first.replay_complete && first.has_more);
    assert_eq!(first.next_after, 7);
    let second = bus.replay(first.next_after, limits).unwrap();
    assert_eq!(second.events[0].sequence(), 8);
    assert_eq!(bus.replay(100, limits).unwrap().events.len(), 0);
    assert!(matches!(
        bus.replay(101, limits),
        Err(TraceError::FutureCursor)
    ));
}

#[test]
fn replay_byte_cap_and_live_lookahead_never_lose_an_event() {
    let bus = OperationalTraceBus::isolated();
    let mut subscriber = bus.subscribe().unwrap();
    let payload = "x".repeat(MAX_TEXT_BYTES);
    for _ in 0..20 {
        bus.publish(stream("worker", &payload)).unwrap();
    }
    let limits = BatchLimits::new(10, MAX_EVENT_ESTIMATED_BYTES).unwrap();
    let batch = bus.replay(0, limits).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert!(batch.has_more && batch.replay_complete);
    assert!(batch.estimated_bytes <= MAX_EVENT_ESTIMATED_BYTES);
    for seq in 1..=20 {
        let live = subscriber.drain_batch(limits);
        assert_eq!(live.events.len(), 1);
        assert_eq!(live.events[0].sequence(), seq);
        assert!(!live.sequence_gap);
        assert!(live.estimated_bytes <= MAX_EVENT_ESTIMATED_BYTES);
    }
    assert!(subscriber.drain_batch(limits).events.is_empty());
}

#[test]
fn batch_limits_reject_oversize_zero_and_nonprogressing_bytes() {
    for (events, bytes) in [
        (0, MAX_BATCH_BYTES),
        (MAX_BATCH_EVENTS + 1, MAX_BATCH_BYTES),
        (1, 0),
        (1, MAX_EVENT_ESTIMATED_BYTES - 1),
        (1, MAX_BATCH_BYTES + 1),
    ] {
        assert_eq!(
            BatchLimits::new(events, bytes),
            Err(TraceError::InvalidBatchLimits)
        );
    }
}

#[test]
fn stream_flood_cannot_evict_critical_and_sparse_replay_reports_internal_holes() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(critical("core")).unwrap();
    for _ in 0..5000 {
        bus.publish(stream("worker", "delta")).unwrap();
    }
    let batch = snapshot(&bus);
    assert_eq!(batch.events[0].sequence(), 1);
    assert_eq!(batch.events[0].retention_class(), RetentionClass::Critical);
    assert!(!batch.replay_complete); // oldest=1 does not mean contiguous history
    assert!(batch.stats.evicted.stream > 0);
    assert_eq!(batch.stats.evicted.critical, 0);
    bounded(&batch.stats);
    let loss = batch.stats.highest_lost_sequence;
    assert!(
        bus.replay(loss, BatchLimits::default())
            .unwrap()
            .replay_complete
    );
}

#[test]
fn state_precedes_stream_and_critical_precedes_state() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(state("core")).unwrap();
    for _ in 0..2000 {
        bus.publish(stream("worker", "delta")).unwrap();
    }
    assert!(all_retained(&bus).iter().any(|e| e.sequence() == 1));
    bus.publish(critical("core")).unwrap();
    for _ in 0..2000 {
        bus.publish(state("worker")).unwrap();
    }
    assert_eq!(bus.stats().evicted.stream, 2000);
    assert!(bus.stats().evicted.state > 0);
    assert_eq!(bus.stats().evicted.critical, 0);
    assert!(all_retained(&bus)
        .iter()
        .any(|e| e.retention_class() == RetentionClass::Critical));
}

#[test]
fn reservations_exist_for_events_and_bytes_before_high_priority_arrives() {
    for payload_bytes in [1, MAX_TEXT_BYTES] {
        let bus = OperationalTraceBus::isolated();
        let payload = "x".repeat(payload_bytes);
        for _ in 0..1500 {
            bus.publish(stream("worker", &payload)).unwrap();
        }
        let stats = bus.stats();
        assert!(
            stats.retained_events
                <= MAX_RETAINED_EVENTS - CRITICAL_RESERVED_EVENTS - STATE_RESERVED_EVENTS
        );
        assert!(
            stats.retained_bytes
                <= MAX_RETAINED_BYTES - CRITICAL_RESERVED_BYTES - STATE_RESERVED_BYTES
        );
        let before = stats.evicted.stream;
        for _ in 0..STATE_RESERVED_EVENTS {
            bus.publish(state("scheduler")).unwrap();
        }
        for _ in 0..CRITICAL_RESERVED_EVENTS {
            bus.publish(critical("core")).unwrap();
        }
        assert_eq!(bus.stats().evicted.stream, before);
        assert_eq!(bus.stats().evicted.state, 0);
        assert_eq!(bus.stats().evicted.critical, 0);
        bounded(&bus.stats());
    }
}

#[test]
fn extreme_critical_overflow_is_bounded_accounted_and_incomplete() {
    let bus = OperationalTraceBus::isolated();
    for _ in 0..3000 {
        bus.publish(critical("core")).unwrap();
    }
    let stats = bus.stats();
    assert_eq!(stats.retained_events, MAX_RETAINED_EVENTS);
    assert_eq!(stats.evicted.critical, 3000 - MAX_RETAINED_EVENTS as u64);
    assert_eq!(
        stats.oldest_retained_sequence,
        Some(3000 - MAX_RETAINED_EVENTS as u64 + 1)
    );
    assert!(!snapshot(&bus).replay_complete);
    let receipt = bus.publish(stream("worker", "delta")).unwrap();
    assert!(!receipt.retained);
    assert_eq!(bus.stats().dropped.stream, 1);
    assert!(!bus.publish(state("scheduler")).unwrap().retained);
    assert_eq!(bus.stats().dropped.state, 1);
    bounded(&bus.stats());
}

#[test]
fn large_critical_events_enforce_byte_cap_alongside_event_cap() {
    let bus = OperationalTraceBus::isolated();
    let kind = OperationalKind::Critical {
        kind: CriticalKind::Failed,
        code: id("failure"),
        message: text(&"x".repeat(MAX_TEXT_BYTES)),
    };
    let draft = EventDraft::new(provenance("core"), kind).unwrap();
    let charge = draft.estimated_bytes();
    for _ in 0..1500 {
        bus.publish(draft.clone()).unwrap();
    }
    let stats = bus.stats();
    assert_eq!(stats.retained_events, MAX_RETAINED_BYTES / charge);
    assert_eq!(stats.retained_bytes, stats.retained_events * charge);
    assert_eq!(stats.evicted.critical, 1500 - stats.retained_events as u64);
    bounded(&stats);
}

#[test]
fn state_byte_reserve_and_stream_drops_do_not_masquerade_as_complete_replay() {
    let bus = OperationalTraceBus::isolated();
    let draft = EventDraft::new(
        provenance("scheduler"),
        OperationalKind::State {
            kind: StateKind::Started,
            code: id("started"),
            detail: text(&"x".repeat(MAX_TEXT_BYTES)),
        },
    )
    .unwrap();
    for _ in 0..1000 {
        bus.publish(draft.clone()).unwrap();
    }
    let before = bus.stats();
    assert!(before.retained_events <= MAX_RETAINED_EVENTS - CRITICAL_RESERVED_EVENTS);
    assert!(before.retained_bytes <= MAX_RETAINED_BYTES - CRITICAL_RESERVED_BYTES);
    let receipt = bus
        .publish(stream("worker", &"x".repeat(MAX_TEXT_BYTES)))
        .unwrap();
    assert!(!receipt.retained);
    let after = bus.stats();
    assert_eq!(after.retained_events, before.retained_events);
    assert_eq!(after.retained_bytes, before.retained_bytes);
    assert_eq!(after.evicted.state, before.evicted.state);
    assert_eq!(after.dropped.stream, 1);
    assert_eq!(after.highest_lost_sequence, receipt.sequence);
    let tail = bus
        .replay(receipt.sequence - 1, BatchLimits::default())
        .unwrap();
    assert!(tail.events.is_empty());
    assert!(!tail.replay_complete && !tail.has_more);
    assert_eq!(tail.next_after, receipt.sequence);
    assert!(
        bus.replay(receipt.sequence, BatchLimits::default())
            .unwrap()
            .replay_complete
    );
    for _ in 0..CRITICAL_RESERVED_EVENTS {
        assert!(bus.publish(critical("core")).unwrap().retained);
    }
    assert_eq!(bus.stats().evicted.state, before.evicted.state);
    bounded(&bus.stats());
}

#[test]
fn byte_accounting_includes_all_provenance_and_payload_strings() {
    let long = "x".repeat(MAX_IDENTIFIER_BYTES);
    let p = Provenance {
        source: TraceSource {
            source_type: SourceType::Core,
            id: id(&long),
            instance: Some(id(&long)),
        },
        task_id: Some(TaskId(1)),
        subtask_id: Some(id(&long)),
        correlation_id: Some(id(&long)),
        coalescing_key: Some(id(&long)),
    };
    let draft = EventDraft::new(
        p,
        OperationalKind::Critical {
            kind: CriticalKind::Failed,
            code: id(&long),
            message: text(&"x".repeat(MAX_TEXT_BYTES)),
        },
    )
    .unwrap();
    assert_eq!(draft.estimated_bytes(), MAX_EVENT_ESTIMATED_BYTES);
    let bus = OperationalTraceBus::isolated();
    bus.publish(draft).unwrap();
    assert_eq!(bus.stats().retained_bytes, MAX_EVENT_ESTIMATED_BYTES);
    assert_eq!(snapshot(&bus).estimated_bytes, MAX_EVENT_ESTIMATED_BYTES);
}

#[test]
fn counters_reconcile_retained_evicted_dropped_and_live_attempts() {
    let bus = OperationalTraceBus::isolated();
    let _subscriber = bus.subscribe().unwrap();
    for _ in 0..2000 {
        bus.publish(stream("worker", "x")).unwrap();
    }
    let stats = bus.stats();
    assert_eq!(stats.published, 2000);
    assert_eq!(stats.latest_sequence, 2000);
    assert_eq!(stats.evicted.stream + stats.retained_events as u64, 2000);
    assert_eq!(stats.dropped, ClassCounts::default());
    assert_eq!(
        stats.live_delivery_dropped,
        2000 - SUBSCRIBER_QUEUE_EVENTS as u64
    );
    assert_eq!(
        stats.retained_bytes,
        stats.retained_events * stream("worker", "x").estimated_bytes()
    );
}

#[test]
fn oversize_is_rejected_before_publication_without_identity_or_silent_truncation() {
    let bus = OperationalTraceBus::isolated();
    assert_eq!(
        TraceText::new(&"x".repeat(MAX_TEXT_BYTES + 1)),
        Err(TraceError::TextTooLarge)
    );
    assert_eq!(bus.stats().published, 0);
    assert_eq!(bus.publish(stream("worker", "ok")).unwrap().sequence, 1);
}

#[test]
fn coalescing_is_exact_utf8_with_identity_ranges_and_originals_unchanged() {
    let bus = OperationalTraceBus::isolated();
    for fragment in ["á\n", "🦀", "  end"] {
        bus.publish(stream("worker", fragment)).unwrap();
    }
    let events = snapshot(&bus).events;
    let result = coalesce_batch(&events).unwrap();
    assert_eq!(result.len(), 1);
    let CoalescedItem::Text(merged) = &result[0] else {
        panic!("expected text")
    };
    assert_eq!(merged.text(), "á\n🦀  end");
    assert_eq!(
        (
            merged.first_sequence,
            merged.last_sequence,
            merged.fragments
        ),
        (1, 3, 3)
    );
    assert_eq!(
        merged.first_observed_at_unix_ms,
        events[0].observed_at_unix_ms()
    );
    assert_eq!(
        merged.last_observed_at_unix_ms,
        events[2].observed_at_unix_ms()
    );
    assert_eq!(snapshot(&bus).events.len(), 3);
}

#[test]
fn coalescing_cannot_cross_any_provenance_dimension_or_channel() {
    let baseline = provenance("worker");
    let mut changes = Vec::new();
    let mut p = baseline.clone();
    p.source.id = id("other");
    changes.push(p);
    let mut p = baseline.clone();
    p.source.source_type = SourceType::SpecialistAgent;
    changes.push(p);
    let mut p = baseline.clone();
    p.source.instance = Some(id("other"));
    changes.push(p);
    let mut p = baseline.clone();
    p.task_id = Some(TaskId(1));
    changes.push(p);
    let mut p = baseline.clone();
    p.subtask_id = Some(id("subtask"));
    changes.push(p);
    let mut p = baseline.clone();
    p.correlation_id = Some(id("other"));
    changes.push(p);
    let mut p = baseline.clone();
    p.coalescing_key = Some(id("other"));
    changes.push(p);
    let mut p = baseline.clone();
    p.coalescing_key = None;
    changes.push(p);
    for p in changes {
        let bus = OperationalTraceBus::isolated();
        bus.publish(stream("worker", "a")).unwrap();
        bus.publish(
            EventDraft::new(
                p,
                OperationalKind::TextDelta {
                    channel: TextChannel::Stdout,
                    text: text("b"),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(coalesce_batch(&snapshot(&bus).events).unwrap().len(), 2);
    }
    let bus = OperationalTraceBus::isolated();
    bus.publish(stream("worker", "a")).unwrap();
    bus.publish(
        EventDraft::new(
            baseline,
            OperationalKind::TextDelta {
                channel: TextChannel::Stderr,
                text: text("b"),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(coalesce_batch(&snapshot(&bus).events).unwrap().len(), 2);
}

#[test]
fn coalescing_does_not_cross_critical_state_or_sequence_gaps() {
    let bus = OperationalTraceBus::isolated();
    for draft in [
        stream("worker", "a"),
        state("worker"),
        stream("worker", "b"),
        critical("worker"),
        stream("worker", "c"),
    ] {
        bus.publish(draft).unwrap();
    }
    let events = snapshot(&bus).events;
    assert_eq!(coalesce_batch(&events).unwrap().len(), 5);
    assert_eq!(
        coalesce_batch(&[events[0].clone(), events[2].clone()])
            .unwrap()
            .len(),
        2
    );
    // Reordered/duplicate input is never concatenated either.
    assert_eq!(
        coalesce_batch(&[events[0].clone(), events[0].clone()])
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn coalescing_caps_fragments_text_and_input_batch() {
    let bus = OperationalTraceBus::isolated();
    for _ in 0..MAX_COALESCED_FRAGMENTS + 1 {
        bus.publish(stream("worker", "x")).unwrap();
    }
    let result = coalesce_batch(&snapshot(&bus).events).unwrap();
    assert_eq!(result.len(), 2);
    let bus = OperationalTraceBus::isolated();
    let fragment = "x".repeat(MAX_TEXT_BYTES);
    for _ in 0..5 {
        bus.publish(stream("worker", &fragment)).unwrap();
    }
    let events = snapshot(&bus).events;
    let result = coalesce_batch(&events).unwrap();
    assert_eq!(result.len(), 2);
    let CoalescedItem::Text(first) = &result[0] else {
        panic!("expected text")
    };
    assert_eq!(first.text().len(), MAX_COALESCED_TEXT_BYTES);
    assert!(coalesce_batch(&vec![events[0].clone(); MAX_BATCH_EVENTS + 1]).is_err());
    assert!(coalesce_batch(&vec![events[0].clone(); MAX_BATCH_EVENTS]).is_err());
}

#[test]
fn synthetic_multi_source_stress_gate() {
    const SOURCES: [(SourceType, &str); 5] = [
        (SourceType::SpecialistAgent, "specialist-a"),
        (SourceType::SpecialistAgent, "specialist-b"),
        (SourceType::Worker, "worker-a"),
        (SourceType::Worker, "worker-b"),
        (SourceType::CognitiveProvider, "provider-fictional"),
    ];
    const STREAMS_PER_SOURCE: usize = 2000;
    const STATE_INTERVAL: usize = 100;
    let bus = OperationalTraceBus::isolated();
    let mut slow = bus.subscribe().unwrap();
    let start = Arc::new(Barrier::new(SOURCES.len()));
    let peak = Arc::new(Barrier::new(SOURCES.len()));
    let handles: Vec<_> = SOURCES
        .into_iter()
        .enumerate()
        .map(|(index, (source_type, source_id))| {
            let bus = bus.clone();
            let start = start.clone();
            let peak = peak.clone();
            thread::spawn(move || {
                let mut p = provenance(source_id);
                p.source.source_type = source_type;
                p.task_id = Some(TaskId(index as u64 + 1));
                p.subtask_id = Some(id(source_id));
                let fragment = format!("fragment:{source_id}:{}á\n", "x".repeat(1024));
                let delta = EventDraft::new(
                    p.clone(),
                    OperationalKind::TextDelta {
                        channel: TextChannel::ProviderText,
                        text: text(&fragment),
                    },
                )
                .unwrap();
                let checkpoint = EventDraft::new(p.clone(), state_kind()).unwrap();
                let done = EventDraft::new(
                    p,
                    OperationalKind::Critical {
                        kind: CriticalKind::Completed,
                        code: id("synthetic-completed"),
                        message: text(source_id),
                    },
                )
                .unwrap();
                let mut receipts = Vec::with_capacity(
                    STREAMS_PER_SOURCE + STREAMS_PER_SOURCE / STATE_INTERVAL + 1,
                );
                start.wait();
                for i in 0..STREAMS_PER_SOURCE {
                    if i == STREAMS_PER_SOURCE / 2 {
                        peak.wait();
                        receipts.push(bus.publish(done.clone()).unwrap().sequence);
                    }
                    receipts.push(bus.publish(delta.clone()).unwrap().sequence);
                    if i % STATE_INTERVAL == 0 {
                        receipts.push(bus.publish(checkpoint.clone()).unwrap().sequence);
                    }
                    bounded(&bus.stats());
                }
                assert!(receipts.windows(2).all(|w| w[0] < w[1]));
                (source_id, fragment, receipts)
            })
        })
        .collect();
    // Subscriber is alive and never drained until EVERY publisher finishes.
    let mut identities = Vec::new();
    let mut fragments = Vec::new();
    for handle in handles {
        let (source, fragment, receipts) = handle.join().unwrap();
        identities.extend(receipts);
        fragments.push((source, fragment));
    }
    identities.sort_unstable();
    let expected = SOURCES.len() * (STREAMS_PER_SOURCE + STREAMS_PER_SOURCE / STATE_INTERVAL + 1);
    assert_eq!(identities.len(), expected);
    assert!(identities.iter().copied().eq(1..=expected as u64));
    let stats = bus.stats();
    assert_eq!(stats.published, expected as u64);
    assert_eq!(
        stats.live_delivery_dropped,
        expected as u64 - SUBSCRIBER_QUEUE_EVENTS as u64
    );
    assert!(stats.evicted.stream > 0);
    assert_eq!(stats.evicted.state, 0);
    assert_eq!(stats.evicted.critical, 0);
    assert_eq!(stats.dropped, ClassCounts::default());
    assert_eq!(
        slow.drain_batch(BatchLimits::default()).events.len(),
        SUBSCRIBER_QUEUE_EVENTS
    );
    let events = all_retained(&bus);
    assert!(events.windows(2).all(|w| w[0].sequence() < w[1].sequence()));
    assert_eq!(
        events
            .iter()
            .filter(|e| e.retention_class() == RetentionClass::Critical)
            .count(),
        SOURCES.len()
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.retention_class() == RetentionClass::State)
            .count(),
        SOURCES.len() * STREAMS_PER_SOURCE / STATE_INTERVAL
    );
    for event in &events {
        let p = event.provenance();
        let index = SOURCES
            .iter()
            .position(|(_, id)| *id == p.source.id.as_str())
            .unwrap();
        assert_eq!(p.source.source_type, SOURCES[index].0);
        assert_eq!(p.task_id, Some(TaskId(index as u64 + 1)));
        assert_eq!(p.subtask_id.as_ref().unwrap().as_str(), SOURCES[index].1);
        match event.kind() {
            OperationalKind::TextDelta { text, .. } => assert_eq!(
                text.as_str(),
                fragments
                    .iter()
                    .find(|(id, _)| *id == SOURCES[index].1)
                    .unwrap()
                    .1
            ),
            OperationalKind::Critical { message, .. } => {
                assert_eq!(message.as_str(), SOURCES[index].1)
            }
            OperationalKind::State { .. } => {}
        }
    }
    assert_eq!(
        stats.retained_bytes,
        events.iter().map(|e| e.estimated_bytes()).sum::<usize>()
    );
    assert!(!snapshot(&bus).replay_complete);
    drop(slow);
    assert!(bus.publish(critical("core")).is_ok());
    bounded(&bus.stats());
    eprintln!("LR-9A synthetic stress: published={} retained={} bytes={} stream_evicted={} live_dropped={} state_evicted={} critical_evicted={}",
        stats.published, stats.retained_events, stats.retained_bytes, stats.evicted.stream, stats.live_delivery_dropped, stats.evicted.state, stats.evicted.critical);
}

#[test]
fn core_observation_path_has_no_provider_execution_network_or_presentation_dependency() {
    for source in [
        include_str!("mod.rs"),
        include_str!("contract.rs"),
        include_str!("bus.rs"),
        include_str!("coalesce.rs"),
    ] {
        for forbidden in [
            "crate::cognition",
            "crate::agents",
            "crate::presentation",
            "tauri::ipc",
            "reqwest",
            "serde_json::Value",
            "std::process",
            "std::fs",
            "tokio::",
            "TaskEventKind",
            "TaskEventBroker",
        ] {
            assert!(
                !source.contains(forbidden),
                "unexpected observation dependency: {forbidden}"
            );
        }
    }
}

#[test]
fn process_wide_identity_and_bus_remain_after_presentation_lifecycle_changes() {
    let first = OperationalTraceBus::process_wide();
    let second = OperationalTraceBus::process_wide();
    assert!(Arc::ptr_eq(&first, &second));
    let registry = crate::luna::runtime::TaskRegistry::default();
    assert!(registry.suspend_ui_if_safe());
    let mut subscriber = first.subscribe().unwrap();
    let before = first.publish(state("core")).unwrap().sequence;
    drop(second);
    registry.resume_ui();
    registry.events.detach_main();
    let after = first.publish(critical("core")).unwrap().sequence;
    assert_eq!(after, before + 1);
    assert_eq!(
        subscriber.drain_batch(BatchLimits::default()).events.len(),
        2
    );
}
