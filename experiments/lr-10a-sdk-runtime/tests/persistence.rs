use github_copilot_sdk::{Client, ClientOptions};
use narys_lr10a_poc::{bounded, options, persistence::*, session_config, shutdown};
use serde_json::Value;
use std::path::Path;

fn fixture(mode: &str, workspace: &Path, root: &Path, manifest: &Path) -> ClientOptions {
    let mut opts = options("/usr/bin/python3".into(), workspace, root.parent().unwrap());
    opts.prefix_args =
        vec![format!("{}/fixtures/persistence_cli.py", env!("CARGO_MANIFEST_DIR")).into()];
    opts.env.push(("MOCK_MODE".into(), mode.into()));
    opts.env.push(("MOCK_MANIFEST".into(), manifest.into()));
    opts
}

fn assert_reclaimed(manifest: &Path) {
    for line in std::fs::read_to_string(manifest).unwrap().lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let pid = row["pid"].as_u64().unwrap();
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            let start: u64 = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .nth(19)
                .unwrap()
                .parse()
                .unwrap();
            assert_ne!(
                Some(start),
                row["start_ticks"].as_u64(),
                "owned fixture identity remains"
            );
        }
    }
    assert!(
        !manifest.with_extension("violations").exists(),
        "inference or unsupported RPC attempted"
    );
}

#[test]
fn credential_environment_is_removed_without_extracting_values() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options("/usr/bin/python3".into(), dir.path(), dir.path());
    for key in [
        "COPILOT_CLI_PATH",
        "COPILOT_SDK_AUTH_TOKEN",
        "GH_TOKEN",
        "GITHUB_TOKEN",
    ] {
        assert!(opts.env_remove.iter().any(|name| name == key));
        assert!(opts.env.iter().all(|(name, _)| name != key));
    }
}

async fn run(mode: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state/session-state");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = dir.path().join("processes.jsonl");
    let report = matrix(
        |r| Ok(fixture(mode, dir.path(), r, &manifest)),
        dir.path(),
        &root,
    )
    .await;
    assert_reclaimed(&manifest);
    assert!(!report.to_string().contains("synthetic-secret"));
    assert_eq!(report["inference_calls"], 0);
    assert_eq!(report["resume_fallback_create"], false);
    assert_eq!(report["real_history_resume_gate"], "BLOCKED_REAL");
    report
}

#[test]
fn ids_are_opaque_unique_uuid_and_storage_paths_fail_closed() {
    let first = opaque_id();
    let second = opaque_id();
    assert_ne!(first, second);
    assert!(uuid::Uuid::parse_str(first.as_ref()).is_ok());
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        artifacts(dir.path(), &"../outside".into())["state"],
        "unsafe_session_id"
    );
    let id = opaque_id();
    std::os::unix::fs::symlink("/", dir.path().join(id.as_ref())).unwrap();
    assert_eq!(artifacts(dir.path(), &id)["state"], "unsafe_storage_path");
    assert_eq!(
        artifacts(&dir.path().join("missing"), &id)["state"],
        "unsafe_or_unavailable_storage_root"
    );
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(
        guarded_options(
            Path::new("/usr/bin/python3"),
            dir.path(),
            dir.path(),
            outside.path(),
            false
        )
        .err(),
        Some("state_outside_owned_root")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fixtures_preserve_explicit_and_generated_ids_across_client_and_cli_restart() {
    let report = run("persisted").await;
    let cases = report["cases"].as_array().unwrap();
    for case in &cases[..4] {
        assert_eq!(case["create"], "acknowledged");
        assert_eq!(case["explicit_id_preserved"], true);
        assert_eq!(case["same_client_resume"]["state"], "resumed");
    }
    assert_eq!(cases[0]["id_kind"], "explicit");
    assert_eq!(cases[1]["id_kind"], "sdk_generated");
    // Store index is not the transcript persistence switch.
    assert_eq!(cases[3]["store_enabled"], false);
    for case in &cases[4..8] {
        assert_eq!(case["resume"]["state"], "resumed");
    }
    assert_eq!(report["fresh_state_resume"]["code"], "session_not_found");
    assert_eq!(report["missing_session"]["code"], "session_not_found");
    for case in &cases[8..] {
        assert_eq!(case["metadata_before_delete"]["state"], "present");
        assert_eq!(case["resume_deleted"]["code"], "session_not_found");
    }
    assert!(report["shutdowns"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v == "graceful"));
}

#[tokio::test(flavor = "current_thread")]
async fn empty_sessions_do_not_fake_persist_or_resume_in_either_state_namespace() {
    let report = run("empty").await;
    for case in &report["cases"].as_array().unwrap()[..4] {
        assert_eq!(
            case["persistence_observation"],
            "empty_session_not_persisted"
        );
        assert_eq!(case["timeline"]["user_messages"], 0);
    }
    for case in &report["cases"].as_array().unwrap()[4..8] {
        assert_eq!(case["resume"]["code"], "session_not_found");
    }
    assert_eq!(report["cases"][2]["abort_empty"], "acknowledged");
}

#[tokio::test(flavor = "current_thread")]
async fn corrupt_synthetic_transcript_is_not_recovered_or_rewritten() {
    let report = run("persisted").await;
    let rows = &report["synthetic_disk_controls"];
    assert_eq!(rows[0]["resume"]["state"], "resumed");
    assert_eq!(rows[0]["source"], "fixture_authored_not_sdk_persistence");
    assert_eq!(rows[0]["resume_deleted"]["code"], "session_not_found");
    assert_eq!(rows[1]["resume"]["state"], "failed");
    assert_eq!(rows[1]["resume"]["rpc_code"], -32075);
    assert_eq!(rows[1]["transcript_bytes_unchanged"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_corruption_rpc_code_is_retained_without_guessing_modern_contract() {
    let report = run("corrupt_legacy").await;
    let row = &report["synthetic_disk_controls"][1];
    assert_eq!(row["resume"]["code"], "rpc_error_unknown");
    assert_eq!(row["resume"]["rpc_code"], -32603);
    assert_eq!(row["transcript_bytes_unchanged"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn storage_and_metadata_failures_remain_distinct_from_missing_sessions() {
    let report = run("storage_unavailable").await;
    for row in report["cases"].as_array().unwrap() {
        assert_eq!(row["create"], "failed");
    }
    let report = run("metadata_unavailable").await;
    assert_eq!(
        report["cases"][0]["metadata_active"]["code"],
        "method_unavailable"
    );
    assert_eq!(report["cases"][0]["same_client_resume"]["state"], "resumed");
}

#[tokio::test(flavor = "current_thread")]
async fn resume_identity_mismatch_and_failed_detach_cannot_be_reported_as_success() {
    let report = run("id_mismatch").await;
    assert_eq!(
        report["cases"][0]["same_client_resume"]["code"],
        "session_id_mismatch"
    );
    let report = run("detach_error").await;
    for row in &report["cases"].as_array().unwrap()[..4] {
        assert_eq!(row["disconnect"], "session_not_found");
        assert!(row.get("same_client_resume").is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn late_disconnect_fails_explicitly_without_recreating_session() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state/session-state");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = dir.path().join("processes.jsonl");
    let client = Client::start(fixture("late_disconnect", dir.path(), &root, &manifest))
        .await
        .unwrap();
    let session = client
        .create_session(session_config(dir.path()))
        .await
        .unwrap();
    bounded(session.disconnect()).await.unwrap();
    assert_eq!(
        bounded(session.disconnect()).await.unwrap_err(),
        "session_not_found"
    );
    assert_eq!(shutdown(&client).await, "graceful");
    drop(session);
    drop(client);
    assert_reclaimed(&manifest);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_storage_root_prevents_any_runtime_launch() {
    let dir = tempfile::tempdir().unwrap();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
    let report = matrix(
        |_| panic!("must not launch with unsafe storage"),
        dir.path(),
        &alias,
    )
    .await;
    assert_eq!(
        report["configuration_error"],
        "unsafe_or_unavailable_storage_root"
    );
}
