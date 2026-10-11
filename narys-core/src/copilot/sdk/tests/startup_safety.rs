//! FIX-1 uses the official SDK, real private reapers and a local synthetic peer.
//! Every fault asserts both supervisor memory and authoritative SQLite receipts.
use super::*;
use crate::copilot::{startup::StartupJournal, CopilotLifecycle};
use crate::luna::runtime::TaskRegistry;
use std::sync::atomic::AtomicBool;

struct SyntheticFactory(SdkRuntimeFactory);
impl RuntimeFactory for SyntheticFactory {
    fn start(&self) -> StartupFuture<'_> {
        Box::pin(self.0.start_owned())
    }
}
fn service(f: SdkRuntimeFactory) -> Arc<CopilotLifecycle> {
    CopilotLifecycle::new(
        f.database.clone(),
        Arc::new(TaskRegistry::default()),
        f.root.parent().unwrap().join("sessions"),
        Arc::new(SyntheticFactory(f)),
    )
    .unwrap()
}
fn owner(db: &crate::persistence::database::Database) -> Value {
    db.open().unwrap().query_row("SELECT runtime_ref,state,cleanup_verified,error_code,owner_json,cleanup_json,private_directory FROM agent_runtime_owners ORDER BY created_at,runtime_ref LIMIT 1",[],|r| Ok(serde_json::json!({
        "runtime_ref":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"cleanup_verified":r.get::<_,bool>(2)?,"error_code":r.get::<_,Option<String>>(3)?,
        "owner":r.get::<_,Option<String>>(4)?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null),
        "proof":r.get::<_,Option<String>>(5)?.and_then(|s|serde_json::from_str::<Value>(&s).ok()),
        "directory":r.get::<_,String>(6)?}))).unwrap()
}
async fn demand(s: &Arc<CopilotLifecycle>) -> Value {
    let admitted = s.admit(AgentLifecycleOperation::Create, None).unwrap();
    task_finished(s, admitted["task_id"].as_u64().unwrap()).await
}
async fn task_finished(s: &Arc<CopilotLifecycle>, id: u64) -> Value {
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let task = crate::copilot::store::task(&s.database.open().unwrap(), id).unwrap();
            if !matches!(task["state"].as_str(), Some("pending" | "running"))
                && s.status()["active_tasks"] == 0
            {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
fn assert_failed(
    s: &CopilotLifecycle,
    db: &crate::persistence::database::Database,
    task: &Value,
    kind: StartupSafety,
    phase: &str,
) {
    assert_eq!(task["state"], "failed");
    let v = s.supervisor.snapshot();
    let failure = v.startup_failure.unwrap();
    assert_eq!(failure.safety, kind);
    assert_eq!(
        v.state,
        crate::agents::lifecycle::AgentRuntimeState::Faulted
    );
    assert_eq!(v.cleanup_verified, failure.cleanup_verified());
    assert_eq!(task["cleanup_verified"], v.cleanup_verified);
    let o = owner(db);
    assert_eq!(o["cleanup_verified"], v.cleanup_verified);
    assert_eq!(o["runtime_ref"], failure.runtime_ref.unwrap());
    assert_eq!(o["owner"]["startup"]["phase"], phase);
    assert_eq!(task["error_code"], failure.code);
    assert_eq!(o["owner"]["startup"]["original_error"], failure.code);
    let conn = db.open().unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM agent_sessions", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM agent_runs", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM agent_runtime_owners", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT state FROM agent_sessions", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "failed"
    );
}
#[tokio::test]
async fn preparation_workspace_logs_sdk_state_failures_are_durable_no_launch_and_retry_safe() {
    for point in [
        StartupPoint::Workspace,
        StartupPoint::Logs,
        StartupPoint::SdkState,
    ] {
        let d = tempfile::tempdir().unwrap();
        let mut f = fixture(d.path(), "normal");
        let db = f.database.clone();
        let once = Arc::new(AtomicBool::new(false));
        f.fault = Some(Arc::new(move |at, dir, db| {
            if at == point && !once.swap(true, Ordering::SeqCst) {
                let o = owner(db);
                assert_eq!(o["state"], "starting");
                assert_eq!(o["owner"]["startup"]["phase"], "preparing");
                assert_eq!(o["cleanup_verified"], false);
                let blocked = match point {
                    StartupPoint::Workspace => dir.join("workspace"),
                    StartupPoint::Logs => dir.join("logs"),
                    _ => dir.parent().unwrap().parent().unwrap().join("sdk-state"),
                };
                fs::write(blocked, b"synthetic blocking file").unwrap();
            }
            Ok(())
        }));
        let s = service(f);
        let failed = demand(&s).await;
        assert_failed(
            &s,
            &db,
            &failed,
            StartupSafety::NoProcessLaunched,
            "failed_before_launch",
        );
        let o = owner(&db);
        assert_eq!(o["state"], "stopped");
        assert_eq!(o["proof"]["no_process_launched"], true);
        assert!(!d.path().join("counts.json").exists());
        assert!(!Path::new(o["directory"].as_str().unwrap())
            .join("primary-owner.json")
            .exists());
        if point == StartupPoint::SdkState {
            fs::remove_file(d.path().join("copilot/sdk-state")).unwrap();
        }
        // No recovery or implicit resend is needed for positively concluded preparation.
        let completed = demand(&s).await;
        assert_eq!(completed["state"], "completed");
        assert_ne!(completed["task_id"], failed["task_id"]);
        assert_ne!(completed["session_ref"], failed["session_ref"]);
        assert_eq!(counts(d.path())["create"], 1);
        assert_eq!(counts(d.path())["send"], 0);
        assert_eq!(s.supervisor.snapshot().generation, 2);
        recover(&db).await.unwrap();
        recover(&db).await.unwrap();
        assert_eq!(
            db.open()
                .unwrap()
                .query_row("SELECT count(*) FROM agent_runs", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            2
        );
    }
}
#[tokio::test]
async fn insert_persistence_failure_blocks_memory_and_reconciles_empty_durable_intent_without_launch(
) {
    let d = tempfile::tempdir().unwrap();
    let mut f = fixture(d.path(), "normal");
    let db = f.database.clone();
    f.fault = Some(Arc::new(|at, _, db| {
        if at == StartupPoint::PersistPreparing {
            db.open().unwrap().execute_batch("CREATE TRIGGER IF NOT EXISTS fail_start_insert BEFORE INSERT ON agent_runtime_owners BEGIN SELECT RAISE(ABORT,'fixture-private-not-diagnostic'); END;").unwrap();
        }
        Ok(())
    }));
    let s = service(f);
    let task = demand(&s).await;
    assert_eq!(task["error_code"], "runtime_ownership_persist_failed");
    assert_eq!(task["cleanup_verified"], false);
    assert_eq!(
        s.supervisor.snapshot().startup_failure.unwrap().safety,
        StartupSafety::PersistenceUncertain
    );
    assert_eq!(
        db.open()
            .unwrap()
            .query_row("SELECT count(*) FROM agent_runtime_owners", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        fs::read_dir(d.path().join("copilot/runtimes"))
            .unwrap()
            .count(),
        0
    );
    assert!(!d.path().join("counts.json").exists());
    let blocked = demand(&s).await;
    assert_eq!(blocked["error_code"], "cleanup_not_verified");
    assert_eq!(s.supervisor.snapshot().generation, 1);
    // Repair only the injected test trigger; no production safety gate is changed.
    db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_start_insert;")
        .unwrap();
    let fresh = service(fixture(d.path(), "normal"));
    fresh.recover_runtime_ownership().await.unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    let next = demand(&fresh).await;
    assert_eq!(next["state"], "completed");
    assert!(next["task_id"].as_u64().unwrap() > blocked["task_id"].as_u64().unwrap());
    assert_eq!(counts(d.path())["create"], 1);
    assert_eq!(counts(d.path())["send"], 0);
}
#[tokio::test]
async fn launch_intent_and_terminal_persistence_faults_never_bless_cleanup() {
    for phase in ["launch_intent", "failed_before_launch"] {
        let d = tempfile::tempdir().unwrap();
        let mut f = fixture(d.path(), "normal");
        let db = f.database.clone();
        let target = phase;
        f.fault = Some(Arc::new(move |at, _, db| {
            if at == StartupPoint::PersistLaunchIntent {
                db.open().unwrap().execute_batch(&format!("CREATE TRIGGER fail_state_write BEFORE UPDATE ON agent_runtime_owners WHEN json_extract(NEW.owner_json,'$.startup.phase')='{target}' BEGIN SELECT RAISE(ABORT,'fixture'); END;")).unwrap();
                if target == "failed_before_launch" {
                    return Err("fixture_intent_write_failure");
                }
            }
            Ok(())
        }));
        let s = service(f);
        let task = demand(&s).await;
        assert_eq!(task["cleanup_verified"], false);
        let snap = s.supervisor.snapshot();
        assert_eq!(
            snap.startup_failure.unwrap().safety,
            StartupSafety::PersistenceUncertain
        );
        assert!(!snap.cleanup_verified);
        assert!(!d.path().join("counts.json").exists());
        let durable = owner(&db);
        assert_eq!(durable["cleanup_verified"], false);
        // When the terminal writes themselves are rejected, the last committed
        // preparing row remains pending/unsafe; the linked run records the error.
        assert_eq!(
            durable["state"],
            if phase == "failed_before_launch" {
                "starting"
            } else {
                "faulted"
            }
        );
        if phase == "failed_before_launch" {
            assert_eq!(durable["owner"]["startup"]["phase"], "preparing");
        }
        let blocked = demand(&s).await;
        assert_eq!(blocked["error_code"], "cleanup_not_verified");
        db.open()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_state_write")
            .unwrap();
        let fresh = service(fixture(d.path(), "normal"));
        fresh.recover_runtime_ownership().await.unwrap();
        fresh.recover_runtime_ownership().await.unwrap();
        assert_eq!(owner(&db)["cleanup_verified"], true);
        assert_eq!(
            db.open()
                .unwrap()
                .query_row("SELECT count(*) FROM agent_runs", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            2
        );
        assert_eq!(demand(&fresh).await["state"], "completed");
    }
}
#[tokio::test]
async fn failure_immediately_before_sdk_invocation_concludes_durable_intent_as_no_launch() {
    let d = tempfile::tempdir().unwrap();
    let mut f = fixture(d.path(), "normal");
    let db = f.database.clone();
    f.fault = Some(Arc::new(|at, _, db| {
        if at == StartupPoint::BeforeLaunch {
            assert_eq!(owner(db)["owner"]["startup"]["phase"], "launch_intent");
            return Err("fixture_before_launch");
        }
        Ok(())
    }));
    let s = service(f);
    let task = demand(&s).await;
    assert_failed(
        &s,
        &db,
        &task,
        StartupSafety::NoProcessLaunched,
        "failed_before_launch",
    );
    assert!(!d.path().join("counts.json").exists());
    let fresh = service(fixture(d.path(), "normal"));
    fresh.recover_runtime_ownership().await.unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    assert_eq!(demand(&fresh).await["state"], "completed");
    assert_eq!(counts(d.path())["create"], 1);
}
#[tokio::test]
async fn launched_failure_requires_kernel_proof_and_restart_never_replays_or_signals_external() {
    let d = tempfile::tempdir().unwrap();
    let mut f = fixture(d.path(), "descendant");
    let db = f.database.clone();
    let mut external = std::process::Command::new("/usr/bin/sleep")
        .arg("60")
        .spawn()
        .unwrap();
    f.fault = Some(Arc::new(|at, _, _| {
        if at == StartupPoint::AfterLaunch {
            Err("fixture_after_launch")
        } else {
            Ok(())
        }
    }));
    let s = service(f);
    let task = demand(&s).await;
    assert_failed(
        &s,
        &db,
        &task,
        StartupSafety::CleanupVerified,
        "failed_after_launch",
    );
    let o = owner(&db);
    assert_eq!(o["proof"]["kernel_children_exhausted"], true);
    assert!(!record_alive(Path::new(o["directory"].as_str().unwrap())).unwrap());
    let descendant = fs::read_to_string(d.path().join("descendant.pid")).unwrap();
    assert!(!Path::new(&format!("/proc/{}", descendant.trim())).exists());
    assert_eq!(counts(d.path())["create"], 0);
    assert_eq!(counts(d.path())["send"], 0);
    let fresh = service(fixture(d.path(), "normal"));
    fresh.recover_runtime_ownership().await.unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    assert_eq!(
        db.open()
            .unwrap()
            .query_row("SELECT count(*) FROM agent_runs", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(counts(d.path())["create"], 0);
    assert!(external.try_wait().unwrap().is_none());
    assert_eq!(demand(&fresh).await["state"], "completed");
    assert_eq!(counts(d.path())["create"], 1);
    external.kill().unwrap();
    external.wait().unwrap();
}
#[tokio::test]
async fn launched_unverified_cleanup_blocks_new_generations_until_explicit_positive_recovery() {
    let d = tempfile::tempdir().unwrap();
    let mut f = fixture(d.path(), "normal");
    let db = f.database.clone();
    f.fault = Some(Arc::new(|at, dir, _| {
        if at == StartupPoint::AfterLaunch {
            // Simulates a contradictory independent-primary cleanup receipt.
            fs::write(
                dir.join("primary-cleanup.json"),
                b"{\"cleanup_complete\":false}",
            )
            .unwrap();
            return Err("fixture_after_launch");
        }
        Ok(())
    }));
    let s = service(f);
    let task = demand(&s).await;
    assert_failed(
        &s,
        &db,
        &task,
        StartupSafety::CleanupUnverified,
        "failed_after_launch",
    );
    assert_eq!(owner(&db)["state"], "faulted");
    assert_eq!(demand(&s).await["error_code"], "cleanup_not_verified");
    assert_eq!(s.supervisor.snapshot().generation, 1);
    let fresh_factory = fixture(d.path(), "normal");
    let failed = fresh_factory.start_owned().await.err().unwrap();
    assert_eq!(failed.code, "runtime_previous_ownership_unresolved");
    assert!(!failed.cleanup_verified());
    let fresh = service(fresh_factory);
    assert_eq!(
        fresh.recover_runtime_ownership().await.err(),
        Some("runtime_cleanup_incomplete")
    );
    assert_eq!(
        fresh.recover_runtime_ownership().await.err(),
        Some("runtime_cleanup_incomplete")
    );
    assert!(!fresh.supervisor.snapshot().cleanup_verified);
    assert_eq!(owner(&db)["cleanup_verified"], false);
    assert_eq!(counts(d.path())["create"], 0);
    let directory = owner(&db)["directory"].as_str().unwrap().to_owned();
    fs::remove_file(Path::new(&directory).join("primary-cleanup.json")).unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    assert_eq!(owner(&db)["cleanup_verified"], true);
    assert_eq!(counts(d.path())["create"], 0);
    assert_eq!(demand(&fresh).await["state"], "completed");
    assert_eq!(counts(d.path())["create"], 1);
    let conn = db.open().unwrap();
    assert_eq!(
        conn.query_row("SELECT count(DISTINCT task_id) FROM agent_runs", [], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM agent_runtime_owners", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        2
    );
}
#[tokio::test]
async fn operational_and_cleanup_persistence_failures_preserve_original_error_and_require_reconciliation(
) {
    for point in [StartupPoint::PersistReady, StartupPoint::AfterLaunch] {
        let d = tempfile::tempdir().unwrap();
        let mut f = fixture(d.path(), "normal");
        let db = f.database.clone();
        f.fault = Some(Arc::new(move |at, _, db| {
            if at == point {
                if point == StartupPoint::AfterLaunch {
                    db.open().unwrap().execute_batch("CREATE TRIGGER no_terminal_state BEFORE UPDATE ON agent_runtime_owners WHEN NEW.state IN ('stopped','faulted') BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
                }
                return Err("fixture_original_start_failure");
            }
            Ok(())
        }));
        let s = service(f);
        let task = demand(&s).await;
        assert_eq!(task["error_code"], "fixture_original_start_failure");
        assert_eq!(task["cleanup_verified"], false);
        let snap = s.supervisor.snapshot();
        assert_eq!(
            snap.startup_failure.unwrap().safety,
            StartupSafety::PersistenceUncertain
        );
        assert!(!snap.cleanup_verified);
        assert_eq!(owner(&db)["cleanup_verified"], false);
        assert_eq!(counts(d.path())["create"], 0);
        let o = owner(&db);
        assert!(!record_alive(Path::new(o["directory"].as_str().unwrap())).unwrap());
        assert_eq!(demand(&s).await["error_code"], "cleanup_not_verified");
        db.open()
            .unwrap()
            .execute_batch("DROP TRIGGER IF EXISTS no_terminal_state")
            .unwrap();
        let fresh = service(fixture(d.path(), "normal"));
        fresh.recover_runtime_ownership().await.unwrap();
        fresh.recover_runtime_ownership().await.unwrap();
        assert_eq!(owner(&db)["cleanup_verified"], true);
        assert_eq!(demand(&fresh).await["state"], "completed");
    }
}
#[tokio::test]
async fn restart_of_committed_preparation_recovers_no_launch_and_run_ids_without_replay() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let db = f.database.clone();
    crate::server::mkdir(&f.root).unwrap();
    let directory = f.root.join("runtime-preparing-crash");
    crate::server::mkdir(&directory).unwrap();
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
    let journal = StartupJournal::begin(db.clone(), &directory, boot.trim()).unwrap();
    journal.leave_pending_for_restart();
    let reference = crate::agents::lifecycle::AgentSessionRef(format!("cs-{}", "a".repeat(32)));
    let session = d.path().join("copilot/sessions").join(&reference.0);
    crate::server::mkdir(session.parent().unwrap()).unwrap();
    crate::server::mkdir(&session).unwrap();
    crate::copilot::store::admit(
        &db,
        777,
        &reference,
        AgentLifecycleOperation::Create,
        &session,
        "fixture-crash-777",
    )
    .unwrap();
    db.open().unwrap().execute("UPDATE agent_runs SET state='running',runtime_ref='runtime-preparing-crash' WHERE task_id=777",[]).unwrap();
    crate::copilot::store::recover(&mut db.open().unwrap()).unwrap();
    let fresh = service(f);
    assert_eq!(
        crate::copilot::store::task(&db.open().unwrap(), 777).unwrap()["state"],
        "interrupted"
    );
    fresh.recover_runtime_ownership().await.unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    assert_eq!(owner(&db)["state"], "stopped");
    assert_eq!(owner(&db)["proof"]["no_process_launched"], true);
    assert_eq!(
        crate::copilot::store::task(&db.open().unwrap(), 777).unwrap()["cleanup_verified"],
        true
    );
    assert!(!d.path().join("counts.json").exists());
    let task = demand(&fresh).await;
    assert_eq!(task["state"], "completed");
    assert!(task["task_id"].as_u64().unwrap() > 777);
    assert_eq!(counts(d.path())["create"], 1);
}

#[tokio::test]
async fn cancel_racing_before_and_after_launch_preserves_original_failure_and_durable_safety() {
    for point in [StartupPoint::BeforeLaunch, StartupPoint::AfterLaunch] {
        let d = tempfile::tempdir().unwrap();
        let mut f = fixture(d.path(), "normal");
        let db = f.database.clone();
        let target = Arc::new(std::sync::Mutex::new(
            None::<std::sync::Weak<CopilotLifecycle>>,
        ));
        let hook = target.clone();
        f.fault = Some(Arc::new(move |at, _, db| {
            if at == point {
                let s = hook.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
                let id = db
                    .open()
                    .unwrap()
                    .query_row("SELECT max(task_id) FROM agent_runs", [], |r| {
                        r.get::<_, u64>(0)
                    })
                    .unwrap();
                assert_eq!(s.cancel(id).unwrap()["cancellation_requested"], true);
                return Err("fixture_startup_failure_wins_cancel");
            }
            Ok(())
        }));
        let s = service(f);
        *target.lock().unwrap() = Some(Arc::downgrade(&s));
        let task = demand(&s).await;
        assert_failed(
            &s,
            &db,
            &task,
            if point == StartupPoint::BeforeLaunch {
                StartupSafety::NoProcessLaunched
            } else {
                StartupSafety::CleanupVerified
            },
            if point == StartupPoint::BeforeLaunch {
                "failed_before_launch"
            } else {
                "failed_after_launch"
            },
        );
        assert_eq!(task["error_code"], "fixture_startup_failure_wins_cancel");
        assert_eq!(s.supervisor.snapshot().generation, 1);
        if point == StartupPoint::AfterLaunch {
            assert_eq!(counts(d.path())["create"], 0);
            assert_eq!(counts(d.path())["send"], 0);
        } else {
            assert!(!d.path().join("counts.json").exists());
        }
        recover(&db).await.unwrap();
        recover(&db).await.unwrap();
        assert_eq!(
            db.open()
                .unwrap()
                .query_row("SELECT count(*) FROM agent_runs", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn interrupted_journal_drop_is_faulted_and_prelaunch_recovery_is_positive_and_idempotent() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let db = f.database.clone();
    crate::server::mkdir(&f.root).unwrap();
    let path = f.root.join("runtime-dropped-preparation");
    crate::server::mkdir(&path).unwrap();
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
    drop(StartupJournal::begin(db.clone(), &path, boot.trim()).unwrap());
    assert_eq!(owner(&db)["state"], "faulted");
    assert_eq!(owner(&db)["cleanup_verified"], false);
    let s = service(f);
    assert_eq!(
        s.recover_runtime_ownership().await.unwrap()["runtime"]["cleanup_verified"],
        true
    );
    s.recover_runtime_ownership().await.unwrap();
    assert_eq!(owner(&db)["proof"]["no_process_launched"], true);
    assert!(!d.path().join("counts.json").exists());
    assert_eq!(s.supervisor.snapshot().generation, 0);
}

#[tokio::test]
async fn incomplete_legacy_and_invalid_boot_records_stay_blocking_but_previous_boot_recovers() {
    for (boot_kind, owner_kind) in [
        ("current", "legacy"),
        ("current", "invalid_json"),
        ("invalid", "legacy"),
        ("previous", "legacy"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let f = fixture(d.path(), "normal");
        let db = f.database.clone();
        crate::server::mkdir(&f.root).unwrap();
        let dir = f.root.join("runtime-legacy");
        crate::server::mkdir(&dir).unwrap();
        let current = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
        let boot = match boot_kind {
            "invalid" => "unprovable-boot",
            "previous" => "00000000-0000-0000-0000-000000000000",
            _ => current.trim(),
        };
        assert_ne!(current.trim(), "00000000-0000-0000-0000-000000000000");
        db.open().unwrap().execute("INSERT INTO agent_runtime_owners(runtime_ref,private_directory,boot_id,state,owner_json) VALUES('runtime-legacy',?1,?2,'starting',?3)",rusqlite::params![dir.to_str(),boot,if owner_kind=="invalid_json" {Some("not-json")}else{None}]).unwrap();
        fs::write(dir.join("cleanup.json"), b"{\"cleanup_complete\":false}").unwrap();
        let s = service(f);
        let result = s.recover_runtime_ownership().await;
        if boot_kind == "previous" {
            result.unwrap();
            s.recover_runtime_ownership().await.unwrap();
            assert_eq!(owner(&db)["cleanup_verified"], true);
            assert_eq!(owner(&db)["proof"]["evidence"], "previous_kernel_boot");
        } else {
            assert!(result.is_err());
            assert!(s.recover_runtime_ownership().await.is_err());
            assert!(!s.supervisor.snapshot().cleanup_verified);
            assert_eq!(owner(&db)["cleanup_verified"], false);
            assert_eq!(demand(&s).await["error_code"], "cleanup_not_verified");
            assert_eq!(s.supervisor.snapshot().generation, 0);
        }
        assert!(!d.path().join("counts.json").exists());
    }
}

#[tokio::test]
async fn contradictory_preparation_and_external_pid_are_never_adopted_or_signalled() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let db = f.database.clone();
    let mut external = std::process::Command::new("/usr/bin/sleep")
        .arg("60")
        .spawn()
        .unwrap();
    crate::server::mkdir(&f.root).unwrap();
    let dir = f.root.join("runtime-contradictory");
    crate::server::mkdir(&dir).unwrap();
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
    let journal = StartupJournal::begin(db.clone(), &dir, boot.trim()).unwrap();
    journal.leave_pending_for_restart();
    let stat = fs::read_to_string(format!("/proc/{}/stat", external.id())).unwrap();
    let ticks = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse::<u64>()
        .unwrap();
    fs::write(
        dir.join("primary-owner.json"),
        serde_json::json!({"primary_pid":external.id(),"primary_start_ticks":ticks}).to_string(),
    )
    .unwrap();
    fs::set_permissions(
        dir.join("primary-owner.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::write(
        dir.join("cleanup.json"),
        b"{\"cleanup_complete\":true,\"kernel_children_exhausted\":true}",
    )
    .unwrap();
    let s = service(f);
    assert!(s.recover_runtime_ownership().await.is_err());
    assert_eq!(owner(&db)["cleanup_verified"], false);
    assert!(external.try_wait().unwrap().is_none());
    assert!(s.recover_runtime_ownership().await.is_err());
    assert!(!s.supervisor.snapshot().cleanup_verified);
    assert_eq!(demand(&s).await["error_code"], "cleanup_not_verified");
    assert!(!d.path().join("counts.json").exists());
    assert!(external.try_wait().unwrap().is_none());
    external.kill().unwrap();
    external.wait().unwrap();
}

#[tokio::test]
async fn lost_ownership_row_with_retained_artifacts_blocks_launch_and_recovery() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let db = f.database.clone();
    crate::server::mkdir(&f.root).unwrap();
    let dir = f.root.join("runtime-unregistered");
    crate::server::mkdir(&dir).unwrap();
    let failure = f.start_owned().await.err().unwrap();
    assert_eq!(failure.safety, StartupSafety::PersistenceUncertain);
    assert_eq!(failure.code, "runtime_unregistered_artifact");
    let s = service(f);
    assert_eq!(
        s.recover_runtime_ownership().await.err(),
        Some("runtime_unregistered_artifact")
    );
    assert_eq!(
        s.recover_runtime_ownership().await.err(),
        Some("runtime_unregistered_artifact")
    );
    assert!(!s.supervisor.snapshot().cleanup_verified);
    assert_eq!(demand(&s).await["error_code"], "cleanup_not_verified");
    assert_eq!(
        db.open()
            .unwrap()
            .query_row("SELECT count(*) FROM agent_runtime_owners", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert!(!d.path().join("counts.json").exists());
}

#[tokio::test]
async fn terminal_cleanup_bit_without_valid_evidence_cannot_bypass_reconciliation() {
    for launched in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let mut f = fixture(d.path(), "normal");
        let db = f.database.clone();
        f.fault = Some(Arc::new(move |point, _, _| {
            if point
                == if launched {
                    StartupPoint::AfterLaunch
                } else {
                    StartupPoint::BeforeLaunch
                }
            {
                return Err("fixture_recoverable_failure");
            }
            Ok(())
        }));
        let s = service(f);
        let task = demand(&s).await;
        assert_eq!(task["cleanup_verified"], true);
        db.open()
            .unwrap()
            .execute("UPDATE agent_runtime_owners SET cleanup_json='{}'", [])
            .unwrap();
        assert_eq!(process_snapshot(&db)["cleanup_verified"], false);
        assert_eq!(process_snapshot(&db)["recorded_cleanup_verified"], true);
        let fresh_factory = fixture(d.path(), "normal");
        let failed = fresh_factory.start_owned().await.err().unwrap();
        assert!(!failed.cleanup_verified());
        assert_eq!(failed.code, "runtime_previous_ownership_unresolved");
        assert_eq!(owner(&db)["cleanup_verified"], false);
        let fresh = service(fresh_factory);
        fresh.recover_runtime_ownership().await.unwrap();
        fresh.recover_runtime_ownership().await.unwrap();
        assert_eq!(owner(&db)["cleanup_verified"], true);
        assert_eq!(
            db.open()
                .unwrap()
                .query_row("SELECT count(*) FROM agent_runs", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            1
        );
        if launched {
            assert_eq!(counts(d.path())["create"], 0);
        } else {
            assert!(!d.path().join("counts.json").exists());
        }
        assert_eq!(demand(&fresh).await["state"], "completed");
        assert_eq!(counts(d.path())["create"], 1);
    }
}

#[tokio::test]
async fn crash_at_durable_launch_intent_without_cleanup_is_never_guessed_as_prelaunch() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let db = f.database.clone();
    crate::server::mkdir(&f.root).unwrap();
    let dir = f.root.join("runtime-intent-crash");
    crate::server::mkdir(&dir).unwrap();
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
    let mut journal = StartupJournal::begin(db.clone(), &dir, boot.trim()).unwrap();
    journal.launch_intent().unwrap();
    journal.leave_pending_for_restart();
    // No SDK was invoked in the fixture. The surviving intention alone cannot
    // establish that fact after a crash, so recovery must remain conservative.
    fs::write(dir.join("cleanup.json"), b"{\"cleanup_complete\":false}").unwrap();
    let fresh = service(f);
    assert!(fresh.recover_runtime_ownership().await.is_err());
    assert!(fresh.recover_runtime_ownership().await.is_err());
    assert!(!fresh.supervisor.snapshot().cleanup_verified);
    assert_eq!(owner(&db)["cleanup_verified"], false);
    assert_eq!(owner(&db)["owner"]["startup"]["phase"], "launch_intent");
    assert_eq!(demand(&fresh).await["error_code"], "cleanup_not_verified");
    assert!(!d.path().join("counts.json").exists());
}

#[tokio::test]
async fn silently_ignored_safety_write_is_a_persistence_failure_not_successful_recovery() {
    let d = tempfile::tempdir().unwrap();
    let mut f = fixture(d.path(), "normal");
    let db = f.database.clone();
    f.fault = Some(Arc::new(|point, _, _| {
        if point == StartupPoint::BeforeLaunch {
            Err("fixture_before_launch")
        } else {
            Ok(())
        }
    }));
    let s = service(f);
    assert_eq!(demand(&s).await["cleanup_verified"], true);
    db.open().unwrap().execute_batch("UPDATE agent_runtime_owners SET cleanup_json='{}'; CREATE TRIGGER ignore_safety_write BEFORE UPDATE ON agent_runtime_owners WHEN NEW.state='faulted' BEGIN SELECT RAISE(IGNORE); END;").unwrap();
    let fresh = service(fixture(d.path(), "normal"));
    let task = demand(&fresh).await;
    assert_eq!(task["cleanup_verified"], false);
    assert_eq!(task["error_code"], "runtime_ownership_persist_failed");
    assert_eq!(
        fresh.supervisor.snapshot().startup_failure.unwrap().safety,
        StartupSafety::PersistenceUncertain
    );
    assert_eq!(
        fresh.recover_runtime_ownership().await.err(),
        Some("runtime_ownership_persist_failed")
    );
    assert_eq!(
        fresh.recover_runtime_ownership().await.err(),
        Some("runtime_ownership_persist_failed")
    );
    assert!(!fresh.supervisor.snapshot().cleanup_verified);
    assert_eq!(owner(&db)["cleanup_verified"], true); // The trigger rejected the repair; the bit is untrusted.
    assert_eq!(process_snapshot(&db)["cleanup_verified"], false);
    assert!(!d.path().join("counts.json").exists());
    db.open()
        .unwrap()
        .execute_batch("DROP TRIGGER ignore_safety_write")
        .unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    fresh.recover_runtime_ownership().await.unwrap();
    assert_eq!(owner(&db)["proof"]["no_process_launched"], true);
    assert_eq!(demand(&fresh).await["state"], "completed");
    assert_eq!(counts(d.path())["create"], 1);
}
