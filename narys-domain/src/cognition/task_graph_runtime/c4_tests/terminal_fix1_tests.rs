//! C4 FIX-1: terminal continuation and history form one durable SQLite fact.
use super::*;

pub(super) fn history_counts(f: &Fixture) -> (u32, u32) {
    f.db.open().unwrap().query_row(
        "SELECT (SELECT COUNT(*) FROM task_records WHERE task_id=100), (SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=100)",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap()
}
pub(super) fn agreement(f: &Fixture, expected: &str, units: &[(&str, &str)]) {
    let conn = f.db.open().unwrap();
    let states: (String, String) = conn.query_row(
        "SELECT c.state,t.state FROM cognitive_continuations c JOIN task_records t ON t.task_id=c.root_task_id WHERE c.root_task_id=100",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(states, (expected.into(), expected.into()));
    let rows: Vec<(String, String)> = conn.prepare(
        "SELECT subtask_id,state FROM task_subtask_records WHERE root_task_id=100 ORDER BY subtask_id",
    ).unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        rows,
        units
            .iter()
            .map(|(id, s)| (id.to_string(), s.to_string()))
            .collect::<Vec<_>>()
    );
}
fn assert_recoverable(f: &Fixture, receipts: usize) {
    assert_eq!(
        state(f),
        ("paused".into(), Some("recovery_required".into()))
    );
    assert_eq!(history_counts(f), (0, 0));
    assert_eq!(f.receipts().len(), receipts);
    let loaded = ContinuationRepository::load(&f.db.open().unwrap(), 100).unwrap();
    assert_eq!(loaded.completed.len(), receipts);
    assert!(loaded.uncertain.is_empty());
}

#[tokio::test]
async fn c4_fix1_c_d_resume_history_rollback_then_recovery_finalizes_without_replay() {
    for table in ["task_records", "task_subtask_records"] {
        let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
        crash_after_a(&mut f).await;
        let before = f.receipts();
        let registry = recover(&mut f);
        f.db.open().unwrap().execute_batch(&format!(
            "CREATE TRIGGER fail_history BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'local terminal fault'); END;"
        )).unwrap();
        let (result, events) = resume(&f, registry).await;
        assert_eq!(result, Ok(TaskState::Paused));
        assert!(!events.iter().any(|e| e["type"] == "task_completed"));
        assert_recoverable(&f, 2);
        assert_eq!(f.receipts()[0], before[0]);
        assert_eq!(f.calls().iter().filter(|c| c.unit == "a").count(), 1);
        assert_eq!(f.calls().iter().filter(|c| c.unit == "b").count(), 1);
        let committed = f.receipts();
        f.db.open()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_history;")
            .unwrap();
        let registry = recover(&mut f);
        let (result, events) = resume(&f, registry.clone()).await;
        assert_eq!(result, Ok(TaskState::Completed));
        assert_eq!(f.calls().len(), 2);
        assert_eq!(f.receipts(), committed);
        assert!(!events.iter().any(|e| e["type"] == "subtask_started"));
        assert_eq!(
            events
                .iter()
                .filter(|e| e["type"] == "task_completed")
                .count(),
            1
        );
        agreement(&f, "completed", &[("a", "completed"), ("b", "completed")]);
        assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
        assert_eq!(f.calls().len(), 2);
    }
}

#[tokio::test]
async fn c4_fix1_e_durable_cancel_wins_over_completed_without_registry_flag() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                assert!(ContinuationRepository::cancel(&db.open().unwrap(), 100).unwrap());
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Cancelled);
    assert_eq!(f.calls().len(), 1);
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    assert_eq!(
        ContinuationRepository::load(&f.db.open().unwrap(), 100)
            .unwrap()
            .state,
        "cancelled"
    );
}

#[tokio::test]
async fn c4_fix1_e_resume_terminal_writer_observes_cancel_committed_after_last_result() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    let registry = recover(&mut f);
    let db = f.db.clone();
    let tx = Channel::new(move |body| {
        if let crate::channel::InvokeResponseBody::Json(json) = body {
            let event: serde_json::Value = serde_json::from_str(&json).unwrap();
            if event["type"] == "subtask_completed" && event["subtask_id"] == "b" {
                assert!(ContinuationRepository::cancel(&db.open().unwrap(), 100).unwrap());
            }
        }
        Ok(())
    });
    assert_eq!(
        resume_task_graph(registry, f.db.clone(), f.runtime.clone(), TaskId(100), tx).await,
        Ok(TaskState::Cancelled)
    );
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "completed")]);
    assert_eq!(f.calls().len(), 2);
}

#[tokio::test]
async fn c4_fix1_f_completed_cancelled_failed_have_exact_terminal_history() {
    let mut completed = Fixture::new(RoutingMode::Auto, false, false, false);
    assert_eq!(
        completed.run(sequential(), |_| {}).await.0.state,
        TaskState::Completed
    );
    agreement(
        &completed,
        "completed",
        &[("a", "completed"), ("b", "completed")],
    );

    let mut cancelled = Fixture::new(RoutingMode::Auto, false, false, false);
    let flag = cancelled.cancelled.clone();
    assert_eq!(
        cancelled
            .run(sequential(), move |e| {
                if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                    flag.store(true, Ordering::Release);
                }
            })
            .await
            .0
            .state,
        TaskState::Cancelled
    );
    agreement(
        &cancelled,
        "cancelled",
        &[("a", "completed"), ("b", "cancelled")],
    );
    assert_eq!(cancelled.calls().len(), 1);

    let mut failed = Fixture::new(RoutingMode::Auto, false, true, false);
    assert_eq!(
        failed.run(sequential(), |_| {}).await.0.state,
        TaskState::Failed
    );
    agreement(&failed, "failed", &[("a", "failed"), ("b", "blocked")]);
    assert_eq!(failed.calls().len(), 1);
}

#[tokio::test]
async fn c4_fix1_g_deferred_fk_commit_failure_rolls_back_history_and_terminal_state() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.db.open().unwrap().execute_batch(
        "CREATE TABLE terminal_fault_parent(id INTEGER PRIMARY KEY);
         CREATE TABLE terminal_fault_child(id INTEGER REFERENCES terminal_fault_parent(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER fail_terminal_commit AFTER UPDATE OF state ON cognitive_continuations WHEN NEW.state='completed'
         BEGIN INSERT INTO terminal_fault_child VALUES(1); END;"
    ).unwrap();
    let (outcome, _) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Paused);
    assert_eq!(outcome.error_code, Some("continuation_write_failed"));
    assert_recoverable(&f, 2);
    assert_eq!(
        f.db.open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM terminal_fault_child", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    let receipts = f.receipts();
    f.db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_terminal_commit;")
        .unwrap();
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Completed));
    assert_eq!(f.receipts(), receipts);
    assert_eq!(f.calls().len(), 2);
    agreement(&f, "completed", &[("a", "completed"), ("b", "completed")]);
}

#[tokio::test]
async fn c4_fix1_rollback_with_pause_failure_leaves_running_for_startup_recovery() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    f.db.open().unwrap().execute_batch(
        "CREATE TRIGGER fail_history BEFORE INSERT ON task_records BEGIN SELECT RAISE(ABORT,'local fault'); END;
         CREATE TRIGGER fail_pause BEFORE UPDATE OF state ON cognitive_continuations WHEN NEW.state='paused' BEGIN SELECT RAISE(ABORT,'local fault'); END;"
    ).unwrap();
    let (outcome, _) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(outcome.error_code, Some("task_history_write_failed"));
    assert_eq!(state(&f).0, "running");
    assert_eq!(history_counts(&f), (0, 0));
    assert_eq!(f.receipts().len(), 2);
    f.db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_pause; DROP TRIGGER fail_history;")
        .unwrap();
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Ok(TaskState::Completed));
    assert_eq!(f.calls().len(), 2);
    agreement(&f, "completed", &[("a", "completed"), ("b", "completed")]);
}

#[tokio::test]
async fn c4_fix1_e_cancel_wins_over_pending_economic_pause_and_commits_history() {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, false);
    let db = f.db.clone();
    let scheduler = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(&scheduler, "runtime-a", QuotaScope::Provider);
                assert!(ContinuationRepository::cancel(&db.open().unwrap(), 100).unwrap());
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Cancelled);
    assert_eq!(f.calls().len(), 1);
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
}
