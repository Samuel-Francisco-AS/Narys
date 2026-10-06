use super::*;
use crate::persistence::{database::Database, migrations};
const ROLES: [CognitiveRole; 4] = [
    CognitiveRole::Conversation,
    CognitiveRole::Summary,
    CognitiveRole::Orchestrator,
    CognitiveRole::Worker,
];
fn fresh() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrations::apply(&conn).unwrap();
    conn
}
fn defaults(conn: &Connection) {
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        16
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM cognitive_role_allocation_policies",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
    for role in ROLES {
        let config = load(conn, role).unwrap();
        assert_eq!(config.role, role);
        assert_eq!(config.allocation_profile, AllocationProfile::Balanced);
        assert_eq!(config.variant_selection_mode, VariantSelectionMode::Auto);
        assert_eq!(config.minimum_cognitive_tier, None);
        assert_eq!(config.paid_use_policy, PaidUseMode::Deny);
        assert_eq!(
            (
                config.max_paid_currency,
                config.max_paid_micros,
                config.reduced_below_percent,
                config.reserve_below_percent
            ),
            (None, None, None, None)
        );
        let runtime = load(conn, role).unwrap().to_runtime().unwrap();
        assert_eq!(
            runtime,
            AllocationRuntimePolicy::new(provider_allocation_default(), None)
        );
    }
    assert_eq!(
        conn.pragma_query_value(None, "integrity_check", |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
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
fn b4_fresh_v13_defaults_and_idempotence() {
    let conn = fresh();
    defaults(&conn);
    migrations::apply(&conn).unwrap();
    defaults(&conn);
}
fn v12() -> Connection {
    let conn = fresh();
    conn.execute_batch("DROP TABLE IF EXISTS cognitive_continuation_units; DROP TABLE IF EXISTS cognitive_continuations; DROP TABLE cognitive_checkpoints; DROP TABLE checkpoint_task_policies; DROP TABLE cognitive_role_allocation_policies; PRAGMA user_version=12;")
        .unwrap();
    conn
}
#[test]
fn b4_v12_upgrade_preserves_routing_and_b3_defaults() {
    let mut conn = v12();
    let mut routing = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    routing.routing_mode = RoutingMode::Auto;
    routing
        .targets
        .push(super::super::policy::CognitiveTargetPolicy {
            provider_id: "groq".into(),
            model: "configured".into(),
            thinking_level: None,
        });
    policy::save(&mut conn, &routing).unwrap();
    migrations::apply(&conn).unwrap();
    defaults(&conn);
    assert_eq!(policy::load(&conn, routing.role).unwrap(), routing);
}
#[test]
fn b4_future_version_rejected() {
    let conn = fresh();
    conn.pragma_update(None, "user_version", 17).unwrap();
    assert_eq!(
        migrations::apply(&conn).unwrap_err().code(),
        "migration_failed"
    );
}
#[test]
fn b4_migration_013_failure_rolls_back_and_can_retry() {
    let conn = v12();
    conn.execute_batch("CREATE TABLE cognitive_role_allocation_policies(fake TEXT);")
        .unwrap();
    assert!(migrations::apply(&conn).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    conn.execute_batch("DROP TABLE cognitive_role_allocation_policies;")
        .unwrap();
    migrations::apply(&conn).unwrap();
    defaults(&conn);
}
#[test]
fn b4_profiles_variants_floor_and_money_reserve_roundtrip() {
    let mut conn = fresh();
    for profile in [
        AllocationProfile::Economy,
        AllocationProfile::Balanced,
        AllocationProfile::Fast,
    ] {
        for mode in [VariantSelectionMode::Explicit, VariantSelectionMode::Auto] {
            for floor in [None, Some(0), Some(255)] {
                for amount in [0, 9007199254740991] {
                    let mut value = load(&conn, CognitiveRole::Worker).unwrap();
                    value.allocation_profile = profile;
                    value.variant_selection_mode = mode;
                    value.minimum_cognitive_tier = floor;
                    value.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
                    value.max_paid_currency = Some("USD".into());
                    value.max_paid_micros = Some(amount);
                    value.reduced_below_percent = Some(100);
                    value.reserve_below_percent = Some(0);
                    assert_eq!(save(&mut conn, &value).unwrap(), value);
                    assert_eq!(
                        load(&conn, value.role)
                            .unwrap()
                            .to_runtime()
                            .unwrap()
                            .minimum_cognitive_tier()
                            .map(|t| t.value()),
                        floor.map(|n| n as u8)
                    );
                }
            }
        }
    }
}
#[test]
fn b4_dto_deserialize_never_bypasses_constructors() {
    let conn = fresh();
    let base = serde_json::to_value(load(&conn, CognitiveRole::Conversation).unwrap()).unwrap();
    for (key, value) in [
        ("minimumCognitiveTier", serde_json::json!(256)),
        ("reserveBelowPercent", serde_json::json!(5)),
        ("maxPaidMicros", serde_json::json!(0)),
    ] {
        let mut json = base.clone();
        json[key] = value;
        let dto: CognitiveRoleAllocationPolicy = serde_json::from_value(json).unwrap();
        assert_eq!(dto.to_runtime().unwrap_err(), "allocation_policy_invalid");
    }
    for (key, value) in [
        ("allocationProfile", "unknown"),
        ("variantSelectionMode", "unknown"),
        ("paidUsePolicy", "unknown"),
    ] {
        let mut json = base.clone();
        json[key] = serde_json::json!(value);
        assert!(serde_json::from_value::<CognitiveRoleAllocationPolicy>(json).is_err());
    }
}
macro_rules! rejected {
    ($name:ident, $set:expr) => { #[test] fn $name() {
        let conn = fresh();
        assert!(conn.execute(&format!("UPDATE cognitive_role_allocation_policies SET {} WHERE role='conversation'", $set), []).is_err());
        assert_eq!(load(&conn, CognitiveRole::Conversation).unwrap().to_runtime().unwrap(), AllocationRuntimePolicy::new(provider_allocation_default(), None));
    }};
}
rejected!(b4_sql_profile, "allocation_profile='private-secret'");
rejected!(b4_sql_variant, "variant_selection_mode='private-prompt'");
rejected!(b4_sql_tier_256, "minimum_cognitive_tier=256");
rejected!(b4_sql_tier_negative, "minimum_cognitive_tier=-1");
rejected!(b4_sql_tier_fractional, "minimum_cognitive_tier=1.5");
rejected!(
    b4_sql_currency_lowercase,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='usd',max_paid_micros=0"
);
rejected!(
    b4_sql_currency_short,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='US',max_paid_micros=0"
);
rejected!(
    b4_sql_currency_long,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='USDD',max_paid_micros=0"
);
rejected!(
    b4_sql_currency_non_ascii,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='ÜSD',max_paid_micros=0"
);
rejected!(
    b4_sql_micros_negative,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='USD',max_paid_micros=-1"
);
rejected!(b4_sql_micros_overflow, "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='USD',max_paid_micros=9007199254740992");
rejected!(
    b4_sql_micros_fractional,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='USD',max_paid_micros=0.5"
);
rejected!(
    b4_sql_deny_with_budget,
    "max_paid_currency='USD',max_paid_micros=0"
);
rejected!(
    b4_sql_allow_without_currency,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_micros=0"
);
rejected!(
    b4_sql_allow_without_micros,
    "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='USD'"
);
rejected!(b4_sql_paid_enum, "paid_use_policy='allow_everything'");
rejected!(b4_sql_only_reduced, "reduced_below_percent=40");
rejected!(b4_sql_only_reserve, "reserve_below_percent=10");
rejected!(
    b4_sql_reserve_above_reduced,
    "reduced_below_percent=10,reserve_below_percent=11"
);
rejected!(
    b4_sql_percent_over_100,
    "reduced_below_percent=101,reserve_below_percent=10"
);
rejected!(
    b4_sql_percent_negative,
    "reduced_below_percent=40,reserve_below_percent=-1"
);
rejected!(
    b4_sql_percent_fractional,
    "reduced_below_percent=40.5,reserve_below_percent=10"
);
rejected!(b4_sql_role_unknown, "role='unknown'");
#[test]
fn b4_sql_foreign_key_cascade() {
    let conn = fresh();
    conn.execute(
        "DELETE FROM cognitive_role_policies WHERE role='conversation'",
        [],
    )
    .unwrap();
    assert!(load(&conn, CognitiveRole::Conversation).is_err());
    assert!(conn.execute("INSERT INTO cognitive_role_allocation_policies(role,allocation_profile,variant_selection_mode,paid_use_policy) VALUES('conversation','balanced','auto','deny')", []).is_err());
}
#[test]
fn b4_load_rejects_missing_and_corrupt_rows_with_sanitized_errors() {
    let conn = fresh();
    conn.execute_batch("PRAGMA ignore_check_constraints=ON")
        .unwrap();
    for set in ["allocation_profile='private-budget'", "variant_selection_mode='private-account'", "paid_use_policy='private-provider'", "minimum_cognitive_tier=256", "max_paid_currency='private-secret'", "reduced_below_percent=101,reserve_below_percent=10", "paid_use_policy='allow_known_cost_within_budget',max_paid_currency='usd',max_paid_micros=0"] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(&format!("UPDATE cognitive_role_allocation_policies SET {set} WHERE role='conversation'"), []).unwrap();
        assert_eq!(load(&tx, CognitiveRole::Conversation).unwrap_err().code(), "read_failed");
        tx.rollback().unwrap();
    }
    conn.execute(
        "DELETE FROM cognitive_role_allocation_policies WHERE role='conversation'",
        [],
    )
    .unwrap();
    assert_eq!(
        load(&conn, CognitiveRole::Conversation).unwrap_err().code(),
        "read_failed"
    );
}
#[test]
fn b4_save_validates_before_writing_and_roles_stay_independent() {
    let mut conn = fresh();
    for (role, profile) in [
        (CognitiveRole::Conversation, AllocationProfile::Economy),
        (CognitiveRole::Orchestrator, AllocationProfile::Fast),
        (CognitiveRole::Worker, AllocationProfile::Balanced),
    ] {
        let mut v = load(&conn, role).unwrap();
        v.allocation_profile = profile;
        if role == CognitiveRole::Orchestrator {
            v.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
            v.max_paid_currency = Some("USD".into());
            v.max_paid_micros = Some(7);
        }
        save(&mut conn, &v).unwrap();
    }
    assert_eq!(
        load(&conn, CognitiveRole::Conversation)
            .unwrap()
            .allocation_profile,
        AllocationProfile::Economy
    );
    assert_eq!(
        load(&conn, CognitiveRole::Orchestrator)
            .unwrap()
            .allocation_profile,
        AllocationProfile::Fast
    );
    assert_eq!(
        load(&conn, CognitiveRole::Worker)
            .unwrap()
            .allocation_profile,
        AllocationProfile::Balanced
    );
    assert_eq!(
        load(&conn, CognitiveRole::Summary)
            .unwrap()
            .allocation_profile,
        AllocationProfile::Balanced
    );
    let original = load(&conn, CognitiveRole::Conversation).unwrap();
    let mut invalid = original.clone();
    invalid.max_paid_micros = Some(5);
    assert_eq!(
        save(&mut conn, &invalid).unwrap_err().code(),
        "write_failed"
    );
    assert_eq!(load(&conn, original.role).unwrap(), original);
    assert_eq!(original.paid_use_policy, PaidUseMode::Deny);
}
#[test]
fn b4_composite_save_roundtrip_and_role_mismatch() {
    let mut conn = fresh();
    let mut routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
    routing.context_max_bytes += 1;
    let mut allocation = load(&conn, routing.role).unwrap();
    allocation.allocation_profile = AllocationProfile::Fast;
    let saved = save_role_settings(&mut conn, &routing, &allocation).unwrap();
    assert_eq!(saved.policy, routing);
    assert_eq!(saved.allocation_policy, allocation);
    allocation.role = CognitiveRole::Conversation;
    assert_eq!(
        validate_role_settings(&routing, &allocation).unwrap_err(),
        "role_mismatch"
    );
    assert!(save_role_settings(&mut conn, &routing, &allocation).is_err());
    assert_eq!(
        load(&conn, CognitiveRole::Conversation)
            .unwrap()
            .allocation_profile,
        AllocationProfile::Balanced
    );
}
#[test]
fn b4_composite_second_write_failure_rolls_back_both_policies_and_targets() {
    let mut conn = fresh();
    let routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
    let allocation = load(&conn, routing.role).unwrap();
    let mut next_r = routing.clone();
    next_r.targets.reverse();
    next_r.context_max_bytes += 1;
    let mut next_a = allocation.clone();
    next_a.allocation_profile = AllocationProfile::Economy;
    conn.execute_batch("CREATE TRIGGER synthetic_failure BEFORE UPDATE ON cognitive_role_allocation_policies BEGIN SELECT RAISE(ABORT,'private-budget-currency-secret'); END;").unwrap();
    assert_eq!(
        save_role_settings(&mut conn, &next_r, &next_a)
            .unwrap_err()
            .code(),
        "write_failed"
    );
    assert!(conn.is_autocommit());
    assert_eq!(policy::load(&conn, routing.role).unwrap(), routing);
    assert_eq!(load(&conn, allocation.role).unwrap(), allocation);
}
#[test]
fn b4_summary_disable_cleanup_rolls_back_on_failure() {
    let mut conn = fresh();
    conn.execute("INSERT INTO conversation_sessions(kind,status,summary_status) VALUES('product','closed','pending')", []).unwrap();
    let original = policy::load(&conn, CognitiveRole::Summary).unwrap();
    let mut routing = original.clone();
    routing.summary_input_max_bytes = 0;
    let allocation = load(&conn, routing.role).unwrap();
    conn.execute_batch("CREATE TRIGGER cleanup_failure BEFORE UPDATE ON conversation_sessions BEGIN SELECT RAISE(ABORT,'private-transcript'); END;").unwrap();
    assert!(save_role_settings(&mut conn, &routing, &allocation).is_err());
    assert_eq!(policy::load(&conn, routing.role).unwrap(), original);
    conn.execute_batch("DROP TRIGGER cleanup_failure").unwrap();
    save_role_settings(&mut conn, &routing, &allocation).unwrap();
    assert_eq!(
        policy::load(&conn, routing.role)
            .unwrap()
            .summary_input_max_bytes,
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT summary_status FROM conversation_sessions",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "none"
    );
    let mut changed = allocation;
    changed.allocation_profile = AllocationProfile::Economy;
    save_role_settings(&mut conn, &routing, &changed).unwrap();
    assert_eq!(
        policy::load(&conn, routing.role)
            .unwrap()
            .summary_input_max_bytes,
        0
    );
}
#[test]
fn b4_fixed_preferred_skip_missing_or_corrupt_allocation_auto_fails_closed() {
    let mut conn = fresh();
    let mut routing = policy::load(&conn, CognitiveRole::Conversation).unwrap();
    conn.execute(
        "DELETE FROM cognitive_role_allocation_policies WHERE role='conversation'",
        [],
    )
    .unwrap();
    assert!(load_role_runtime_policy(&conn, routing.role)
        .unwrap()
        .allocation
        .is_none());
    routing
        .targets
        .push(super::super::policy::CognitiveTargetPolicy {
            provider_id: "groq".into(),
            model: "configured".into(),
            thinking_level: None,
        });
    routing.routing_mode = RoutingMode::Preferred;
    policy::save(&mut conn, &routing).unwrap();
    assert!(load_role_runtime_policy(&conn, routing.role)
        .unwrap()
        .allocation
        .is_none());
    routing.routing_mode = RoutingMode::Auto;
    policy::save(&mut conn, &routing).unwrap();
    assert_eq!(
        load_role_runtime_policy(&conn, routing.role)
            .unwrap_err()
            .code(),
        "read_failed"
    );
    conn.execute_batch("PRAGMA ignore_check_constraints=ON; INSERT INTO cognitive_role_allocation_policies(role,allocation_profile,variant_selection_mode,paid_use_policy) VALUES('conversation','private-corruption','auto','deny');").unwrap();
    assert!(load_role_runtime_policy(&conn, routing.role).is_err());
    routing.routing_mode = RoutingMode::Preferred;
    policy::save(&mut conn, &routing).unwrap();
    assert!(load_role_runtime_policy(&conn, routing.role)
        .unwrap()
        .allocation
        .is_none());
}
#[test]
fn b4_atomic_read_snapshot_across_concurrent_composite_save_and_multiple_roles() {
    let dir = std::env::temp_dir().join(format!(
        "b4-snapshot-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = Database::new(dir.clone());
    let conn = db.open().unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let mut initialized = policy::load(&conn, CognitiveRole::Worker).unwrap();
    initialized.routing_mode = RoutingMode::Auto;
    let allocation = load(&conn, initialized.role).unwrap();
    let mut writer = db.open().unwrap();
    save_role_settings(&mut writer, &initialized, &allocation).unwrap();
    drop(writer);
    let old_routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
    let old_allocation = load(&conn, CognitiveRole::Worker).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    // Establish read snapshot before the concurrent writer commits.
    assert_eq!(
        policy::load(&tx, CognitiveRole::Worker).unwrap(),
        old_routing
    );
    let writer = db.clone();
    std::thread::spawn(move || {
        let mut conn = writer.open().unwrap();
        let mut routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
        routing.routing_mode = RoutingMode::Auto;
        routing.targets.reverse();
        let mut allocation = load(&conn, routing.role).unwrap();
        allocation.allocation_profile = AllocationProfile::Fast;
        save_role_settings(&mut conn, &routing, &allocation).unwrap();
    })
    .join()
    .unwrap();
    assert_eq!(load(&tx, CognitiveRole::Worker).unwrap(), old_allocation);
    let snapshots =
        load_role_runtime_policies(&tx, &[CognitiveRole::Orchestrator, CognitiveRole::Worker])
            .unwrap();
    assert_eq!(snapshots[1].routing, old_routing);
    assert_eq!(
        snapshots[1].allocation.as_ref().unwrap().policy().profile,
        AllocationProfile::Balanced
    );
    tx.commit().unwrap();
    let next = load_role_runtime_policy(&conn, CognitiveRole::Worker).unwrap();
    assert_eq!(next.routing.routing_mode, RoutingMode::Auto);
    assert_eq!(
        next.allocation.unwrap().policy().profile,
        AllocationProfile::Fast
    );
    assert_eq!(snapshots[1].routing, old_routing);
    drop(conn);
    std::fs::remove_dir_all(dir).unwrap();
}
