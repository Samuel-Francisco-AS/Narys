use super::*;
use crate::{
    cognition::{
        allocation_policy,
        policy::{self, CognitiveRole, RoutingMode},
    },
    cognitive_resources::*,
    persistence::{database::Database, migrations},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    },
};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture {
    directory: PathBuf,
    db: Database,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "lr85c-c2-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let db = Database::for_test(directory.join("test.sqlite3"));
        db.open().unwrap();
        Self { directory, db }
    }
    fn open(&self) -> Connection {
        self.db.open().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn unit(sequence: u64) -> ExecutionUnitId {
    ExecutionUnitId::new(17, sequence).unwrap()
}
fn receipt(sequence: u64) -> CheckpointId {
    CheckpointId::new(unit(sequence), 1).unwrap()
}
fn prepared(conn: &Connection, sequence: u64, effects: EffectState) -> CognitiveCheckpoint {
    let mut routing = policy::load(conn, CognitiveRole::Worker).unwrap();
    routing.routing_mode = RoutingMode::Fixed;
    routing.targets.truncate(1);
    routing.targets[0].provider_id = "runtime-a".into();
    routing.targets[0].model = "model-a".into();
    routing.targets[0].thinking_level = None;
    let snapshot = TaskPolicySnapshot::new(routing, None).unwrap();
    CognitiveCheckpoint::confirmed(
        receipt(sequence),
        ExecutionUnitFacts {
            id: unit(sequence),
            state: ExecutionUnitState::Completed,
            effects,
        },
        HandoffBoundary::ConfirmedCompletion {
            checkpoint: receipt(sequence),
        },
        None,
        snapshot,
        AllocationVariant {
            resource_id: ResourceId::new("resource-a").unwrap(),
            access_path: AccessPath::new("local-path").unwrap(),
            billing_domain_id: BillingDomainId::new("local-domain").unwrap(),
            model_id: ModelId::new("model-a").unwrap(),
            effort: None,
        },
        CheckpointProvenance {
            source: ExecutionSource::subtask(format!("unit-{sequence}")).unwrap(),
            runtime_id: RuntimeId::new("runtime-a").unwrap(),
        },
        HandoffContext::default(),
    )
    .unwrap()
}
fn load(conn: &Connection, cp: &CognitiveCheckpoint) -> CheckpointLoadResult {
    CheckpointRepository::lookup(conn, cp.id.unit_id(), &cp.provenance.source).unwrap()
}
fn committed(conn: &Connection, cp: &CognitiveCheckpoint) -> CheckpointRecord {
    let CheckpointLoadResult::Committed(record) = load(conn, cp) else {
        panic!("expected validated committed receipt");
    };
    record
}
fn counts(conn: &Connection) -> (i64, i64) {
    (
        conn.query_row("SELECT COUNT(*) FROM checkpoint_task_policies", [], |r| {
            r.get(0)
        })
        .unwrap(),
        conn.query_row("SELECT COUNT(*) FROM cognitive_checkpoints", [], |r| {
            r.get(0)
        })
        .unwrap(),
    )
}
fn history(conn: &Connection, state: &str, subtask: Option<(&str, &str)>) {
    conn.execute("INSERT INTO task_records(task_id,kind,state,started_at,finished_at) VALUES(17,'task_graph',?1,'2026-01-01T00:00:00.000Z','2026-01-01T00:00:01.000Z')",[state]).unwrap();
    if let Some((id, state)) = subtask {
        conn.execute("INSERT INTO task_subtask_records(root_task_id,subtask_id,provider_id,state,finished_at) VALUES(17,?1,'runtime-a',?2,'2026-01-01T00:00:01.000Z')",params![id,state]).unwrap();
    }
}
fn rebuild(
    cp: &CognitiveCheckpoint,
    id: CheckpointId,
    facts: ExecutionUnitFacts,
    boundary: HandoffBoundary,
) -> Result<CognitiveCheckpoint, CheckpointError> {
    CognitiveCheckpoint::confirmed(
        id,
        facts,
        boundary,
        cp.predecessor,
        cp.policy.clone(),
        cp.allocation.clone(),
        cp.provenance.clone(),
        cp.context.clone(),
    )
}
fn facts(cp: &CognitiveCheckpoint) -> ExecutionUnitFacts {
    ExecutionUnitFacts {
        id: cp.id.unit_id(),
        state: ExecutionUnitState::Completed,
        effects: cp.effects,
    }
}

#[test]
fn c2_a_exact_identity_allocation_policy_and_context_roundtrip() {
    let f = Fixture::new();
    let mut conn = f.open();
    let first = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    let mut second = prepared(&conn, 2, EffectState::Committed);
    second.predecessor = Some(first.id);
    second.context = HandoffContext::new(vec![first.id]).unwrap();
    let saved = CheckpointRepository::commit(&mut conn, &second).unwrap();
    assert_eq!(saved, committed(&conn, &second));
    assert_eq!(saved.checkpoint, second);
    assert_eq!(
        saved.boundary(),
        HandoffBoundary::ConfirmedCompletion {
            checkpoint: second.id
        }
    );
    assert_eq!(counts(&conn), (1, 2));
}
#[test]
fn c2_b_prepared_observations_are_not_committed_before_write() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    assert_eq!(load(&conn, &cp), CheckpointLoadResult::Absent);
    conn.execute_batch("CREATE TRIGGER reject_checkpoint BEFORE INSERT ON cognitive_checkpoints BEGIN SELECT RAISE(ABORT,'write_rejected'); END;").unwrap();
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &cp),
        Err(CheckpointError::Write)
    );
    assert_eq!(counts(&conn), (0, 0));
    assert_eq!(load(&conn, &cp), CheckpointLoadResult::Absent);
}
#[test]
fn c2_b_failed_commit_rolls_back_policy_checkpoint_and_deferred_write() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    conn.execute_batch("CREATE TABLE rollback_parent(id INTEGER PRIMARY KEY); CREATE TABLE rollback_child(id INTEGER REFERENCES rollback_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_commit AFTER INSERT ON cognitive_checkpoints BEGIN INSERT INTO rollback_child VALUES(1); END;").unwrap();
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &cp),
        Err(CheckpointError::Write)
    );
    assert!(conn.is_autocommit());
    assert_eq!(counts(&conn), (0, 0));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM rollback_child", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    assert_eq!(load(&f.open(), &cp), CheckpointLoadResult::Absent);
}
#[test]
fn c2_c_restart_preserves_committed_identity_and_forbids_replay() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    let saved = CheckpointRepository::commit(&mut conn, &cp).unwrap();
    drop(conn);
    let reopened = f.open();
    assert_eq!(saved, committed(&reopened, &cp));
    assert_eq!(
        load(&reopened, &cp).replay(),
        ReplayDecision::Forbidden(ReplayReason::EffectCommitted)
    );
}
#[test]
fn c2_d_identical_write_is_idempotent_including_timestamp_after_reopen() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    let first = CheckpointRepository::commit(&mut conn, &cp).unwrap();
    drop(conn);
    let mut reopened = f.open();
    let retry = CheckpointRepository::commit(&mut reopened, &cp).unwrap();
    assert_eq!(first, retry);
    assert_eq!(counts(&reopened), (1, 1));
}
#[test]
fn c2_e_conflicting_effect_or_allocation_never_overwrites_identity() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    let original = CheckpointRepository::commit(&mut conn, &cp).unwrap();
    for changed in [
        {
            let mut c = cp.clone();
            c.effects = EffectState::UnknownOrInFlight;
            c
        },
        {
            let mut c = cp.clone();
            c.allocation.resource_id = ResourceId::new("resource-b").unwrap();
            c
        },
    ] {
        assert_eq!(
            CheckpointRepository::commit(&mut conn, &changed),
            Err(CheckpointError::Conflict)
        );
        assert_eq!(committed(&conn, &cp), original);
    }
}
#[test]
fn c2_f_wrong_unit_or_unconfirmed_boundary_is_rejected() {
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    let mut wrong = facts(&cp);
    wrong.id = unit(2);
    assert_eq!(
        rebuild(
            &cp,
            cp.id,
            wrong,
            HandoffBoundary::ConfirmedCompletion { checkpoint: cp.id }
        ),
        Err(CheckpointError::UnitMismatch)
    );
    for boundary in [
        HandoffBoundary::Unknown,
        HandoffBoundary::Unconfirmed,
        HandoffBoundary::ConfirmedCompletion {
            checkpoint: receipt(2),
        },
    ] {
        assert_eq!(
            rebuild(&cp, cp.id, facts(&cp), boundary),
            Err(CheckpointError::BoundaryUnconfirmed)
        );
    }
}
#[test]
fn c2_g_root_mismatch_rejected_in_facts_and_dependency() {
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    let mut wrong = facts(&cp);
    wrong.id = ExecutionUnitId::new(18, 1).unwrap();
    assert_eq!(
        rebuild(
            &cp,
            cp.id,
            wrong,
            HandoffBoundary::ConfirmedCompletion { checkpoint: cp.id }
        ),
        Err(CheckpointError::TaskMismatch)
    );
    let mut changed = cp.clone();
    changed.context = HandoffContext::new(vec![CheckpointId::new(
        ExecutionUnitId::new(18, 1).unwrap(),
        1,
    )
    .unwrap()])
    .unwrap();
    assert_eq!(
        rebuild(
            &changed,
            changed.id,
            facts(&changed),
            HandoffBoundary::ConfirmedCompletion {
                checkpoint: changed.id
            }
        ),
        Err(CheckpointError::TaskMismatch)
    );
}
#[test]
fn c2_h_predecessor_and_dependencies_must_advance_sequence() {
    let f = Fixture::new();
    let conn = f.open();
    let mut cp = prepared(&conn, 2, EffectState::NotStarted);
    for previous in [receipt(2), receipt(3)] {
        cp.predecessor = Some(previous);
        assert_eq!(
            rebuild(
                &cp,
                cp.id,
                facts(&cp),
                HandoffBoundary::ConfirmedCompletion { checkpoint: cp.id }
            ),
            Err(CheckpointError::SequenceNotAdvancing)
        );
    }
}
#[test]
fn c2_h_one_final_checkpoint_per_unit_no_regressive_or_new_sequence() {
    let f = Fixture::new();
    let mut conn = f.open();
    let mut cp = prepared(&conn, 1, EffectState::NotStarted);
    cp.id = CheckpointId::new(unit(1), 2).unwrap();
    let first = CheckpointRepository::commit(&mut conn, &cp).unwrap();
    for sequence in [1, 3] {
        cp.id = CheckpointId::new(unit(1), sequence).unwrap();
        assert_eq!(
            CheckpointRepository::commit(&mut conn, &cp),
            Err(CheckpointError::Conflict)
        );
    }
    assert_eq!(committed(&conn, &cp), first);
}
#[test]
fn c2_i_unknown_effect_survives_reload_without_safe_boundary() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::UnknownOrInFlight);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    drop(conn);
    let record = committed(&f.open(), &cp);
    assert_eq!(record.checkpoint.effects, EffectState::UnknownOrInFlight);
    assert_eq!(record.boundary(), HandoffBoundary::Unknown);
    assert_eq!(
        record.replay(),
        ReplayDecision::Forbidden(ReplayReason::EffectUnknownOrInFlight)
    );
}
#[test]
fn c2_j_completed_without_effects_still_forbids_replay() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    drop(conn);
    assert_eq!(
        load(&f.open(), &cp).replay(),
        ReplayDecision::Forbidden(ReplayReason::UnitCompleted)
    );
}
#[test]
fn c2_k_corrupt_json_and_extra_fields_fail_closed() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    let mut value = serde_json::from_str::<serde_json::Value>(&cp.encode().unwrap()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    for corrupt in ["{".to_string(), value.to_string(), "[]".into()] {
        conn.execute(
            "UPDATE cognitive_checkpoints SET checkpoint_json=?1",
            [corrupt],
        )
        .unwrap();
        assert!(matches!(load(&conn, &cp), CheckpointLoadResult::Invalid(_)));
        assert_eq!(
            load(&conn, &cp).replay(),
            ReplayDecision::Forbidden(ReplayReason::UnitStateUnknown)
        );
    }
}
#[test]
fn c2_k_invalid_ids_lifecycle_boundary_and_index_linkage_fail_closed() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    let original = serde_json::from_str::<serde_json::Value>(&cp.encode().unwrap()).unwrap();
    for (field, value) in [
        ("rootTaskId", 0),
        ("rootTaskId", 18),
        ("unitSequence", 0),
        ("unitSequence", 2),
        ("sequence", 0),
        ("sequence", MAX_HANDOFF_SEQUENCE + 1),
    ] {
        let mut corrupt = original.clone();
        corrupt["id"][field] = serde_json::json!(value);
        conn.execute(
            "UPDATE cognitive_checkpoints SET checkpoint_json=?1",
            [corrupt.to_string()],
        )
        .unwrap();
        assert!(matches!(load(&conn, &cp), CheckpointLoadResult::Invalid(_)));
    }
    for (field, value) in [
        ("lifecycle", "running"),
        ("boundary", "unconfirmed"),
        ("effects", "committed"),
    ] {
        let mut corrupt = original.clone();
        corrupt[field] = serde_json::json!(value);
        conn.execute(
            "UPDATE cognitive_checkpoints SET checkpoint_json=?1",
            [corrupt.to_string()],
        )
        .unwrap();
        assert!(matches!(load(&conn, &cp), CheckpointLoadResult::Invalid(_)));
    }
}
#[test]
fn c2_k_corrupt_policy_and_timestamp_not_promoted() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    conn.execute("UPDATE checkpoint_task_policies SET snapshot_json='{}'", [])
        .unwrap();
    assert!(matches!(load(&conn, &cp), CheckpointLoadResult::Invalid(_)));
    conn.execute(
        "UPDATE checkpoint_task_policies SET snapshot_json=?1",
        [serde_json::to_string(&cp.policy).unwrap()],
    )
    .unwrap();
    conn.execute(
        "UPDATE cognitive_checkpoints SET committed_at='2026-99-99T00:00:00.000Z'",
        [],
    )
    .unwrap();
    assert!(matches!(load(&conn, &cp), CheckpointLoadResult::Invalid(_)));
}
#[test]
fn c2_l_provenance_roundtrip_has_only_public_identity_and_enum_fields() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    assert_eq!(committed(&conn, &cp).checkpoint.provenance, cp.provenance);
    let value = serde_json::from_str::<serde_json::Value>(&cp.encode().unwrap()).unwrap();
    let keys: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "id",
            "lifecycle",
            "boundary",
            "effects",
            "predecessor",
            "allocation",
            "source",
            "runtimeId",
            "completedDependencies"
        ])
    );
    assert_eq!(value["boundary"], "confirmed_completion");
}
#[test]
fn c2_m_context_bounds_duplicates_and_canonical_order() {
    assert!(HandoffContext::new((1..=32).map(receipt).collect()).is_ok());
    assert_eq!(
        HandoffContext::new((1..=33).map(receipt).collect()),
        Err(CheckpointError::ContextLimit)
    );
    assert_eq!(
        HandoffContext::new(vec![receipt(1), receipt(1)]),
        Err(CheckpointError::InvalidContext)
    );
    assert_eq!(
        HandoffContext::new(vec![receipt(2), receipt(1)]),
        HandoffContext::new(vec![receipt(1), receipt(2)])
    );
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 40, EffectState::NotStarted);
    let mut wire = serde_json::from_str::<serde_json::Value>(&cp.encode().unwrap()).unwrap();
    wire["completedDependencies"] = serde_json::json!((1..=33)
        .map(|i| serde_json::json!({"rootTaskId":17,"unitSequence":i,"sequence":1}))
        .collect::<Vec<_>>());
    let parsed: CheckpointWire = serde_json::from_value(wire).unwrap();
    assert_eq!(
        parsed.validated(cp.policy),
        Err(CheckpointError::ContextLimit)
    );
    assert_eq!(counts(&conn), (0, 0));
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    assert!(conn
        .execute(
            "UPDATE cognitive_checkpoints SET checkpoint_json=?1",
            ["x".repeat(MAX_CHECKPOINT_BYTES + 1)]
        )
        .is_err());
    conn.pragma_update(None, "ignore_check_constraints", true)
        .unwrap();
    conn.execute(
        "UPDATE cognitive_checkpoints SET checkpoint_json=?1",
        ["x".repeat(MAX_CHECKPOINT_BYTES + 1)],
    )
    .unwrap();
    assert!(matches!(load(&conn, &cp), CheckpointLoadResult::Invalid(_)));
}
#[test]
fn c2_n_schema_and_wire_have_no_generic_content_or_commercial_binding() {
    let schema =
        include_str!("../../../migrations/014_cognitive_checkpoints.sql").to_ascii_lowercase();
    let contracts = include_str!("contracts.rs");
    for field in [
        "prompt",
        "chain_of_thought",
        "api_key",
        "headers",
        "cookies",
        "credentials",
        "response_body",
        "secret",
    ] {
        assert!(!schema.contains(field));
    }
    for brand in ["groq", "cloudflare", "gemini", "codex", "copilot"] {
        assert!(!schema.contains(brand));
        assert!(!contracts.to_ascii_lowercase().contains(brand));
    }
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    let mut wire = serde_json::from_str::<serde_json::Value>(&cp.encode().unwrap()).unwrap();
    wire["contextText"] = serde_json::json!("untrusted");
    assert!(serde_json::from_value::<CheckpointWire>(wire).is_err());
    for error in [
        CheckpointError::InvalidIdentity,
        CheckpointError::InvalidContext,
        CheckpointError::Conflict,
    ] {
        assert_eq!(error.to_string(), format!("{error:?}"));
    }
}
#[test]
fn c2_nonterminal_unit_is_never_checkpointed() {
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    for state in [
        ExecutionUnitState::NotStarted,
        ExecutionUnitState::Running,
        ExecutionUnitState::PartialOutputObserved,
        ExecutionUnitState::Cancelled,
        ExecutionUnitState::Failed,
        ExecutionUnitState::Unknown,
    ] {
        let mut fact = facts(&cp);
        fact.state = state;
        assert_eq!(
            rebuild(
                &cp,
                cp.id,
                fact,
                HandoffBoundary::ConfirmedCompletion { checkpoint: cp.id }
            ),
            Err(CheckpointError::UnitNotCompleted)
        );
    }
}
#[test]
fn c2_source_ids_are_bounded_public_machine_labels() {
    for bad in [
        "".to_string(),
        "a".repeat(65),
        "unit 1".into(),
        "../a".into(),
        "a\n".into(),
        "á".into(),
    ] {
        assert_eq!(
            ExecutionSource::subtask(bad),
            Err(CheckpointError::InvalidProvenance)
        );
    }
    assert!(ExecutionSource::subtask("a".repeat(64)).is_ok());
}
#[test]
fn c2_source_cannot_be_rebound_to_another_unit() {
    let f = Fixture::new();
    let mut conn = f.open();
    let first = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    let mut second = prepared(&conn, 2, EffectState::NotStarted);
    second.provenance.source = first.provenance.source.clone();
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &second),
        Err(CheckpointError::Conflict)
    );
    assert_eq!(
        load(&conn, &second),
        CheckpointLoadResult::Invalid(CheckpointError::UnitMismatch)
    );
    assert_eq!(
        CheckpointRepository::lookup(&conn, unit(1), &ExecutionSource::subtask("other").unwrap())
            .unwrap(),
        CheckpointLoadResult::Invalid(CheckpointError::UnitMismatch)
    );
}
#[test]
fn c2_policy_is_immutable_for_task_role_and_independent_of_settings() {
    let f = Fixture::new();
    let mut conn = f.open();
    let first = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    conn.execute(
        "UPDATE cognitive_role_policies SET max_retries=9 WHERE role='worker'",
        [],
    )
    .unwrap();
    assert_eq!(committed(&conn, &first).checkpoint.policy, first.policy);
    let mut second = prepared(&conn, 2, EffectState::NotStarted);
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &second),
        Err(CheckpointError::Conflict)
    );
    second.policy = first.policy.clone();
    assert!(CheckpointRepository::commit(&mut conn, &second).is_ok());
}
#[test]
fn c2_auto_reuses_b4_dto_without_score_or_new_authorization() {
    let f = Fixture::new();
    let mut conn = f.open();
    let mut cp = prepared(&conn, 1, EffectState::NotStarted);
    let mut routing = cp.policy.routing().clone();
    routing.routing_mode = RoutingMode::Auto;
    let mut other = routing.targets[0].clone();
    other.provider_id = "runtime-b".into();
    routing.targets.push(other);
    let allocation = allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
    cp.policy = TaskPolicySnapshot::new(routing.clone(), Some(allocation.clone())).unwrap();
    cp.allocation.model_id = ModelId::new("model-actual-auto-variant").unwrap();
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    assert_eq!(
        committed(&conn, &cp).checkpoint.policy.allocation(),
        Some(&allocation)
    );
    assert_eq!(
        TaskPolicySnapshot::new(routing, None),
        Err(CheckpointError::InvalidPolicy)
    );
}
#[test]
fn c2_dependencies_require_exact_durable_safe_receipts() {
    let f = Fixture::new();
    let mut conn = f.open();
    let mut second = prepared(&conn, 2, EffectState::NotStarted);
    second.predecessor = Some(receipt(1));
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &second),
        Err(CheckpointError::ReferenceNotCommitted)
    );
    assert_eq!(counts(&conn), (0, 0));
    let first = prepared(&conn, 1, EffectState::UnknownOrInFlight);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &second),
        Err(CheckpointError::ReferenceNotCommitted)
    );
}
#[test]
fn c2_corrupt_dependency_after_write_invalidates_context_on_load() {
    let f = Fixture::new();
    let mut conn = f.open();
    let first = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    let mut second = prepared(&conn, 2, EffectState::NotStarted);
    second.context = HandoffContext::new(vec![first.id]).unwrap();
    CheckpointRepository::commit(&mut conn, &second).unwrap();
    conn.execute(
        "UPDATE cognitive_checkpoints SET checkpoint_json='{}' WHERE unit_sequence=1",
        [],
    )
    .unwrap();
    assert!(matches!(
        load(&conn, &second),
        CheckpointLoadResult::Invalid(_)
    ));
}
#[test]
fn c2_terminal_history_without_checkpoint_is_never_not_started() {
    for state in ["completed", "cancelled", "failed"] {
        let f = Fixture::new();
        let conn = f.open();
        history(&conn, state, None);
        let result =
            CheckpointRepository::lookup(&conn, unit(1), &ExecutionSource::RootTask).unwrap();
        assert_eq!(
            result,
            CheckpointLoadResult::HistoryWithoutCheckpoint {
                state: parse_history_state(state).unwrap()
            }
        );
        assert!(matches!(result.replay(), ReplayDecision::Forbidden(_)));
        let result = CheckpointRepository::lookup(
            &conn,
            unit(1),
            &ExecutionSource::subtask("missing").unwrap(),
        )
        .unwrap();
        assert_eq!(
            result,
            CheckpointLoadResult::Invalid(CheckpointError::HistoryContradiction)
        );
    }
}
#[test]
fn c2_completed_subtask_remains_committed_under_cancelled_root() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    history(&conn, "cancelled", Some(("unit-1", "completed")));
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    assert_eq!(committed(&conn, &cp).checkpoint, cp);
}
#[test]
fn c2_contradictory_history_rejects_write_and_later_lookup() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    history(&conn, "failed", Some(("unit-1", "failed")));
    assert_eq!(
        load(&conn, &cp),
        CheckpointLoadResult::Invalid(CheckpointError::HistoryContradiction)
    );
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &cp),
        Err(CheckpointError::HistoryContradiction)
    );
    conn.execute(
        "UPDATE task_subtask_records SET state='completed',provider_id='runtime-b'",
        [],
    )
    .unwrap();
    assert_eq!(
        load(&conn, &cp),
        CheckpointLoadResult::Invalid(CheckpointError::HistoryContradiction)
    );
}
#[test]
fn c2_absence_still_has_unknown_replay_fence() {
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    assert_eq!(load(&conn, &cp), CheckpointLoadResult::Absent);
    assert_eq!(
        load(&conn, &cp).replay(),
        ReplayDecision::Forbidden(ReplayReason::UnitStateUnknown)
    );
}
#[test]
fn c2_outer_transaction_and_non_durable_modes_are_rejected() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &cp),
        Err(CheckpointError::TransactionActive)
    );
    assert_eq!(
        CheckpointRepository::lookup(&conn, unit(1), &cp.provenance.source),
        Err(CheckpointError::TransactionActive)
    );
    conn.execute_batch("ROLLBACK;").unwrap();
    for setting in [
        "PRAGMA synchronous=OFF;",
        "PRAGMA journal_mode=MEMORY;",
        "PRAGMA foreign_keys=OFF;",
    ] {
        let mut other = f.open();
        other.execute_batch(setting).unwrap();
        assert_eq!(
            CheckpointRepository::commit(&mut other, &cp),
            Err(CheckpointError::DurabilityUnavailable)
        );
    }
    let mut memory = Connection::open_in_memory().unwrap();
    migrations::apply(&memory).unwrap();
    assert_eq!(
        CheckpointRepository::commit(&mut memory, &cp),
        Err(CheckpointError::DurabilityUnavailable)
    );
}
#[test]
fn c2_concurrent_identical_writes_have_one_identical_receipt() {
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    drop(conn);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let db = f.db.clone();
            let cp = cp.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut conn = db.open().unwrap();
                barrier.wait();
                CheckpointRepository::commit(&mut conn, &cp)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().unwrap().unwrap())
        .collect();
    assert_eq!(results[0], results[1]);
    assert_eq!(counts(&f.open()), (1, 1));
}
#[test]
fn c2_concurrent_conflicts_never_store_two_contents() {
    let f = Fixture::new();
    let conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    drop(conn);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [EffectState::Committed, EffectState::UnknownOrInFlight]
        .into_iter()
        .map(|effects| {
            let db = f.db.clone();
            let mut cp = cp.clone();
            cp.effects = effects;
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut conn = db.open().unwrap();
                barrier.wait();
                CheckpointRepository::commit(&mut conn, &cp)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(CheckpointError::Conflict))
            .count(),
        1
    );
    assert_eq!(counts(&f.open()), (1, 1));
    assert_eq!(
        committed(&f.open(), &cp),
        results.into_iter().find_map(Result::ok).unwrap()
    );
}
#[test]
fn c2_migration_014_upgrades_v13_preserving_history_and_b4_settings() {
    let f = Fixture::new();
    let conn = f.open();
    history(&conn, "completed", Some(("unit-1", "completed")));
    let routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
    let allocation = allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
    conn.execute_batch("DROP TABLE IF EXISTS cognitive_continuation_units; DROP TABLE IF EXISTS cognitive_continuations; DROP TABLE cognitive_checkpoints; DROP TABLE checkpoint_task_policies; PRAGMA user_version=13;").unwrap();
    drop(conn);
    let reopened = f.open();
    assert_eq!(
        reopened
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        21
    );
    assert_eq!(
        policy::load(&reopened, CognitiveRole::Worker).unwrap(),
        routing
    );
    assert_eq!(
        allocation_policy::load(&reopened, CognitiveRole::Worker).unwrap(),
        allocation
    );
    assert_eq!(
        reopened
            .query_row(
                "SELECT COUNT(*) FROM task_subtask_records WHERE state='completed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(counts(&reopened), (0, 0));
    migrations::apply(&reopened).unwrap();
}
#[test]
fn c2_migration_014_failure_is_atomic_and_retryable() {
    let f = Fixture::new();
    let conn = f.open();
    conn.execute_batch("DROP TABLE IF EXISTS cognitive_continuation_units; DROP TABLE IF EXISTS cognitive_continuations; DROP TABLE cognitive_checkpoints; DROP TABLE checkpoint_task_policies; PRAGMA user_version=13; CREATE TABLE cognitive_checkpoints(dummy INTEGER);").unwrap();
    assert!(migrations::apply(&conn).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        13
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='checkpoint_task_policies'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TABLE IF EXISTS cognitive_continuation_units; DROP TABLE IF EXISTS cognitive_continuations; DROP TABLE cognitive_checkpoints;")
        .unwrap();
    migrations::apply(&conn).unwrap();
    assert_eq!(counts(&conn), (0, 0));
}

#[test]
fn c2_transitive_verification_is_bounded_and_fails_closed() {
    let f = Fixture::new();
    let mut conn = f.open();
    let first = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    // Build a long, valid chain in one local SQLite transaction to exercise the
    // load-work bound without quadratic fixture construction through the API.
    let tx = conn.transaction().unwrap();
    for sequence in 2..=258 {
        let mut cp = prepared(&tx, sequence, EffectState::NotStarted);
        cp.predecessor = Some(receipt(sequence - 1));
        tx.execute("INSERT INTO cognitive_checkpoints(root_task_id,unit_sequence,checkpoint_sequence,role,source_kind,source_key,effect_state,checkpoint_json) VALUES(17,?1,1,'worker','task_graph_subtask',?2,'not_started',?3)", params![sequence,format!("unit-{sequence}"),cp.encode().unwrap()]).unwrap();
    }
    tx.commit().unwrap();
    assert!(matches!(
        load(&conn, &prepared(&conn, 257, EffectState::NotStarted)),
        CheckpointLoadResult::Committed(_)
    ));
    assert_eq!(
        load(&conn, &prepared(&conn, 258, EffectState::NotStarted)),
        CheckpointLoadResult::Invalid(CheckpointError::VerificationLimit)
    );
}

#[test]
fn c2_reference_checkpoint_sequence_must_match_exactly() {
    let f = Fixture::new();
    let mut conn = f.open();
    let first = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &first).unwrap();
    let mut second = prepared(&conn, 2, EffectState::NotStarted);
    second.predecessor = Some(CheckpointId::new(unit(1), 2).unwrap());
    assert_eq!(
        CheckpointRepository::commit(&mut conn, &second),
        Err(CheckpointError::ReferenceNotCommitted)
    );
    assert_eq!(counts(&conn), (1, 1));
}

#[test]
fn c2_root_checkpoint_validity_and_cancelled_root_contradiction() {
    let f = Fixture::new();
    let mut conn = f.open();
    let mut cp = prepared(&conn, 1, EffectState::NotStarted);
    cp.provenance.source = ExecutionSource::RootTask;
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    history(&conn, "completed", None);
    assert_eq!(committed(&conn, &cp).checkpoint, cp);
    conn.execute("UPDATE task_records SET state='cancelled'", [])
        .unwrap();
    assert_eq!(
        load(&conn, &cp),
        CheckpointLoadResult::Invalid(CheckpointError::HistoryContradiction)
    );
}

#[test]
fn c2_schema_rejects_invalid_indexed_id_and_state() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::NotStarted);
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    for sql in [
        "UPDATE cognitive_checkpoints SET unit_sequence=0",
        "UPDATE cognitive_checkpoints SET checkpoint_sequence=9007199254740992",
        "UPDATE cognitive_checkpoints SET effect_state='invalid'",
        "UPDATE cognitive_checkpoints SET commit_state='pending'",
        "UPDATE cognitive_checkpoints SET role='invalid'",
    ] {
        assert!(conn.execute(sql, []).is_err());
        assert_eq!(committed(&conn, &cp).checkpoint, cp);
    }
    assert_eq!(
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[test]
fn c2_temp_tables_cannot_shadow_durable_ledger_or_terminal_evidence() {
    let f = Fixture::new();
    let mut conn = f.open();
    let cp = prepared(&conn, 1, EffectState::Committed);
    let temp_schema = include_str!("../../../migrations/014_cognitive_checkpoints.sql")
        .replace("CREATE TABLE", "CREATE TEMP TABLE");
    conn.execute_batch(&temp_schema).unwrap();
    conn.execute_batch("CREATE TEMP TABLE task_records AS SELECT * FROM main.task_records WHERE 0; CREATE TEMP TABLE task_subtask_records AS SELECT * FROM main.task_subtask_records WHERE 0;").unwrap();
    CheckpointRepository::commit(&mut conn, &cp).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM main.cognitive_checkpoints", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM temp.cognitive_checkpoints", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    let conn = f.open();
    assert_eq!(committed(&conn, &cp).checkpoint, cp);
    history(&conn, "failed", Some(("unit-1", "failed")));
    conn.execute_batch("CREATE TEMP TABLE task_records AS SELECT * FROM main.task_records WHERE 0; CREATE TEMP TABLE task_subtask_records AS SELECT * FROM main.task_subtask_records WHERE 0;").unwrap();
    assert_eq!(
        load(&conn, &cp),
        CheckpointLoadResult::Invalid(CheckpointError::HistoryContradiction)
    );
}
