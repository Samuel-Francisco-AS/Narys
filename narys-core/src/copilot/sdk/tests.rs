use super::*;
use crate::agents::lifecycle::AgentLifecycleOperation;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
fn fixture(d: &Path, mode: &str) -> SdkRuntimeFactory {
    let script = d.join("fixture-cli");
    fs::write(
        &script,
        format!(
            "#!/usr/bin/python3\nMODE={}\nROOT={}\n{}",
            serde_json::to_string(mode).unwrap(),
            serde_json::to_string(d.to_str().unwrap()).unwrap(),
            include_str!("../../../tests/lr10b_sdk_peer.py")
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let db = crate::persistence::database::Database::new(d.join("db"));
    db.open().unwrap();
    crate::server::mkdir(&d.join("copilot")).unwrap();
    SdkRuntimeFactory::new(script, d.join("copilot/runtimes"), db)
}
fn invocation(
    d: &Path,
    operation: AgentLifecycleOperation,
    id: Option<String>,
) -> SessionInvocation {
    let directory = d.join("session");
    crate::server::mkdir(&directory).unwrap();
    use sha2::{Digest, Sha256};
    let anchor = id.as_ref().map(|id| {
        format!(
            "{:x}",
            Sha256::digest(format!("{id}:fixture-root").as_bytes())
        )
    });
    SessionInvocation {
        operation,
        directory,
        provider_session_id: id,
        expected_history_anchor: anchor,
    }
}
fn counts(d: &Path) -> Value {
    serde_json::from_slice(&fs::read(d.join("counts.json")).unwrap()).unwrap()
}
fn proofs(d: &Path) -> Value {
    let dir = fs::read_dir(d.join("copilot/runtimes"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    serde_json::from_slice(&fs::read(dir.join("cleanup.json")).unwrap()).unwrap()
}
#[tokio::test]
async fn official_sdk_fixture_creates_detaches_resumes_and_never_sends() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let r = f.start_owned().await.unwrap();
    let receipt = r
        .session(
            &invocation(d.path(), AgentLifecycleOperation::Create, None),
            &Cancellation::default(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let r2 = r
        .session(
            &invocation(
                d.path(),
                AgentLifecycleOperation::Resume,
                Some(receipt.provider_session_id.clone()),
            ),
            &Cancellation::default(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    assert_eq!(r2.provider_session_id, receipt.provider_session_id);
    r.stop().await.unwrap();
    assert_eq!(counts(d.path())["create"], 1);
    assert_eq!(counts(d.path())["resume"], 1);
    assert_eq!(counts(d.path())["detach"], 2);
    assert_eq!(counts(d.path())["send"], 0);
    assert_eq!(proofs(d.path())["cleanup_complete"], true);
}
#[tokio::test]
async fn sdk_cancel_during_create_aborts_detaches_and_reaps() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "slow");
    let r = f.start_owned().await.unwrap();
    let c = Arc::new(Cancellation::default());
    let work = {
        let r = r.clone();
        let c = c.clone();
        let i = invocation(d.path(), AgentLifecycleOperation::Create, None);
        tokio::spawn(async move { r.session(&i, &c, Arc::new(|_| {})).await })
    };
    while fs::read(d.path().join("counts.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_none_or(|v| v["create"] != 1)
    {
        tokio::task::yield_now().await;
    }
    c.cancel();
    assert_eq!(work.await.unwrap().unwrap_err(), "cancelled");
    r.stop().await.unwrap();
    assert_eq!(counts(d.path())["abort"], 1);
    assert_eq!(counts(d.path())["detach"], 1);
    assert_eq!(proofs(d.path())["cleanup_complete"], true);
}
#[tokio::test]
async fn cli_absent_and_wrong_pin_do_not_launch_runtime() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    assert_eq!(f.start().await.err().unwrap().code, "cli_pin_mismatch");
    assert!(!d.path().join("copilot/runtimes").exists());
    let f = SdkRuntimeFactory::new(
        d.path().join("absent"),
        d.path().join("copilot/runtimes"),
        f.database.clone(),
    );
    assert_eq!(f.start().await.err().unwrap().code, "cli_unavailable");
    assert!(!d.path().join("copilot/runtimes").exists());
}
#[tokio::test]
async fn incompatible_runtime_is_cleaned_before_admission() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "incompatible");
    let failure = f.start_owned().await.err().unwrap();
    assert_eq!(failure.code, "runtime_protocol_mismatch");
    assert_eq!(failure.safety, StartupSafety::CleanupVerified);
    assert_eq!(proofs(d.path())["cleanup_complete"], true);
}
#[tokio::test]
async fn failed_create_and_missing_resume_have_no_fallback_or_secret_diagnostic() {
    for mode in ["create_error", "resume_missing"] {
        let d = tempfile::tempdir().unwrap();
        let f = fixture(d.path(), mode);
        let r = f.start_owned().await.unwrap();
        let operation = if mode == "create_error" {
            AgentLifecycleOperation::Create
        } else {
            AgentLifecycleOperation::Resume
        };
        let result = r
            .session(
                &invocation(
                    d.path(),
                    operation,
                    if mode == "resume_missing" {
                        Some("registered-id".into())
                    } else {
                        None
                    },
                ),
                &Cancellation::default(),
                Arc::new(|_| {}),
            )
            .await;
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains("fixture-private"));
        r.stop().await.unwrap();
        assert_eq!(counts(d.path())["send"], 0);
        assert_eq!(
            counts(d.path())["create"],
            if mode == "create_error" { 1 } else { 0 }
        );
    }
}
#[tokio::test]
async fn unexpected_cli_death_is_reaped_and_recovery_is_explicit() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "session_death");
    let r = f.start_owned().await.unwrap();
    assert!(r
        .session(
            &invocation(d.path(), AgentLifecycleOperation::Create, None),
            &Cancellation::default(),
            Arc::new(|_| {})
        )
        .await
        .is_err());
    let stopped = r.stop().await;
    assert!(stopped.is_ok() || stopped == Err("sdk_shutdown_recovered"));
    assert_eq!(proofs(d.path())["cleanup_complete"], true);
    assert_eq!(counts(d.path())["send"], 0);
}
fn signal_owned(pid: u32) {
    // Test-only PID comes from the fixture's private kernel ownership receipt.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) as i32 };
    assert!(fd >= 0);
    assert_eq!(
        unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, libc::SIGKILL, 0, 0) },
        0
    );
    unsafe {
        libc::close(fd);
    }
}
#[tokio::test]
async fn killed_guardian_and_killed_primary_each_recover_descendants_without_touching_external() {
    for killed in ["guardian_pid", "primary_pid"] {
        let d = tempfile::tempdir().unwrap();
        let f = fixture(d.path(), "descendant");
        let r = f.start_owned().await.unwrap();
        let mut external = std::process::Command::new("/usr/bin/sleep")
            .arg("120")
            .spawn()
            .unwrap();
        let dir = fs::read_dir(d.path().join("copilot/runtimes"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let file = if killed == "primary_pid" {
            "primary-owner.json"
        } else {
            "owner.json"
        };
        let owner: Value = serde_json::from_slice(&fs::read(dir.join(file)).unwrap()).unwrap();
        signal_owned(owner[killed].as_u64().unwrap() as u32);
        let stopped = r.stop().await;
        assert!(
            stopped.is_ok() || stopped == Err("sdk_shutdown_recovered"),
            "{stopped:?}"
        );
        assert_eq!(proofs(d.path())["cleanup_complete"], true);
        assert!(!record_alive(&dir).unwrap());
        assert!(external.try_wait().unwrap().is_none());
        external.kill().unwrap();
        external.wait().unwrap();
        let descendant = fs::read_to_string(d.path().join("descendant.pid")).unwrap();
        assert!(!PathBuf::from(format!("/proc/{}", descendant.trim())).exists());
    }
}
#[tokio::test]
async fn detached_observer_and_burst_events_never_leak_private_payload() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "burst");
    let r = f.start_owned().await.unwrap();
    let observations = Arc::new(AtomicUsize::new(0));
    let observed = observations.clone();
    let result = r
        .session(
            &invocation(d.path(), AgentLifecycleOperation::Create, None),
            &Cancellation::default(),
            Arc::new(move |code| {
                assert!(!code.contains("private"));
                observed.fetch_add(1, Ordering::Relaxed);
            }),
        )
        .await
        .unwrap();
    assert!(
        result.observation_gaps > 0,
        "burst should produce a factual gap"
    );
    r.stop().await.unwrap();
    assert!(observations.load(Ordering::Relaxed) <= 5);
}
#[tokio::test]
async fn late_tool_and_error_events_are_real_failures_not_observer_failures() {
    for (mode, expected) in [
        ("tools", "unexpected_tool_execution"),
        ("error_event", "session_error_observed"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let f = fixture(d.path(), mode);
        let r = f.start_owned().await.unwrap();
        let result = r
            .session(
                &invocation(d.path(), AgentLifecycleOperation::Create, None),
                &Cancellation::default(),
                Arc::new(|_| {}),
            )
            .await;
        assert_eq!(result.unwrap_err(), expected);
        r.stop().await.unwrap();
        assert_eq!(counts(d.path())["send"], 0);
        assert_eq!(proofs(d.path())["cleanup_complete"], true);
    }
}
#[tokio::test]
async fn official_deny_all_handler_rejects_native_shell_permission_request() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "permission");
    let r = f.start_owned().await.unwrap();
    r.session(
        &invocation(d.path(), AgentLifecycleOperation::Create, None),
        &Cancellation::default(),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    r.stop().await.unwrap();
    assert_eq!(counts(d.path())["permissions_denied"], 1);
    assert_eq!(counts(d.path())["send"], 0);
}
#[tokio::test]
async fn interrupted_startup_and_artifact_retention_are_bounded_and_recoverable() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "startup_death");
    assert!(f.start_owned().await.is_err());
    assert_eq!(proofs(d.path())["cleanup_complete"], true);
    let conn = f.database.open().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT cleanup_verified FROM agent_runtime_owners",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap(),
        true
    );
    // Retention fixtures create no processes; only proved private directories are pruned.
    for n in 0..36 {
        let reference = format!("runtime-retention-{n:02}");
        let path = f.root.join(&reference);
        crate::server::mkdir(&path).unwrap();
        conn.execute("INSERT INTO agent_runtime_owners(runtime_ref,private_directory,boot_id,state,cleanup_verified) VALUES(?1,?2,'fixture','stopped',1)",rusqlite::params![reference,path.to_str()]).unwrap();
    }
    prune(&f.database, &f.root).unwrap();
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 32);
}
#[tokio::test]
async fn graceful_deadline_forces_owned_runtime_and_records_verified_recovery() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "hung_exit");
    let r = f.start_owned().await.unwrap();
    let started = std::time::Instant::now();
    assert_eq!(r.stop().await, Err("sdk_shutdown_recovered"));
    assert!(started.elapsed() < Duration::from_secs(12));
    assert_eq!(proofs(d.path())["cleanup_complete"], true);
    assert_eq!(
        f.database
            .open()
            .unwrap()
            .query_row(
                "SELECT cleanup_verified FROM agent_runtime_owners",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
        true
    );
}
#[tokio::test]
async fn restart_reconciles_owned_artifacts_without_relaunching_or_signalling_external() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "normal");
    let r = f.start_owned().await.unwrap();
    let reference = r.ownership_ref().unwrap();
    drop(r); // Model host loss: SDK drop kills its transport owner; guardian reaps.
    recover(&f.database).await.unwrap();
    let conn = f.database.open().unwrap();
    let (state, verified): (String, bool) = conn
        .query_row(
            "SELECT state,cleanup_verified FROM agent_runtime_owners WHERE runtime_ref=?1",
            [reference],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "stopped");
    assert!(verified);
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}
#[tokio::test]
async fn resume_requires_matching_durable_history_anchor_without_create_fallback() {
    let d = tempfile::tempdir().unwrap();
    let f = fixture(d.path(), "history_mismatch");
    let r = f.start_owned().await.unwrap();
    let initial = r
        .session(
            &invocation(d.path(), AgentLifecycleOperation::Create, None),
            &Cancellation::default(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let resumed = r
        .session(
            &invocation(
                d.path(),
                AgentLifecycleOperation::Resume,
                Some(initial.provider_session_id),
            ),
            &Cancellation::default(),
            Arc::new(|_| {}),
        )
        .await;
    assert_eq!(resumed.unwrap_err(), "session_history_not_verified");
    r.stop().await.unwrap();
    assert_eq!(counts(d.path())["create"], 1);
    assert_eq!(counts(d.path())["send"], 0);
    assert_eq!(counts(d.path())["detach"], 2);
}

mod startup_safety;
