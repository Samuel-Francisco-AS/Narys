#[path = "../src/metadata_confirmation.rs"]
mod metadata_confirmation;
use github_copilot_sdk::{CliProgram, ClientOptions};
use serde_json::{json, Value};

async fn exercise(mode: &str, blocked_phase: Option<usize>) -> (Value, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let options = ClientOptions::new()
        .with_program(CliProgram::Path("/usr/bin/python3".into()))
        .with_prefix_args([
            "-I",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/metadata_confirmation_cli.py"
            ),
        ])
        .with_env([
            ("METADATA_FIXTURE_ROOT", dir.path().to_str().unwrap()),
            ("METADATA_FIXTURE_MODE", mode),
        ]);
    let mut count = 0;
    let result = metadata_confirmation::confirm(options, || {
        let safe = blocked_phase != Some(count);
        count += 1;
        json!({"structural_check":if safe {"PASS_METADATA_ACCESS"} else {"BLOCKED"},
               "snapshot":{"state":"synthetic_placeholder"}})
    })
    .await;
    let methods = std::fs::read(dir.path().join("methods.json"))
        .map(|b| serde_json::from_slice(&b).unwrap())
        .unwrap_or_default();
    assert_eq!(result["session_operations"], 0);
    assert_eq!(result["sdk_send_calls"], 0);
    assert_eq!(result["model_quota_calls"], 0);
    assert!(!result.to_string().contains("synthetic-private"));
    (result, methods)
}

#[tokio::test(flavor = "current_thread")]
async fn exactly_status_auth_shutdown_no_sensitive_method() {
    for (mode, expected) in [("positive", true), ("negative", false)] {
        let (report, methods) = exercise(mode, None).await;
        assert_eq!(report["authenticated"], expected);
        assert_eq!(report["shutdown"], "graceful");
        assert_eq!(report["phases"].as_array().unwrap().len(), 5);
        assert_eq!(
            methods
                .iter()
                .filter(|m| m.as_str() == "status.get")
                .count(),
            1
        );
        assert_eq!(
            methods
                .iter()
                .filter(|m| m.as_str() == "auth.getStatus")
                .count(),
            1
        );
        assert!(methods.iter().all(|m| matches!(
            m.as_str(),
            "connect" | "ping" | "status.get" | "auth.getStatus" | "runtime.shutdown"
        )));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn structural_failure_stops_optional_rpc_and_still_shutdowns() {
    for index in [0, 1, 2] {
        let (report, methods) = exercise("positive", Some(index)).await;
        assert!(!methods.iter().any(|m| m == "auth.getStatus"));
        if index == 0 {
            assert_eq!(report["start_calls"], 0);
            assert!(methods.is_empty());
        } else {
            assert_eq!(report["shutdown"], "graceful");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn version_mismatch_or_status_error_never_calls_auth_or_retries() {
    for mode in ["wrong_version", "status_error"] {
        let (report, methods) = exercise(mode, None).await;
        assert_eq!(report["auth_calls"], 0);
        assert_eq!(report["shutdown"], "graceful");
        assert_eq!(
            methods
                .iter()
                .filter(|m| m.as_str() == "status.get")
                .count(),
            1
        );
        assert!(!methods.iter().any(|m| m == "auth.getStatus"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn structural_failure_after_auth_keeps_auth_observation_and_shutdown() {
    let (report, _) = exercise("positive", Some(3)).await;
    assert_eq!(report["authenticated"], true);
    assert_eq!(
        report["phases"][3]["observation"]["structural_check"],
        "BLOCKED"
    );
    assert_eq!(report["shutdown"], "graceful");
}
