#[path = "../src/host_assisted.rs"]
#[allow(dead_code)]
mod host_assisted;
use github_copilot_sdk::{CliProgram, Client, ClientOptions, MessageOptions, SessionId};
use narys_lr10a_poc::{bounded, resume_config, session_config, shutdown};
use serde_json::{json, Value};
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Arc, Barrier},
    time::Duration,
};

fn private(dir: &Path) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn options(dir: &Path, mode: &str) -> ClientOptions {
    ClientOptions::new()
        .with_program(CliProgram::Path("/usr/bin/python3".into()))
        .with_prefix_args([
            "-I",
            concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/a9_host_cli.py"),
        ])
        .with_cwd(dir)
        .with_env([
            ("A9_SYNTHETIC_ROOT", dir.to_str().unwrap()),
            ("A9_SYNTHETIC_MODE", mode),
        ])
}
fn summary(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("summary.json")).unwrap()).unwrap()
}
async fn finish(client: Client) {
    let pid = client.pid().unwrap();
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
}

#[test]
fn concurrent_marker_claim_is_exclusive_and_persistent() {
    let dir = tempfile::tempdir().unwrap();
    private(dir.path());
    let barrier = Arc::new(Barrier::new(8));
    let mut workers = vec![];
    for _ in 0..8 {
        let p = dir.path().to_path_buf();
        let b = barrier.clone();
        workers.push(std::thread::spawn(move || {
            b.wait();
            host_assisted::claim_attempt(&p).is_ok()
        }));
    }
    assert_eq!(
        workers
            .into_iter()
            .map(|w| usize::from(w.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let marker = dir.path().join("lr10a-a9-host-attempt.json");
    assert_eq!(
        std::fs::metadata(&marker).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::write(&marker, b"crash-corrupted-fixture").unwrap();
    assert!(host_assisted::claim_attempt(dir.path()).is_err());
}
#[test]
fn marker_rejects_symlinks_permissions_and_existing_unknown_entries() {
    let dir = tempfile::tempdir().unwrap();
    private(dir.path());
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(dir.path(), &link).unwrap();
    assert!(host_assisted::claim_attempt(&link).is_err());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(host_assisted::claim_attempt(dir.path()).is_err());
    private(dir.path());
    std::os::unix::fs::symlink(
        "/nonexistent",
        dir.path().join("lr10a-a9-host-attempt.json"),
    )
    .unwrap();
    assert!(host_assisted::claim_attempt(dir.path()).is_err());
}
#[test]
fn billing_unknown_auto_and_even_mock_zero_cost_never_admit_real_send() {
    for observation in [
        json!({}),
        json!({"auth":{"authenticated":true},"catalog":{"models":[{"id":"auto"}]}}),
        json!({"auth":{"authenticated":true},"catalog":{"models":[{"id":"synthetic-zero","billing_multiplier":0}]}}),
    ] {
        let reasons = host_assisted::blockers(&observation);
        assert!(reasons.contains(&"billing_units_and_maximum_cost_unverified"));
        assert!(reasons.contains(&"no_paid_fallback_enforcement_unverified"));
        assert!(!reasons.is_empty());
    }
}
#[test]
fn preflight_uses_normal_existing_auth_without_extracting_token() {
    let opts = host_assisted::preflight_options(
        Path::new("/synthetic"),
        Path::new("/workspace"),
        Path::new("/logs"),
    );
    assert!(!opts.extra_args.iter().any(|s| s == "--no-auto-login"));
    assert_eq!(opts.use_logged_in_user, Some(true));
    assert!(opts.github_token.is_none());
    assert!(opts.base_directory.is_none());
    assert!(opts.env_remove.iter().any(|s| s == "GITHUB_TOKEN"));
}

#[tokio::test(flavor = "current_thread")]
async fn synthetic_exact_prompt_one_send_detach_restart_resume_without_resend() {
    let dir = tempfile::tempdir().unwrap();
    private(dir.path());
    let id = SessionId::from(uuid::Uuid::new_v4().to_string());
    let client = Client::start(options(dir.path(), "normal")).await.unwrap();
    let mut config = session_config(dir.path()).with_model("synthetic-zero");
    config.session_id = Some(id.clone());
    let prepared = client.prepare_session(config).unwrap();
    let _events = prepared.subscribe();
    let session = bounded(prepared.start()).await.unwrap();
    host_assisted::claim_attempt(dir.path()).unwrap(); // Claim BEFORE SDK send.
    assert_eq!(
        bounded(session.send(MessageOptions::new(host_assisted::PROMPT)))
            .await
            .unwrap(),
        "synthetic-message"
    );
    assert!(host_assisted::claim_attempt(dir.path()).is_err()); // No second send.
    bounded(session.disconnect()).await.unwrap();
    drop(session);
    finish(client).await;
    let first = summary(dir.path());
    assert_eq!(first["send_calls"], 1);
    assert_eq!(first["prompt_matches"], true);
    for k in ["zero_tools", "request_permission", "instructions_disabled"] {
        assert_eq!(first[k], true);
    }
    assert_eq!(first["unexpected_methods"], 0);
    let client = Client::start(options(dir.path(), "normal")).await.unwrap();
    let prepared = client
        .prepare_resume_session(resume_config(id.clone(), dir.path()).with_model("synthetic-zero"))
        .unwrap();
    let _events = prepared.subscribe();
    let session = bounded(prepared.start()).await.unwrap();
    assert_eq!(session.id(), &id);
    let events = bounded(session.get_events()).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data["content"], "5");
    bounded(session.disconnect()).await.unwrap();
    drop(session);
    finish(client).await;
    let second = summary(dir.path());
    assert_eq!(second["send_calls"], 0);
    assert_eq!(second["create_calls"], 0);
    assert_eq!(second["resume_calls"], 1);
}
#[tokio::test(flavor = "current_thread")]
async fn synthetic_send_error_or_timeout_burns_attempt_no_retry() {
    for mode in ["error", "timeout"] {
        let dir = tempfile::tempdir().unwrap();
        private(dir.path());
        let client = Client::start(options(dir.path(), mode)).await.unwrap();
        let prepared = client
            .prepare_session(session_config(dir.path()).with_model("synthetic-zero"))
            .unwrap();
        let _events = prepared.subscribe();
        let session = bounded(prepared.start()).await.unwrap();
        host_assisted::claim_attempt(dir.path()).unwrap();
        let sent = tokio::time::timeout(
            Duration::from_millis(200),
            session.send(MessageOptions::new(host_assisted::PROMPT)),
        )
        .await;
        assert!(!matches!(sent, Ok(Ok(_))));
        assert!(host_assisted::claim_attempt(dir.path()).is_err());
        let _ = bounded(session.abort()).await;
        bounded(session.disconnect()).await.unwrap();
        drop(session);
        finish(client).await;
        assert_eq!(summary(dir.path())["send_calls"], 1);
    }
}

#[test]
fn available_quota_is_not_a_pricing_or_paid_fallback_admission() {
    let base = json!({"auth":{"authenticated":true},"catalog":{"models":[{"id":"synthetic-zero","billing_multiplier":0}]},
        "quota":{"snapshots":[{"kind":"premium_interactions","state":"quota_available","snapshot":{
            "entitlementRequests":200,"remainingPercentage":74.2,"overageAllowedWithExhaustedQuota":false,
            "usageAllowedWithExhaustedQuota":false}}]}});
    let reasons = host_assisted::blockers(&base);
    assert!(!reasons.contains(&"included_quota_unverified"));
    assert!(reasons.contains(&"billing_units_and_maximum_cost_unverified"));
    let mut enabled = base.clone();
    enabled["quota"]["snapshots"][0]["snapshot"]["overageAllowedWithExhaustedQuota"] = json!(true);
    assert!(host_assisted::blockers(&enabled).contains(&"overage_policy_unverified_or_enabled"));
    enabled["quota"]["snapshots"][0]["snapshot"]
        .as_object_mut()
        .unwrap()
        .remove("overageAllowedWithExhaustedQuota");
    assert!(host_assisted::blockers(&enabled).contains(&"overage_policy_unverified_or_enabled"));
}

#[test]
fn live_binary_rejects_send_modes_and_missing_ownership_before_cli_start() {
    for mode in ["send", "a9", "preflight"] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_a9-host-assisted"))
            .args([mode, "/nonexistent-cli-must-not-launch"])
            .env_remove("NARYS_LR10A_OWNED_HARNESS")
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["sdk_send_calls"], 0);
        assert_eq!(report["code"], "owned_preflight_only");
    }
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_a9-host-assisted"))
        .args(["send", "/nonexistent-cli-must-not-launch"])
        .env("NARYS_LR10A_OWNED_HARNESS", "1")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
}
