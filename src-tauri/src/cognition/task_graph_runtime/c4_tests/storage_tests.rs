//! C4 local storage and failure gates, separate from the final A–Z scenarios.
use super::*;

fn loaded(f: &Fixture) -> ContinuationLoad {
    ContinuationRepository::load(&f.db.open().unwrap(), 100).unwrap()
}
fn lease() -> ContinuationLease {
    ContinuationLease {
        root: 100,
        generation: 1,
    }
}

#[tokio::test]
async fn c4_result_retry_is_idempotent_but_conflicting_content_is_rejected() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.run(vec![step("a", &[])], |_| {}).await;
    let saved = loaded(&f).completed.remove("a").unwrap();
    let mut conn = f.db.open().unwrap();
    assert_eq!(
        ContinuationRepository::commit_result(
            &mut conn,
            lease(),
            saved.receipt.checkpoint(),
            &saved.result
        )
        .unwrap(),
        saved.receipt
    );
    let mut conflicting = saved.result.clone();
    conflicting.text = "different useful result".into();
    assert_eq!(
        ContinuationRepository::commit_result(
            &mut conn,
            lease(),
            saved.receipt.checkpoint(),
            &conflicting
        ),
        Err("handoff_checkpoint_write_failed")
    );
    assert_eq!(loaded(&f).completed["a"].result.text, saved.result.text);
}
#[tokio::test]
async fn c4_result_byte_bounds_fail_before_checkpoint_write() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.run(vec![step("a", &[])], |_| {}).await;
    let saved = loaded(&f).completed.remove("a").unwrap();
    for text in [" ".into(), "é".repeat(8193), "x".repeat(16385)] {
        let mut result = saved.result.clone();
        result.text = text;
        let mut conn = f.db.open().unwrap();
        assert_eq!(
            ContinuationRepository::commit_result(
                &mut conn,
                lease(),
                saved.receipt.checkpoint(),
                &result
            ),
            Err("continuation_result_bounds")
        );
    }
    assert_eq!(loaded(&f).completed["a"].result.text, "verified-result-a");
}
#[tokio::test]
async fn c4_result_wrong_unit_or_provider_is_rejected() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.run(vec![step("a", &[])], |_| {}).await;
    let saved = loaded(&f).completed.remove("a").unwrap();
    let mut wrong = saved.result.clone();
    wrong.provider_id = "runtime-b".into();
    assert_eq!(
        ContinuationRepository::commit_result(
            &mut f.db.open().unwrap(),
            lease(),
            saved.receipt.checkpoint(),
            &wrong
        ),
        Err("continuation_result_invalid")
    );
    let mut wrong = saved.result.clone();
    wrong.subtask_id = "b".into();
    assert!(ContinuationRepository::commit_result(
        &mut f.db.open().unwrap(),
        lease(),
        saved.receipt.checkpoint(),
        &wrong
    )
    .is_err());
    assert_eq!(f.receipts().len(), 1);
}
#[tokio::test]
async fn c4_manifest_is_validated_and_identity_cannot_attach_to_old_work() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.run(vec![step("a", &[])], |_| {}).await;
    let mut manifest = loaded(&f).manifest;
    let mut conn = f.db.open().unwrap();
    assert_eq!(
        ContinuationRepository::create(&mut conn, &manifest).unwrap_err(),
        "continuation_identity_conflict"
    );
    manifest.root = 0;
    assert_eq!(
        ContinuationRepository::create(&mut conn, &manifest).unwrap_err(),
        "continuation_identity_invalid"
    );
    manifest.root = 200;
    manifest.steps[0].description = "x".repeat(1025);
    assert_eq!(
        ContinuationRepository::create(&mut conn, &manifest).unwrap_err(),
        "continuation_manifest_invalid"
    );
    manifest.steps[0].description = "bounded unit instruction".into();
    manifest.identity_version = "x".repeat(129);
    assert_eq!(
        ContinuationRepository::create(&mut conn, &manifest).unwrap_err(),
        "continuation_manifest_invalid"
    );
    assert_eq!(task_history::max_id(&conn).unwrap(), 100);
}
#[tokio::test]
async fn c4_manifest_and_all_unit_rows_rollback_atomically() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.db.open().unwrap().execute_batch("CREATE TRIGGER fail_manifest_unit BEFORE INSERT ON cognitive_continuation_units WHEN NEW.subtask_id='b' BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let (outcome, _) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert!(f.calls().is_empty());
    let conn = f.db.open().unwrap();
    for table in ["cognitive_continuations", "cognitive_continuation_units"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
    }
}
#[tokio::test]
async fn c4_dispatch_marker_failure_means_zero_provider_calls() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.db.open().unwrap().execute_batch("CREATE TRIGGER fail_started BEFORE UPDATE OF state ON cognitive_continuation_units WHEN NEW.state='started' BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert!(f.calls().is_empty());
    assert!(!events.iter().any(|e| e["type"] == "subtask_started"));
    assert!(f.receipts().is_empty());
}
#[tokio::test]
async fn c4_pause_write_failure_fails_closed_instead_of_dispatching_b() {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, false);
    let s = f.runtime.scheduler.clone();
    f.db.open().unwrap().execute_batch("CREATE TRIGGER fail_pause BEFORE UPDATE OF state ON cognitive_continuations WHEN NEW.state='paused' BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(outcome.error_code, Some("continuation_write_failed"));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(state(&f).0, "running");
    assert_eq!(f.receipts().len(), 1);
}
#[tokio::test]
async fn c4_claim_write_failure_is_atomic_and_makes_zero_new_calls() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    let registry = recover(&mut f);
    f.db.open().unwrap().execute_batch("CREATE TRIGGER fail_claim BEFORE UPDATE OF state ON cognitive_continuations WHEN NEW.state='running' BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let generation = loaded(&f).generation;
    assert_eq!(
        resume(&f, registry).await.0,
        Err("continuation_claim_failed")
    );
    assert_eq!(f.calls().len(), 1);
    assert_eq!(loaded(&f).generation, generation);
    assert_eq!(state(&f).0, "paused");
}
#[tokio::test]
async fn c4_terminal_state_write_failure_never_reports_completed() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.db.open().unwrap().execute_batch("CREATE TRIGGER fail_terminal BEFORE UPDATE OF state ON cognitive_continuations WHEN NEW.state='completed' BEGIN SELECT RAISE(ABORT,'local test'); END;").unwrap();
    let (outcome, events) = f.run(vec![step("a", &[])], |_| {}).await;
    assert_eq!(outcome.state, TaskState::Paused);
    assert!(outcome.result.is_none());
    assert!(!events.iter().any(|e| e["type"] == "task_completed"));
    assert_eq!(f.receipts().len(), 1);
    let registry = recover(&mut f);
    f.db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_terminal;")
        .unwrap();
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Completed));
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn c4_missing_result_does_not_turn_receipt_into_dispatch_authorization() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    f.db.open().unwrap().execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE cognitive_continuation_units SET result_json=NULL WHERE subtask_id='a';").unwrap();
    let registry = recover(&mut f);
    assert_eq!(state(&f).1.as_deref(), Some("invalid_recovery"));
    assert!(resume(&f, registry).await.0.is_err());
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn c4_result_invalid_utf8_fails_closed() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    f.db.open().unwrap().execute("UPDATE cognitive_continuation_units SET result_json=CAST(x'fffe' AS TEXT) WHERE subtask_id='a'",[]).unwrap();
    let registry = recover(&mut f);
    assert!(resume(&f, registry).await.0.is_err());
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn c4_missing_not_started_row_is_corruption_not_a_new_unit() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    f.db.open()
        .unwrap()
        .execute(
            "DELETE FROM cognitive_continuation_units WHERE subtask_id='b'",
            [],
        )
        .unwrap();
    let registry = recover(&mut f);
    assert!(resume(&f, registry).await.0.is_err());
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn c4_manifest_root_mismatch_is_rejected_without_losing_high_water_mark() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    f.db.open()
        .unwrap()
        .execute(
            "UPDATE cognitive_continuations SET manifest_json=json_set(manifest_json,'$.root',99)",
            [],
        )
        .unwrap();
    let registry = recover(&mut f);
    assert_eq!(registry.reserve_background_id().unwrap(), TaskId(101));
    assert_eq!(
        resume(&f, registry).await.0,
        Err("continuation_identity_invalid")
    );
}
#[tokio::test]
async fn c4_changed_identity_context_pauses_without_replaying_a() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    let registry = recover(&mut f);
    f.db.open()
        .unwrap()
        .execute(
            "UPDATE identity_snapshots SET version='different-local-version' WHERE is_current=1",
            [],
        )
        .unwrap();
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Paused));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(state(&f).1.as_deref(), Some("insufficient_durable_context"));
}
#[tokio::test]
async fn c4_lease_from_before_restart_cannot_start_another_unit() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    recover(&mut f);
    let mut conn = f.db.open().unwrap();
    let (new, _) = ContinuationRepository::claim(&mut conn, 100).unwrap();
    assert_eq!(new.generation, 2);
    assert_eq!(
        finish_terminal_completed(&mut conn, lease()),
        Err("continuation_claim_lost")
    );
    assert_eq!(
        ContinuationRepository::claim(&mut conn, 100).err(),
        Some("continuation_resume_busy")
    );
}
#[test]
fn c4_v15_migration_failure_rolls_back_and_retries_from_v14() {
    let f = Fixture::new(RoutingMode::Auto, false, false, false);
    let conn = f.db.open().unwrap();
    conn.execute_batch("DROP TABLE cognitive_continuation_units; DROP TABLE cognitive_continuations; PRAGMA user_version=14; CREATE TABLE cognitive_continuation_units(dummy INTEGER);").unwrap();
    assert!(migrations::apply(&conn).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        14
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='cognitive_continuations'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TABLE cognitive_continuation_units;")
        .unwrap();
    migrations::apply(&conn).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        17
    );
}
#[test]
fn c4_schema_and_fixtures_do_not_introduce_sensitive_context_fields() {
    let schema = include_str!("../../../../migrations/015_cognitive_continuations.sql");
    for forbidden in [
        "prompt",
        "reasoning",
        "chain_of_thought",
        "credential",
        "api_key",
        "cookie",
        "header",
        "http_body",
    ] {
        assert!(!schema.contains(forbidden));
    }
}

#[tokio::test]
async fn c4_uncertain_branch_blocks_dependents_but_independent_unit_can_resume() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let manifest = ContinuationManifest {
        version: 1,
        root: 100,
        objective: "local recovery gate".into(),
        steps: vec![step("a", &[]), step("b", &["a"]), step("c", &[])],
        policy: f.snapshot.clone(),
        timeouts: f
            .snapshot
            .routing()
            .targets
            .iter()
            .map(|t| {
                (
                    t.provider_id.clone(),
                    crate::cognition::types::ProviderTimeouts {
                        request_timeout_ms: 1000,
                        stream_idle_timeout_ms: 1000,
                    },
                )
            })
            .collect(),
        identity_version: f
            .context
            .as_ref()
            .unwrap()
            .metadata
            .identity_version
            .clone(),
        planner_provider_id: "planner-runtime".into(),
        planner_usage: SchedulerUsage::default(),
    };
    let mut conn = f.db.open().unwrap();
    let owned = ContinuationRepository::create(&mut conn, &manifest).unwrap();
    let targets = f
        .snapshot
        .routing()
        .provider_targets(
            &manifest
                .timeouts
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        )
        .unwrap();
    let allocation = f.snapshot.allocation().unwrap().to_runtime().unwrap();
    let pin = f
        .runtime
        .scheduler
        .ranked_provider_allocations(
            &f.snapshot.routing().selection(),
            &targets,
            Some(&allocation),
        )
        .unwrap()
        .remove(0);
    ContinuationRepository::mark_started(
        &mut conn,
        owned,
        "a",
        ExecutionUnitId::new(100, 1).unwrap(),
        &pin,
        crate::cognition::types::TaskBudget {
            max_provider_calls: 2,
            max_output_tokens: f.snapshot.routing().max_output_tokens.map(|n| n / 2),
        },
    )
    .unwrap();
    drop(conn);
    let registry = recover(&mut f);
    let (result, events) = resume(&f, registry).await;
    assert_eq!(result, Ok(TaskState::Paused));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.calls()[0].unit, "c");
    assert!(events
        .iter()
        .any(|e| e["type"] == "subtask_completed" && e["subtask_id"] == "c"));
    assert!(!events
        .iter()
        .any(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b"));
    assert_eq!(f.receipts().len(), 1);
    assert_eq!(state(&f).1.as_deref(), Some("uncertain_execution"));
}
#[tokio::test]
async fn c4_durable_cancel_between_resume_claim_and_dispatch_wins_without_registry_flag() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    recover(&mut f);
    let mut conn = f.db.open().unwrap();
    let (claimed, _) = ContinuationRepository::claim(&mut conn, 100).unwrap();
    assert!(ContinuationRepository::cancel(&conn, 100).unwrap());
    assert_eq!(finish_terminal_completed(&mut conn, claimed), Ok(true));
    assert_eq!(state(&f).0, "cancelled");
    assert_eq!(f.calls().len(), 1);
}

#[tokio::test]
async fn c4_legacy_terminal_history_contradiction_cannot_resume() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    task_history::insert(
        &f.db.open().unwrap(),
        &TaskRecord {
            task_id: 100,
            kind: "task_graph".into(),
            state: "failed".into(),
            started_at: now(),
            finished_at: now(),
            summary: None,
            error_code: Some("local_failure".into()),
        },
    )
    .unwrap();
    let registry = recover(&mut f);
    assert_eq!(
        resume(&f, registry).await.0,
        Err("continuation_history_contradiction")
    );
    assert_eq!(f.calls().len(), 1);
}
#[tokio::test]
async fn c4_finish_completed_requires_all_exact_receipts_and_results() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    assert_eq!(
        finish_terminal_completed(&mut f.db.open().unwrap(), lease()),
        Err("continuation_state_invalid")
    );
    assert_eq!(state(&f).0, "running");
    assert_eq!(f.calls().len(), 1);
}

#[tokio::test]
async fn c4_cancel_resume_preserves_terminal_state_and_never_started_descendants() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let committed = Arc::new(tokio::sync::Notify::new());
    let done = committed.clone();
    {
        let run = f.run(
            vec![step("a", &[]), step("b", &["a"]), step("c", &["b"])],
            move |e| {
                if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                    done.notify_one();
                }
            },
        );
        tokio::pin!(run);
        tokio::select! { biased; _ = committed.notified() => {}, _ = &mut run => panic!("crash boundary missed") }
    }
    let registry = recover(&mut f);
    let cancel = registry.clone();
    let db = f.db.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body {
            let event: serde_json::Value = serde_json::from_str(&json).unwrap();
            if event["type"] == "subtask_started" && event["subtask_id"] == "b" {
                assert!(cancel.cancel(TaskId(100)));
                assert!(ContinuationRepository::cancel(&db.open().unwrap(), 100).unwrap());
            }
        }
        Ok(())
    });
    assert_eq!(
        resume_task_graph(
            registry.clone(),
            f.db.clone(),
            f.runtime.clone(),
            TaskId(100),
            channel
        )
        .await,
        Ok(TaskState::Cancelled)
    );
    assert_eq!(loaded(&f).state, "cancelled");
    assert_eq!(f.calls().len(), 1);
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(
        f.db.open()
            .unwrap()
            .query_row(
                "SELECT state FROM task_subtask_records WHERE subtask_id='c'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "cancelled"
    );
}

fn finish_terminal_completed(
    conn: &mut rusqlite::Connection,
    lease: ContinuationLease,
) -> Result<bool, &'static str> {
    ContinuationRepository::finish_terminal(
        conn,
        lease,
        TaskRecord {
            task_id: lease.root,
            kind: "task_graph".into(),
            state: "completed".into(),
            started_at: now(),
            finished_at: now(),
            summary: None,
            error_code: None,
        },
        vec![],
    )
}
