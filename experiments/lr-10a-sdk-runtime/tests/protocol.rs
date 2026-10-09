use github_copilot_sdk::{CliProgram, Client, ClientOptions, Error, ErrorKind, Transport};
use narys_lr10a_poc::*;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn fixture(mode: &str, dir: &Path) -> ClientOptions {
    ClientOptions::new()
        .with_program(CliProgram::Path(PathBuf::from("/usr/bin/python3")))
        .with_prefix_args([format!(
            "{}/fixtures/mock_cli.py",
            env!("CARGO_MANIFEST_DIR")
        )])
        .with_transport(Transport::Stdio)
        .with_cwd(dir)
        .with_env([
            ("MOCK_MODE", mode),
            ("MOCK_PID_FILE", dir.join("pid").to_str().unwrap()),
        ])
}
async fn gone(pid: u32) {
    for _ in 0..100 {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("fixture PID {pid} not reaped");
}
fn fixture_pid(dir: &Path) -> u32 {
    std::fs::read_to_string(dir.join("pid"))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn quota_absent_invalid_zero_and_unlimited_are_distinct() {
    assert_eq!(quota_state(None), "quota_unknown");
    assert_eq!(quota_state(Some(&json!({}))), "quota_unknown");
    for (n, r, s) in [
        (10, 0.0, "limit_reached"),
        (0, 50.0, "limit_reached"),
        (10, 25.0, "quota_available"),
        (10, -1.0, "quota_unknown"),
        (10, 101.0, "quota_unknown"),
        (-1, 100.0, "quota_unknown"),
    ] {
        assert_eq!(
            quota_state(Some(
                &json!({"isUnlimitedEntitlement":false,"entitlementRequests":n,"remainingPercentage":r})
            )),
            s
        );
    }
    assert_eq!(
        quota_state(Some(
            &json!({"isUnlimitedEntitlement":true,"entitlementRequests":-1})
        )),
        "unlimited_reported"
    );
}
#[test]
fn errors_never_echo_secrets_or_guess_auth_and_quota() {
    for (kind, expected) in [
        (ErrorKind::Rpc { code: -32601 }, "method_unavailable"),
        (ErrorKind::Rpc { code: 401 }, "rpc_error_unknown"),
        (ErrorKind::Io, "transport_unavailable"),
        (ErrorKind::InvalidConfig, "invalid_configuration"),
    ] {
        assert_eq!(
            error_code(&Error::with_message(kind, "synthetic-secret")),
            expected
        );
    }
}
#[test]
fn path_resolution_is_explicit_and_fail_closed() {
    assert_eq!(
        explicit_program(Path::new("copilot")).unwrap_err(),
        "absolute_cli_path_required"
    );
    assert_eq!(
        explicit_program(Path::new("/nonexistent/copilot")).unwrap_err(),
        "runtime_unavailable"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_preserves_opaque_id_and_idle_is_not_success() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::start(fixture("normal", dir.path())).await.unwrap();
    let pid = client.pid().unwrap();
    let report = lifecycle(&client, dir.path()).await.unwrap();
    assert_eq!(report["resume_same_id"], true);
    assert_eq!(report["idle_seen"], true);
    assert_eq!(report["task_completed"], false);
    assert_eq!(report["operation_closed"], true);
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    gone(pid).await;
}
#[tokio::test(flavor = "current_thread")]
async fn safe_metadata_distinguishes_unauth_unknown_quota_and_unavailable_method() {
    for mode in ["unauth", "quota_unavailable"] {
        let dir = tempfile::tempdir().unwrap();
        let client = Client::start(fixture(mode, dir.path())).await.unwrap();
        let pid = client.pid().unwrap();
        let report = metadata(&client).await;
        assert_eq!(report["quota"]["state"], "quota_unknown");
        if mode == "unauth" {
            assert_eq!(report["auth"]["state"], "authentication_required");
        } else {
            assert_eq!(report["quota"]["error"], "method_unavailable");
        }
        assert!(!report.to_string().contains("synthetic-secret"));
        assert_eq!(shutdown(&client).await, "graceful");
        drop(client);
        gone(pid).await;
    }
}
#[tokio::test(flavor = "current_thread")]
async fn failed_handshake_and_cancelled_start_reclaim_process() {
    for mode in ["bad_version", "early_exit", "hang_start", "malformed"] {
        let dir = tempfile::tempdir().unwrap();
        let result = tokio::time::timeout(
            Duration::from_millis(300),
            Client::start(fixture(mode, dir.path())),
        )
        .await;
        assert!(!matches!(result, Ok(Ok(_))));
        // early_exit creates its pid file before exiting too.
        gone(fixture_pid(dir.path())).await;
    }
}
#[tokio::test(flavor = "current_thread")]
async fn abort_timeout_late_cancel_and_shutdown_timeout_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::start(fixture("abort_timeout", dir.path()))
        .await
        .unwrap();
    let pid = client.pid().unwrap();
    let session = client
        .create_session(session_config(dir.path()))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), session.abort())
            .await
            .is_err()
    );
    session.disconnect().await.unwrap();
    // Late abort may be acknowledged by runtime, but cannot imply task success.
    let _ = tokio::time::timeout(Duration::from_millis(100), session.abort()).await;
    assert!(!EventFacts::default().task_completed());
    drop(session);
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    gone(pid).await;
    let client = Client::start(fixture("stop_timeout", dir.path()))
        .await
        .unwrap();
    let pid = client.pid().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), client.stop())
            .await
            .is_err()
    );
    client.force_stop();
    drop(client);
    gone(pid).await;
}
#[tokio::test(flavor = "current_thread")]
async fn session_rpc_failure_and_detach_failure_do_not_report_success() {
    for mode in ["session_error", "detach_error"] {
        let dir = tempfile::tempdir().unwrap();
        let client = Client::start(fixture(mode, dir.path())).await.unwrap();
        let pid = client.pid().unwrap();
        assert!(lifecycle(&client, dir.path()).await.is_err());
        let _ = shutdown(&client).await;
        drop(client);
        gone(pid).await;
    }
}
#[tokio::test(flavor = "current_thread")]
async fn graceful_fixture_cleanup_reclaims_owned_descendant() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::start(fixture("descendant", dir.path()))
        .await
        .unwrap();
    let pid = client.pid().unwrap();
    let child: u32 = std::fs::read_to_string(dir.path().join("pid.child"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    gone(pid).await;
    gone(child).await;
}

#[tokio::test(flavor = "current_thread")]
async fn official_deny_handler_rejects_unknown_and_managed_requests() {
    use github_copilot_sdk::handler::{DenyAllHandler, PermissionHandler, PermissionResult};
    for data in [
        json!({}),
        json!({"kind":"shell","managedApprovalRequired":true,"command":"synthetic-command"}),
        json!({"kind":"future-unknown-kind"}),
    ] {
        let request = serde_json::from_value(data).unwrap();
        let result = DenyAllHandler
            .handle("fixture-session".into(), "fixture-request".into(), request)
            .await;
        match result {
            PermissionResult::Decision { decision, .. } => {
                assert_eq!(serde_json::to_value(decision).unwrap()["kind"], "reject")
            }
            PermissionResult::NoResult => panic!("no result could delegate approval elsewhere"),
        }
    }
}

#[test]
fn event_correlation_detects_duplicates_missing_parents_and_bounded_overflow() {
    let mut facts = EventFacts::default();
    let event = |id: String, parent: Option<String>| {
        serde_json::from_value(json!({"id":id,"parentId":parent,"timestamp":"fixture","type":"assistant.message","data":{"content":"claimed success"}})).unwrap()
    };
    facts.observe(&event("first".into(), None));
    facts.observe(&event("second".into(), Some("missing".into())));
    assert!(facts.correlation_invalid);
    facts.observe(&event("second".into(), None));
    for n in 0..40 {
        facts.observe(&event(format!("event-{n}"), None));
    }
    assert!(facts.gaps > 0);
    assert!(!facts.task_completed());
}

#[tokio::test(flavor = "current_thread")]
async fn fixture_persistence_resume_after_client_restart_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::start(fixture("normal", dir.path())).await.unwrap();
    let pid = client.pid().unwrap();
    let session = client
        .create_session(session_config(dir.path()))
        .await
        .unwrap();
    let id = session.id().clone();
    session.disconnect().await.unwrap();
    drop(session);
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    gone(pid).await;
    let client = Client::start(fixture("normal", dir.path())).await.unwrap();
    let pid = client.pid().unwrap();
    let session = client
        .resume_session(resume_config(id.clone(), dir.path()))
        .await
        .unwrap();
    assert_eq!(session.id(), &id);
    session.disconnect().await.unwrap();
    drop(session);
    client.delete_session(&id).await.unwrap();
    assert!(client
        .resume_session(resume_config(id, dir.path()))
        .await
        .is_err());
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    gone(pid).await;
}
