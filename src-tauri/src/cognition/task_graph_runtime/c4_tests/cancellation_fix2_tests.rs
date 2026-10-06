//! FIX-2: durable cancel intent is not a terminal lifecycle fact.
use super::terminal_fix1_tests::{agreement, history_counts};
use super::*;
use crate::{luna::cancel_task_core, persistence::checkpoints::*};

fn requested(f: &Fixture) -> bool {
    f.db.open()
        .unwrap()
        .query_row(
            "SELECT cancel_requested FROM cognitive_continuations WHERE root_task_id=100",
            [],
            |r| r.get(0),
        )
        .unwrap()
}
fn cancelled_record() -> TaskRecord {
    TaskRecord {
        task_id: 100,
        kind: "task_graph".into(),
        state: "cancelled".into(),
        started_at: now(),
        finished_at: now(),
        summary: None,
        error_code: Some("cancelled".into()),
    }
}
async fn paused() -> Fixture {
    let mut f = Fixture::new_economic(RoutingMode::Auto, false, false, false, true, false);
    economic_pause(&mut f).await;
    f
}

#[tokio::test]
async fn c4_fix2_a_g_h_paused_command_cancel_is_atomic_before_reopen() {
    let mut f = paused().await;
    let before = f.receipts();
    let registry = Arc::new(TaskRegistry::default());
    assert!(cancel_task_core(&registry, &f.db, TaskId(100))
        .await
        .unwrap());
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    assert!(!requested(&f));
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(f.receipts(), before);
    assert_eq!(f.calls().len(), 1);
}

#[tokio::test]
async fn c4_fix2_b_c_paused_command_history_failures_roll_back_every_terminal_write() {
    for table in ["task_records", "task_subtask_records"] {
        let f = paused().await;
        let before = f.receipts();
        f.db.open().unwrap().execute_batch(&format!(
            "CREATE TRIGGER cancel_history_fault BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'local cancel fault'); END;"
        )).unwrap();
        assert_eq!(
            cancel_task_core(&TaskRegistry::default(), &f.db, TaskId(100)).await,
            Err("task_history_write_failed")
        );
        assert_eq!(
            state(&f),
            ("paused".into(), Some("economic_authorization".into()))
        );
        assert_eq!(history_counts(&f), (0, 0));
        assert_eq!(f.receipts(), before);
        assert_eq!(f.calls().len(), 1);
        f.db.open()
            .unwrap()
            .execute_batch("DROP TRIGGER cancel_history_fault;")
            .unwrap();
        assert!(ContinuationRepository::cancel(&f.db.open().unwrap(), 100).unwrap());
        agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    }
}

#[tokio::test]
async fn c4_fix2_d_cancel_deferred_commit_failure_rolls_back_and_can_retry() {
    let f = paused().await;
    let before = f.receipts();
    f.db.open().unwrap().execute_batch(
        "CREATE TABLE cancel_fault_parent(id INTEGER PRIMARY KEY);
         CREATE TABLE cancel_fault_child(id INTEGER REFERENCES cancel_fault_parent(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER cancel_commit_fault AFTER UPDATE OF state ON cognitive_continuations WHEN NEW.state='cancelled'
         BEGIN INSERT INTO cancel_fault_child VALUES(1); END;"
    ).unwrap();
    assert_eq!(
        cancel_task_core(&TaskRegistry::default(), &f.db, TaskId(100)).await,
        Err("continuation_write_failed")
    );
    assert_eq!(state(&f).0, "paused");
    assert_eq!(history_counts(&f), (0, 0));
    assert_eq!(
        f.db.open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM cancel_fault_child", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(f.receipts(), before);
    f.db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER cancel_commit_fault;")
        .unwrap();
    assert!(ContinuationRepository::cancel(&f.db.open().unwrap(), 100).unwrap());
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
}

// The actual Worker request has started and emitted output. Dropping its future
// after the durable request simulates the crash before the terminal owner writes.
async fn crash_after_running_cancel(f: &mut Fixture) {
    let observed = Arc::new(tokio::sync::Notify::new());
    let done = observed.clone();
    let db = f.db.clone();
    let scheduler = f.runtime.scheduler.clone();
    let run = f.run(sequential(), move |e| {
        if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
            exhaust(&scheduler, "runtime-a", QuotaScope::Provider);
        }
        if e["type"] == "subtask_output_observed" && e["subtask_id"] == "b" {
            assert!(ContinuationRepository::cancel(&db.open().unwrap(), 100).unwrap());
            done.notify_one();
        }
    });
    tokio::pin!(run);
    tokio::select! { biased;
        _ = observed.notified() => {},
        _ = &mut run => panic!("cancel/crash boundary missed"),
    }
}
fn in_flight_fixture() -> Fixture {
    Fixture::new_recovery_fixture(
        RoutingMode::Auto,
        false,
        false,
        false,
        false,
        false,
        true,
        true,
    )
}

#[tokio::test]
async fn c4_fix2_e_running_cancel_intent_survives_crash_and_cannot_resume() {
    let mut f = in_flight_fixture();
    crash_after_running_cancel(&mut f).await;
    assert_eq!(state(&f).0, "running");
    assert!(requested(&f));
    assert_eq!(history_counts(&f), (0, 0));
    assert_eq!(f.calls().len(), 2);
    let before = f.receipts();
    let registry = recover(&mut f);
    assert!(requested(&f));
    assert_eq!(state(&f).0, "paused");
    let loaded = ContinuationRepository::load(&f.db.open().unwrap(), 100).unwrap();
    assert!(loaded.cancel_requested);
    assert!(loaded.uncertain.contains("b"));
    assert_eq!(
        resume(&f, registry.clone()).await.0,
        Err("continuation_cancel_requested")
    );
    assert_eq!(f.calls().len(), 2);
    assert!(cancel_task_core(&registry, &f.db, TaskId(100))
        .await
        .unwrap());
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    assert_eq!(f.receipts(), before);
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(f.calls().len(), 2);
}

#[tokio::test]
async fn c4_fix2_f_late_result_after_cancel_intent_or_terminal_cannot_commit() {
    let mut f = in_flight_fixture();
    crash_after_running_cancel(&mut f).await;
    let loaded = ContinuationRepository::load(&f.db.open().unwrap(), 100).unwrap();
    let a = &loaded.completed["a"];
    let conn = f.db.open().unwrap();
    let (sequence, allocation): (u64, String) = conn.query_row(
        "SELECT unit_sequence,allocation_json FROM cognitive_continuation_units WHERE root_task_id=100 AND subtask_id='b'",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    let wire: serde_json::Value = serde_json::from_str(&allocation).unwrap();
    let id = CheckpointId::new(ExecutionUnitId::new(100, sequence).unwrap(), 1).unwrap();
    let cp = CognitiveCheckpoint::confirmed(
        id,
        ExecutionUnitFacts {
            id: id.unit_id(),
            state: ExecutionUnitState::Completed,
            effects: EffectState::NotStarted,
        },
        HandoffBoundary::ConfirmedCompletion { checkpoint: id },
        None,
        loaded.manifest.policy.clone(),
        AllocationVariant {
            resource_id: ResourceId::new(wire["resourceId"].as_str().unwrap()).unwrap(),
            access_path: AccessPath::new(wire["accessPath"].as_str().unwrap()).unwrap(),
            billing_domain_id: BillingDomainId::new(wire["billingDomainId"].as_str().unwrap())
                .unwrap(),
            model_id: ModelId::new(wire["modelId"].as_str().unwrap()).unwrap(),
            effort: wire["effort"].as_str().map(|v| EffortId::new(v).unwrap()),
        },
        CheckpointProvenance {
            source: ExecutionSource::subtask("b").unwrap(),
            runtime_id: RuntimeId::new("runtime-b").unwrap(),
        },
        HandoffContext::new(vec![a.receipt.checkpoint().id()]).unwrap(),
    )
    .unwrap();
    let mut result = a.result.clone();
    result.subtask_id = "b".into();
    result.provider_id = "runtime-b".into();
    result.usage.providers_used = vec!["runtime-b".into()];
    result.text = "verified-result-b".into();
    let lease = ContinuationLease {
        root: 100,
        generation: loaded.generation,
    };
    let before = f.receipts();
    assert_eq!(
        ContinuationRepository::commit_result(&mut f.db.open().unwrap(), lease, &cp, &result),
        Err("handoff_checkpoint_write_failed")
    );
    assert_eq!(f.receipts(), before);
    assert_eq!(
        ContinuationRepository::finish_terminal(
            &mut f.db.open().unwrap(),
            lease,
            cancelled_record(),
            vec![]
        ),
        Ok(true)
    );
    assert_eq!(
        ContinuationRepository::commit_result(&mut f.db.open().unwrap(), lease, &cp, &result),
        Err("handoff_checkpoint_write_failed")
    );
    assert_eq!(f.receipts(), before);
    assert_eq!(conn.query_row("SELECT result_json FROM cognitive_continuation_units WHERE root_task_id=100 AND subtask_id='b'", [], |r| r.get::<_, Option<String>>(0)).unwrap(), None);
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(f.calls().len(), 2);
}

#[tokio::test]
async fn c4_fix2_i_concurrent_paused_and_running_cancels_are_idempotent() {
    let mut f = paused().await;
    for running in [false, true] {
        if running {
            f = Fixture::new(RoutingMode::Auto, false, false, false);
            crash_after_a(&mut f).await;
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let mut joins = vec![];
        for _ in 0..2 {
            let db = f.db.clone();
            let barrier = barrier.clone();
            joins.push(std::thread::spawn(move || {
                let conn = db.open().unwrap();
                barrier.wait();
                ContinuationRepository::cancel(&conn, 100)
            }));
        }
        for join in joins {
            assert_eq!(join.join().unwrap(), Ok(true));
        }
        if running {
            assert_eq!(state(&f).0, "running");
            assert!(requested(&f));
            assert_eq!(history_counts(&f), (0, 0));
            recover(&mut f);
            assert!(ContinuationRepository::cancel(&f.db.open().unwrap(), 100).unwrap());
        }
        agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
        assert!(ContinuationRepository::cancel(&f.db.open().unwrap(), 100).unwrap());
        assert_eq!(history_counts(&f), (1, 2));
        assert_eq!(f.calls().len(), 1);
    }
}

#[tokio::test]
async fn c4_fix2_j_completion_committed_before_cancel_is_not_rewritten() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    assert_eq!(
        f.run(sequential(), |_| {}).await.0.state,
        TaskState::Completed
    );
    assert!(
        !cancel_task_core(&TaskRegistry::default(), &f.db, TaskId(100))
            .await
            .unwrap()
    );
    agreement(&f, "completed", &[("a", "completed"), ("b", "completed")]);
    assert!(!requested(&f));
    assert_eq!(f.calls().len(), 2);
    // The other serialization order is exercised by FIX-1's cancellation after
    // the last result and before the terminal writer, now using durable intent.
}

#[tokio::test]
async fn c4_fix2_running_command_persists_intent_and_signals_atomic_flag() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    let registry = TaskRegistry::default();
    let flag = registry.register_existing(TaskId(100)).unwrap();
    assert!(cancel_task_core(&registry, &f.db, TaskId(100))
        .await
        .unwrap());
    assert!(flag.load(Ordering::Acquire));
    assert!(requested(&f));
    assert_eq!(state(&f).0, "running");
    assert_eq!(history_counts(&f), (0, 0));
    assert_eq!(
        ContinuationRepository::finish_terminal(
            &mut f.db.open().unwrap(),
            ContinuationLease {
                root: 100,
                generation: 1
            },
            cancelled_record(),
            vec![]
        ),
        Ok(true)
    );
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
}

#[tokio::test]
async fn c4_fix2_v15_upgrade_preserves_ledger_policy_history_and_rate_state() {
    let mut f = paused().await;
    let conn = f.db.open().unwrap();
    task_history::insert(
        &conn,
        &TaskRecord {
            task_id: 500,
            kind: "mock".into(),
            state: "completed".into(),
            started_at: now(),
            finished_at: now(),
            summary: Some("preserved-local-history".into()),
            error_code: None,
        },
    )
    .unwrap();
    use crate::cognition::rate::{
        DailyBudgetPolicy, RateLimitManager, RatePolicy, SystemRateClock,
    };
    let manager = RateLimitManager::new(
        ["runtime-a".into()],
        Arc::new(SystemRateClock::default()),
        Some(f.db.clone()),
    )
    .unwrap();
    manager
        .set_policy(
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
    let receipts = f.receipts();
    let manifest: String = conn
        .query_row(
            "SELECT manifest_json FROM cognitive_continuations WHERE root_task_id=100",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let history: Vec<(u64,String,String,String,String,Option<String>,Option<String>)> = conn.prepare("SELECT task_id,kind,state,started_at,finished_at,summary,error_code FROM task_records ORDER BY task_id").unwrap().query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap().map(Result::unwrap).collect();
    assert!(!history.is_empty());
    let rate: Vec<(String, String)> = conn
        .prepare("SELECT provider_id,local_state FROM cognitive_rate_state ORDER BY provider_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!rate.is_empty());
    let policy = crate::cognition::allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
    conn.execute_batch(
        "ALTER TABLE cognitive_continuations DROP COLUMN cancel_requested; PRAGMA user_version=15;",
    )
    .unwrap();
    migrations::apply(&conn).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        16
    );
    assert_eq!(
        conn.query_row(
            "SELECT manifest_json FROM cognitive_continuations WHERE root_task_id=100",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        manifest
    );
    assert_eq!(
        conn.prepare("SELECT task_id,kind,state,started_at,finished_at,summary,error_code FROM task_records ORDER BY task_id").unwrap().query_map([], |r| Ok((r.get::<_,u64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?))).unwrap().map(Result::unwrap).collect::<Vec<_>>(),
        history
    );
    assert_eq!(
        crate::cognition::allocation_policy::load(&conn, CognitiveRole::Worker).unwrap(),
        policy
    );
    assert_eq!(
        conn.prepare(
            "SELECT provider_id,local_state FROM cognitive_rate_state ORDER BY provider_id"
        )
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>(),
        rate
    );
    assert!(!requested(&f));
    drop(conn);
    recover(&mut f);
    assert_eq!(f.receipts(), receipts);
}

#[tokio::test]
async fn c4_fix2_v16_migration_failure_does_not_advance_version_and_retry_succeeds() {
    let f = paused().await;
    let conn = f.db.open().unwrap();
    let before = f.receipts();
    // A conflicting column forces ALTER failure; the transaction and version
    // must roll back. Removing the conflict models correction before retry.
    conn.pragma_update(None, "user_version", 15).unwrap();
    assert!(migrations::apply(&conn).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        15
    );
    assert_eq!(
        conn.query_row(
            "SELECT state FROM cognitive_continuations WHERE root_task_id=100",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "paused"
    );
    conn.execute_batch("ALTER TABLE cognitive_continuations DROP COLUMN cancel_requested;")
        .unwrap();
    migrations::apply(&conn).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        16
    );
    assert_eq!(f.receipts(), before);
    assert!(!requested(&f));
    assert!(conn
        .execute(
            "UPDATE cognitive_continuations SET cancel_requested=2 WHERE root_task_id=100",
            []
        )
        .is_err());
}

#[tokio::test]
async fn c4_fix2_j_cancel_intent_before_completion_transaction_wins() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "b" {
                let conn = db.open().unwrap();
                assert_eq!(
                    conn.query_row(
                        "SELECT state FROM cognitive_continuations WHERE root_task_id=100",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "running"
                );
                assert!(ContinuationRepository::cancel(&conn, 100).unwrap());
                assert_eq!(
                    conn.query_row(
                        "SELECT COUNT(*) FROM task_records WHERE task_id=100",
                        [],
                        |r| r.get::<_, u32>(0)
                    )
                    .unwrap(),
                    0
                );
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Cancelled);
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "completed")]);
    let mut record = cancelled_record();
    record.state = "completed".into();
    assert_eq!(
        ContinuationRepository::finish_terminal(
            &mut f.db.open().unwrap(),
            ContinuationLease {
                root: 100,
                generation: 1
            },
            record,
            vec![]
        ),
        Ok(true)
    );
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "completed")]);
    assert_eq!(f.calls().len(), 2);
}

#[tokio::test]
async fn c4_fix2_running_command_write_failure_is_not_reported_as_durable_success() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    let registry = TaskRegistry::default();
    let flag = registry.register_existing(TaskId(100)).unwrap();
    f.db.open().unwrap().execute_batch("CREATE TRIGGER cancel_request_fault BEFORE UPDATE OF cancel_requested ON cognitive_continuations WHEN NEW.cancel_requested=1 BEGIN SELECT RAISE(ABORT,'local intent fault'); END;").unwrap();
    assert_eq!(
        cancel_task_core(&registry, &f.db, TaskId(100)).await,
        Err("continuation_write_failed")
    );
    assert!(flag.load(Ordering::Acquire));
    assert!(!requested(&f));
    assert_eq!(state(&f).0, "running");
    assert_eq!(history_counts(&f), (0, 0));
    assert_eq!(f.calls().len(), 1);
}

#[tokio::test]
async fn c4_fix2_cancel_preserves_committed_unit_with_unknown_effect_without_normalizing() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    crash_after_a(&mut f).await;
    replace_effect(&f, "unknown_or_in_flight");
    recover(&mut f);
    let before = f.receipts();
    assert!(ContinuationRepository::cancel(&f.db.open().unwrap(), 100).unwrap());
    agreement(&f, "cancelled", &[("a", "completed"), ("b", "cancelled")]);
    assert_eq!(f.receipts(), before);
    assert_eq!(
        f.receipts()[0].checkpoint().effects(),
        EffectState::UnknownOrInFlight
    );
    let registry = recover(&mut f);
    assert_eq!(resume(&f, registry).await.0, Err("continuation_terminal"));
    assert_eq!(f.calls().len(), 1);
}
