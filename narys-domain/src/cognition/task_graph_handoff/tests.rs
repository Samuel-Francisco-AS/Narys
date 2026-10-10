use super::*;
use crate::{
    cognition::{
        allocation_policy,
        policy::{self, CognitiveRole},
    },
    luna::runtime::TaskRegistry,
    persistence::task_history,
};

#[test]
fn c3_a_restart_checkpoint_root_above_terminal_history_never_reuses_id() {
    let directory = std::env::temp_dir().join(format!(
        "c3-root-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = Database::for_test(directory.join("test.sqlite3"));
    let mut conn = db.open().unwrap();
    let mut routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
    routing.routing_mode = RoutingMode::Fixed;
    routing.targets = vec![crate::cognition::policy::CognitiveTargetPolicy {
        provider_id: "runtime-a".into(),
        model: "model-a".into(),
        thinking_level: None,
    }];
    let dto = allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
    let snapshot = TaskPolicySnapshot::new(
        routing.clone(),
        if routing.routing_mode == RoutingMode::Auto {
            Some(dto)
        } else {
            None
        },
    )
    .unwrap();
    let target = &routing.targets[0];
    let unit = ExecutionUnitId::new(417, 1).unwrap();
    let checkpoint = CheckpointId::new(unit, 1).unwrap();
    let cp = CognitiveCheckpoint::confirmed(
        checkpoint,
        ExecutionUnitFacts {
            id: unit,
            state: ExecutionUnitState::Completed,
            effects: EffectState::NotStarted,
        },
        HandoffBoundary::ConfirmedCompletion { checkpoint },
        None,
        snapshot,
        AllocationVariant {
            resource_id: ResourceId::new(&target.provider_id).unwrap(),
            access_path: AccessPath::new("provider_runtime").unwrap(),
            billing_domain_id: BillingDomainId::new(&target.provider_id).unwrap(),
            model_id: ModelId::new(&target.model).unwrap(),
            effort: target.thinking_level.map(EffortId::from_thinking_level),
        },
        CheckpointProvenance {
            source: ExecutionSource::subtask("a").unwrap(),
            runtime_id: RuntimeId::new(&target.provider_id).unwrap(),
        },
        HandoffContext::default(),
    )
    .unwrap();
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    task_history::insert(
        &conn,
        &task_history::TaskRecord {
            task_id: 416,
            kind: "task_graph".into(),
            state: "completed".into(),
            started_at: "2026-10-06T00:00:00Z".into(),
            finished_at: "2026-10-06T00:00:01Z".into(),
            summary: None,
            error_code: None,
        },
    )
    .unwrap();
    drop(conn);
    let conn = db.open().unwrap();
    let registry = TaskRegistry::default();
    registry.seed_next_id(task_history::max_id(&conn).unwrap());
    registry.seed_next_id(416); // A late/older seed cannot regress the allocator.
    assert_eq!(registry.register().unwrap().0 .0, 418);
    assert_eq!(registry.reserve_background_id().unwrap().0, 419);
    assert!(matches!(
        CheckpointRepository::lookup(&conn, unit, &ExecutionSource::subtask("a").unwrap()).unwrap(),
        CheckpointLoadResult::Committed(_)
    ));
    drop(conn);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn c3_a_recovery_preserves_task_id_ceiling_and_rejects_invalid_history() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::persistence::migrations::apply(&conn).unwrap();
    conn.execute("INSERT INTO task_records(task_id,kind,state,started_at,finished_at) VALUES (?1,'task_graph','completed','s','f')", [MAX_HANDOFF_SEQUENCE + 1]).unwrap();
    assert!(task_history::max_id(&conn).is_err());
    let registry = TaskRegistry::default();
    registry.seed_next_id(MAX_HANDOFF_SEQUENCE);
    assert!(registry.register().is_err());
    assert!(registry.reserve_background_id().is_err());
}

#[test]
fn c3_a_recovery_cannot_be_shadowed_by_temp_history_or_checkpoint_tables() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::persistence::migrations::apply(&conn).unwrap();
    conn.execute_batch("INSERT INTO main.task_records(task_id,kind,state,started_at,finished_at) VALUES (417,'task_graph','completed','s','f'); CREATE TEMP TABLE task_records(task_id); CREATE TEMP TABLE cognitive_checkpoints(root_task_id);").unwrap();
    assert_eq!(task_history::max_id(&conn).unwrap(), 417);
}

#[test]
fn c3_j_committed_unknown_receipt_is_rejected_by_c1_boundary_even_without_row_mismatch() {
    let directory = std::env::temp_dir().join(format!(
        "c3-unknown-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = Database::for_test(directory.join("test.sqlite3"));
    let mut conn = db.open().unwrap();
    let mut routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
    routing.routing_mode = RoutingMode::Fixed;
    routing.targets = vec![crate::cognition::policy::CognitiveTargetPolicy {
        provider_id: "runtime-a".into(),
        model: "model-a".into(),
        thinking_level: None,
    }];
    let policy = TaskPolicySnapshot::new(routing, None).unwrap();
    let unit = ExecutionUnitId::new(100, 1).unwrap();
    let id = CheckpointId::new(unit, 1).unwrap();
    let cp = CognitiveCheckpoint::confirmed(
        id,
        ExecutionUnitFacts {
            id: unit,
            state: ExecutionUnitState::Completed,
            effects: EffectState::UnknownOrInFlight,
        },
        HandoffBoundary::ConfirmedCompletion { checkpoint: id },
        None,
        policy,
        AllocationVariant {
            resource_id: ResourceId::new("runtime-a").unwrap(),
            access_path: AccessPath::new("provider_runtime").unwrap(),
            billing_domain_id: BillingDomainId::new("runtime-a").unwrap(),
            model_id: ModelId::new("model-a").unwrap(),
            effort: None,
        },
        CheckpointProvenance {
            source: ExecutionSource::subtask("a").unwrap(),
            runtime_id: RuntimeId::new("runtime-a").unwrap(),
        },
        HandoffContext::default(),
    )
    .unwrap();
    let receipt = CheckpointRepository::commit(&mut conn, &cp).unwrap();
    assert_eq!(
        validate_boundary(ExecutionUnitId::new(100, 2).unwrap(), &[receipt], false),
        Err("handoff_boundary_unsafe")
    );
    drop(conn);
    std::fs::remove_dir_all(directory).unwrap();
}
