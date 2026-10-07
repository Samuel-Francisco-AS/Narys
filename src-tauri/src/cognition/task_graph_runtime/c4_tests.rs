//! Final integrated LR-8.5C gates: real graph/Worker/Scheduler, local mocks only.
use super::c3_tests::{exhaust, sequential, step, Fixture};
use super::*;
use crate::{
    cognition::{
        policy::{RoutingMode, ThinkingLevel},
        telemetry::{Provenance, QuotaDimension, QuotaScope},
    },
    cognitive_resources::*,
    persistence::migrations,
};
use std::sync::Mutex;

fn state(f: &Fixture) -> (String, Option<String>) {
    f.db.open()
        .unwrap()
        .query_row(
            "SELECT state,pause_reason FROM cognitive_continuations WHERE root_task_id=100",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}
fn channel() -> (Channel<TaskEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let saved = events.clone();
    (
        Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                saved
                    .lock()
                    .unwrap()
                    .push(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        }),
        events,
    )
}
fn recover(f: &mut Fixture) -> Arc<TaskRegistry> {
    f.db = Database::for_test(f.directory.join("test.sqlite3"));
    let mut conn = f.db.open().unwrap();
    let registry = Arc::new(TaskRegistry::default());
    registry.seed_next_id(task_history::max_id(&conn).unwrap());
    ContinuationRepository::recover(&mut conn).unwrap();
    registry
}
async fn crash_after_a(f: &mut Fixture) {
    let committed = Arc::new(tokio::sync::Notify::new());
    let done = committed.clone();
    let run = f.run(sequential(), move |e| {
        if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
            done.notify_one();
        }
    });
    tokio::pin!(run);
    tokio::select! { biased;
        _ = committed.notified() => {},
        _ = &mut run => panic!("crash boundary missed"),
    }
    // Dropping the graph future here simulates loss of all in-memory progress.
}
async fn resume(
    f: &Fixture,
    registry: Arc<TaskRegistry>,
) -> (Result<TaskState, &'static str>, Vec<serde_json::Value>) {
    let (tx, events) = channel();
    let outcome =
        resume_task_graph(registry, f.db.clone(), f.runtime.clone(), TaskId(100), tx).await;
    let saved = events.lock().unwrap().clone();
    (outcome, saved)
}
async fn economic_pause(f: &mut Fixture) {
    let scheduler = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(&scheduler, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Paused);
    assert_eq!(
        state(f),
        ("paused".into(), Some("economic_authorization".into()))
    );
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.receipts().len(), 1);
}

#[tokio::test]
async fn lr85c_final_a_d_t_cross_resource_reads_facts_only_at_next_boundary() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let s = f.runtime.scheduler.clone();
    let (outcome, events) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_output_observed" && e["subtask_id"] == "a" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(
        f.calls()
            .iter()
            .map(|c| c.provider.as_str())
            .collect::<Vec<_>>(),
        ["runtime-a", "runtime-b"]
    );
    let receipts = f.receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(
        receipts[0].checkpoint().allocation().resource_id.as_str(),
        "runtime-a"
    );
    let b = events
        .iter()
        .find(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b")
        .unwrap();
    assert_eq!(b["transitions"][0]["change"]["resource"], true);
}
#[tokio::test]
async fn lr85c_final_b_same_resource_model_effort_changes() {
    let mut f = Fixture::new(RoutingMode::Auto, true, false, false);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(
                    &s,
                    "runtime-a",
                    QuotaScope::Model {
                        model: "model-x".into(),
                    },
                );
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    let calls = f.calls();
    assert_eq!(calls[0].provider, calls[1].provider);
    assert_eq!(
        (&calls[0].model, calls[0].effort),
        (&"model-x".into(), Some(ThinkingLevel::Low))
    );
    assert_eq!(
        (&calls[1].model, calls[1].effort),
        (&"model-y".into(), Some(ThinkingLevel::High))
    );
}
#[tokio::test]
async fn lr85c_final_c_unchanged_selection_is_not_a_false_transition() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Completed);
    let b = events
        .iter()
        .find(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b")
        .unwrap();
    assert_eq!(
        b["transitions"][0]["change"],
        serde_json::json!({"resource":false,"accessPath":false,"model":false,"effort":false})
    );
}
#[tokio::test]
async fn lr85c_final_e_paid_only_deny_persists_pause_zero_paid_calls_and_accounting() {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, false);
    economic_pause(&mut f).await;
    assert!(f.calls().iter().all(|c| c.provider != "runtime-b"));
    let snapshots = f.runtime.scheduler.telemetry.snapshots();
    let paid = snapshots
        .iter()
        .find(|s| s.provider_id == "runtime-b")
        .unwrap();
    assert!(matches!(
        paid.usage[&crate::cognition::telemetry::UsageDimension::Requests].observed,
        crate::cognition::telemetry::Fact::Known { value: 0, .. }
    ));
    let original = f.snapshot.clone();
    let registry = recover(&mut f);
    assert_eq!(state(&f).1.as_deref(), Some("economic_authorization"));
    let (outcome, _) = resume(&f, registry).await;
    assert_eq!(outcome, Ok(TaskState::Paused));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(
        ContinuationRepository::load(&f.db.open().unwrap(), 100)
            .unwrap()
            .manifest
            .policy,
        original
    );
}
#[tokio::test]
async fn lr85c_final_f_original_allow_known_cost_permits_paid_candidate() {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, true);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(f.calls()[1].provider, "runtime-b");
    assert_eq!(f.receipts()[1].checkpoint().policy(), &f.snapshot);
}
#[tokio::test]
async fn lr85c_final_g_unknown_economics_stays_unknown_and_is_not_a_paid_pause() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
                exhaust(&s, "runtime-b", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(outcome.error_code, Some("handoff_allocation_unavailable"));
    assert_eq!(state(&f).1, None);
    assert_eq!(f.calls().len(), 1);
    // UNKNOWN has no positive spend evidence in B, never a fabricated price.
    assert_eq!(
        f.snapshot.allocation().unwrap().paid_use_policy,
        crate::cognition::allocation_policy::PaidUseMode::Deny
    );
}
#[tokio::test]
async fn lr85c_final_g_unknown_paid_cost_is_not_zero_or_authorized() {
    let mut f = Fixture::new_recovery_fixture(
        RoutingMode::Auto,
        false,
        false,
        false,
        true,
        true,
        false,
        false,
    );
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(outcome.error_code, Some("handoff_allocation_unavailable"));
    assert_eq!(f.calls().len(), 1);
    assert!(f.calls().iter().all(|c| c.provider != "runtime-b"));
    assert_eq!(state(&f).1, None);
}
#[tokio::test]
async fn lr85c_final_h_cancel_after_committed_a_prevents_b() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let cancel = f.cancelled.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" {
                cancel.store(true, Ordering::Release);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Cancelled);
    assert_eq!(f.calls().len(), 1);
    assert_eq!(state(&f).0, "cancelled");
}
#[tokio::test]
async fn lr85c_final_i_cancel_paused_survives_restart_and_resume_is_terminal() {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, false);
    economic_pause(&mut f).await;
    assert!(ContinuationRepository::cancel(&f.db.open().unwrap(), 100).unwrap());
    terminal_fix1_tests::agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.receipts().len(), 1);
    assert_eq!(state(&f).0, "cancelled");
}
#[tokio::test]
async fn lr85c_final_j_checkpoint_failure_rolls_back_result_and_blocks_b() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    db.open().unwrap().execute_batch("CREATE TRIGGER fail_cp BEFORE INSERT ON cognitive_checkpoints BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(f.calls().len(), 1);
    assert!(f.receipts().is_empty());
    assert!(!events.iter().any(|e| e["type"] == "subtask_completed"));
    assert_eq!(
        db.open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM cognitive_continuation_units WHERE result_json IS NOT NULL",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn lr85c_final_k_result_write_failure_rolls_back_checkpoint_and_policy() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.db.open().unwrap().execute_batch("CREATE TRIGGER fail_result BEFORE UPDATE OF result_json ON cognitive_continuation_units BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(f.calls().len(), 1);
    assert!(f.receipts().is_empty());
    assert!(!events.iter().any(|e| e["type"] == "subtask_completed"));
    assert_eq!(
        f.db.open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM checkpoint_task_policies", [], |r| r
                .get::<_, u32>(
                0
            ))
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn lr85c_final_l_restart_real_resume_restores_exact_dependency_without_replaying_a() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let initial_registry = TaskRegistry::default();
    initial_registry.seed_next_id(99);
    assert_eq!(initial_registry.register().unwrap().0, TaskId(100));
    crash_after_a(&mut f).await;
    assert_eq!(f.calls().len(), 1);
    let before = f.receipts()[0].clone();
    drop(initial_registry);
    // Fresh provider/runtime objects, fresh Database and TaskRegistry.
    let fresh = Fixture::new(RoutingMode::Auto, false, false, false);
    f.runtime = fresh.runtime.clone();
    f.calls = fresh.calls.clone();
    let registry = recover(&mut f);
    assert!(f.calls().is_empty());
    let (outcome, events) = resume(&f, registry.clone()).await;
    assert_eq!(outcome, Ok(TaskState::Completed));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.calls()[0].unit, "b");
    assert_eq!(
        f.calls()[0].dependencies.as_deref(),
        Some("--- a [runtime-a] ---\nverified-result-a\n")
    );
    assert_eq!(f.receipts()[0], before);
    assert_eq!(state(&f).0, "completed");
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "subtask_started")
            .count(),
        1
    );
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn lr85c_final_m_x_concurrent_resumes_claim_once_and_do_not_duplicate_b() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    let registry = recover(&mut f);
    let (a, b) = tokio::join!(resume(&f, registry.clone()), resume(&f, registry));
    assert_eq!(
        [a.0, b.0]
            .iter()
            .filter(|r| **r == Ok(TaskState::Completed))
            .count(),
        1
    );
    assert_eq!(f.calls().iter().filter(|c| c.unit == "a").count(), 1);
    assert_eq!(f.calls().iter().filter(|c| c.unit == "b").count(), 1);
}
#[tokio::test]
async fn lr85c_final_n_partial_output_crash_after_a_never_replays_b() {
    let mut f = Fixture::new_recovery_fixture(
        RoutingMode::Auto,
        false,
        false,
        false,
        false,
        false,
        true,
        true,
    );
    let s = f.runtime.scheduler.clone();
    let observed = Arc::new(tokio::sync::Notify::new());
    let marker = observed.clone();
    {
        let run = f.run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
            if e["type"] == "subtask_output_observed" && e["subtask_id"] == "b" {
                marker.notify_one();
            }
        });
        tokio::pin!(run);
        tokio::select! { biased; _ = observed.notified() => {}, _ = &mut run => panic!("partial crash boundary missed") }
    }
    let registry = recover(&mut f);
    assert_eq!(f.calls().len(), 2);
    assert_eq!(state(&f).1.as_deref(), Some("uncertain_execution"));
    assert_eq!(
        resume(&f, registry).await.0,
        Err("continuation_uncertain_execution")
    );
    assert_eq!(f.calls().len(), 2);
    assert_eq!(f.receipts().len(), 1);
    let conn = f.db.open().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT state FROM cognitive_continuation_units WHERE subtask_id='b'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "started"
    );
}
fn replace_effect(f: &Fixture, code: &str) {
    let conn = f.db.open().unwrap();
    let json: String = conn
        .query_row(
            "SELECT checkpoint_json FROM cognitive_checkpoints WHERE source_key='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["effects"] = code.into();
    conn.execute(
        "UPDATE cognitive_checkpoints SET effect_state=?1,checkpoint_json=?2 WHERE source_key='a'",
        rusqlite::params![code, value.to_string()],
    )
    .unwrap();
}
#[tokio::test]
async fn lr85c_final_o_committed_effect_remains_non_replayable_after_restart() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    replace_effect(&f, "committed");
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Completed));
    assert_eq!(f.calls().iter().filter(|c| c.unit == "a").count(), 1);
    assert_eq!(
        f.receipts()[0].replay(),
        ReplayDecision::Forbidden(ReplayReason::EffectCommitted)
    );
}
#[tokio::test]
async fn lr85c_final_p_unknown_effect_stays_uncertain_no_successor() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    replace_effect(&f, "unknown_or_in_flight");
    let registry = recover(&mut f);
    assert_eq!(state(&f).1.as_deref(), Some("uncertain_execution"));
    assert_eq!(
        resume(&f, registry).await.0,
        Err("continuation_uncertain_execution")
    );
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn lr85c_final_q_root_identity_fence_includes_manifest_even_when_corrupt() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    f.db.open()
        .unwrap()
        .execute("UPDATE cognitive_continuations SET manifest_json='{}'", [])
        .unwrap();
    let registry = recover(&mut f);
    assert_eq!(registry.register().unwrap().0, TaskId(101));
    assert_eq!(
        resume(&f, registry).await.0,
        Err("continuation_manifest_invalid")
    );
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn lr85c_final_r_fixed_restart_preserves_explicit_target() {
    let mut f = Fixture::new(RoutingMode::Fixed, false, false, false);
    crash_after_a(&mut f).await;
    f.runtime.scheduler.telemetry.observe_quota(
        "runtime-a",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerMinute,
        Some(100),
        Some(1),
        None,
        Provenance::ProviderHeader,
    );
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Completed));
    assert!(f.calls().iter().all(|c| c.provider == "runtime-a"));
}
#[tokio::test]
async fn lr85c_final_s_preferred_restart_preserves_explicit_order() {
    let mut f = Fixture::new(RoutingMode::Preferred, false, false, false);
    crash_after_a(&mut f).await;
    f.runtime.scheduler.telemetry.observe_quota(
        "runtime-b",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerMinute,
        Some(100),
        Some(1),
        None,
        Provenance::ProviderHeader,
    );
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Completed));
    assert_eq!(
        f.calls()
            .iter()
            .map(|c| c.provider.as_str())
            .collect::<Vec<_>>(),
        ["runtime-a", "runtime-b"]
    );
}
#[tokio::test]
async fn lr85c_final_u_parallel_units_keep_independent_pins_and_results() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, true);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(vec![step("a", &[]), step("b", &[])], move |e| {
            if e["type"] == "subtask_output_observed" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    let calls = f.calls();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].provider, calls[1].provider);
    let loaded = ContinuationRepository::load(&f.db.open().unwrap(), 100).unwrap();
    assert_eq!(loaded.completed.len(), 2);
    for call in calls {
        assert_eq!(
            loaded.completed[&call.unit].result.provider_id,
            call.provider
        );
    }
}
#[tokio::test]
async fn lr85c_final_v_policy_freeze_settings_changes_do_not_authorize_old_task() {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, false);
    economic_pause(&mut f).await;
    let mut conn = f.db.open().unwrap();
    let mut dto = crate::cognition::allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
    dto.paid_use_policy =
        crate::cognition::allocation_policy::PaidUseMode::AllowKnownCostWithinBudget;
    dto.max_paid_currency = Some("USD".into());
    dto.max_paid_micros = Some(100);
    crate::cognition::allocation_policy::save_role_settings(&mut conn, f.snapshot.routing(), &dto)
        .unwrap();
    drop(conn);
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Paused));
    assert_eq!(f.calls().len(), 1);
    f.runtime
        .scheduler
        .telemetry
        .invalidate_provider_quotas("runtime-a");
    assert_eq!(
        resume(&f, Arc::new(TaskRegistry::default())).await.0,
        Ok(TaskState::Completed)
    );
    assert_eq!(f.calls()[1].provider, "runtime-a");
}
#[tokio::test]
async fn lr85c_final_w_selection_score_and_boundary_reason_remain_distinct_and_sanitized() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let (_, events) = f.run(sequential(), |_| {}).await;
    let loaded = ContinuationRepository::load(&f.db.open().unwrap(), 100).unwrap();
    let b = events
        .iter()
        .find(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b")
        .unwrap();
    assert_eq!(b["selection"]["mode"], "auto");
    assert!(b["selection"]["score"].is_i64());
    assert_eq!(
        b["handoff_reason"]["kind"],
        "successor_at_confirmed_completion"
    );
    assert_eq!(
        loaded.completed["b"].selection.score,
        b["selection"]["score"].as_i64()
    );
    let public = b.to_string();
    for excluded in [
        "verified-result",
        "RESULTADOS",
        "header",
        "credential",
        "secret",
    ] {
        assert!(!public.contains(excluded));
    }
}
#[tokio::test]
async fn lr85c_final_y_corrupt_result_or_checkpoint_never_dispatches() {
    for corruption in [
        "UPDATE cognitive_continuation_units SET result_json='{}' WHERE subtask_id='a'",
        "UPDATE cognitive_checkpoints SET checkpoint_json='{}' WHERE source_key='a'",
        "DELETE FROM cognitive_continuation_units WHERE subtask_id='b'",
    ] {
        let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
        crash_after_a(&mut f).await;
        f.db.open().unwrap().execute_batch(corruption).unwrap();
        let registry = recover(&mut f);
        assert_eq!(state(&f).1.as_deref(), Some("invalid_recovery"));
        assert!(resume(&f, registry).await.0.is_err());
        assert_eq!(f.calls().len(), 1);
    }
}
#[tokio::test]
async fn lr85c_final_z_v14_upgrade_preserves_b4_checkpoints_history_and_rate_schema() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.run(vec![step("a", &[])], |_| {}).await;
    let before = f.receipts();
    let conn = f.db.open().unwrap();
    let routing = crate::cognition::policy::load(&conn, CognitiveRole::Worker).unwrap();
    let original_allocation =
        crate::cognition::allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
    task_history::insert(
        &conn,
        &TaskRecord {
            task_id: 500,
            kind: "mock".into(),
            state: "completed".into(),
            started_at: now(),
            finished_at: now(),
            summary: None,
            error_code: None,
        },
    )
    .unwrap();
    use crate::cognition::rate::{
        DailyBudgetPolicy, RateLimitManager, RatePolicy, SystemRateClock,
    };
    let rate = RateLimitManager::new(
        ["runtime-a".into()],
        Arc::new(SystemRateClock::default()),
        Some(f.db.clone()),
    )
    .unwrap();
    rate.set_policy(
        "runtime-a",
        RatePolicy {
            limits: vec![],
            daily_budget: Some(DailyBudgetPolicy {
                anchor_unix_ms: 0,
                max_requests: Some(100),
                max_accounted_tokens: Some(10000),
            }),
        },
    )
    .unwrap();
    let original_rate: String = conn
        .query_row(
            "SELECT local_state FROM cognitive_rate_state WHERE provider_id='runtime-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let rate_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM cognitive_rate_state", [], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute_batch("DROP TABLE cognitive_continuation_units; DROP TABLE cognitive_continuations; PRAGMA user_version=14;").unwrap();
    migrations::apply(&conn).unwrap();
    drop(conn);
    assert_eq!(f.receipts(), before);
    let conn = f.db.open().unwrap();
    assert_eq!(
        crate::cognition::policy::load(&conn, CognitiveRole::Worker).unwrap(),
        routing
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM cognitive_rate_state", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        rate_count
    );
    assert_eq!(
        crate::cognition::allocation_policy::load(&conn, CognitiveRole::Worker).unwrap(),
        original_allocation
    );
    assert_eq!(
        conn.query_row(
            "SELECT state FROM task_records WHERE task_id=500",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "completed"
    );
    assert_eq!(
        conn.query_row(
            "SELECT local_state FROM cognitive_rate_state WHERE provider_id='runtime-a'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        original_rate
    );
    RateLimitManager::new(
        ["runtime-a".into()],
        Arc::new(SystemRateClock::default()),
        Some(f.db.clone()),
    )
    .unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        17
    );
}

#[path = "c4_tests/storage_tests.rs"]
mod storage_tests;

#[path = "c4_tests/terminal_fix1_tests.rs"]
mod terminal_fix1_tests;

#[path = "c4_tests/cancellation_fix2_tests.rs"]
mod cancellation_fix2_tests;
