#[path = "../src/host_assisted.rs"]
#[allow(dead_code)]
mod host_assisted;
use github_copilot_sdk::{CliProgram, Client, ClientMode};
use narys_lr10a_poc::shutdown;
use serde_json::Value;
use std::path::Path;

#[test]
fn metadata_context_contract_preserves_normal_auth_and_no_secret_override() {
    let opts = host_assisted::preflight_options(
        Path::new("/native"),
        Path::new("/fixture"),
        Path::new("/logs"),
    );
    assert_eq!(opts.mode, ClientMode::CopilotCli);
    assert_eq!(opts.use_logged_in_user, Some(true));
    assert!(opts.github_token.is_none());
    assert!(opts.base_directory.is_none());
    assert!(opts.prefix_args.is_empty());
    assert!(opts.env.is_empty());
    assert!(!opts.extra_args.iter().any(|a| a == "--no-auto-login"));
    for name in [
        "COPILOT_HOME",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "COPILOT_GITHUB_TOKEN",
        "COPILOT_SDK_AUTH_TOKEN",
    ] {
        assert!(opts.env_remove.iter().any(|n| n == name));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn synthetic_context_and_fallback_matrix_never_creates_or_sends() {
    for (context, logged_in, expected) in [
        ("missing", true, false),
        ("available", true, true),
        ("available", false, false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut opts =
            host_assisted::preflight_options(Path::new("/usr/bin/python3"), dir.path(), dir.path());
        opts.program = CliProgram::Path("/usr/bin/python3".into());
        opts.prefix_args = vec![
            "-I".into(),
            concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/auth_context_cli.py").into(),
        ];
        opts.use_logged_in_user = Some(logged_in);
        opts.env = vec![
            ("A9_AUTH_FIXTURE_ROOT".into(), dir.path().into()),
            ("A9_PUBLIC_AUTH_CONTEXT".into(), context.into()),
        ];
        let client = Client::start(opts).await.unwrap();
        let observation = host_assisted::preflight(&client).await;
        assert_eq!(observation["auth"]["authenticated"], expected);
        if expected {
            assert_eq!(observation["catalog"]["models"][0]["id"], "synthetic-zero");
            assert_eq!(observation["quota"]["state"], "observed");
        } else {
            assert_eq!(observation["catalog"]["models"], Value::Null);
            assert_eq!(observation["quota"]["state"], "quota_unknown");
        }
        let text = observation.to_string();
        assert!(!text.contains("synthetic-identity"));
        assert!(!text.contains("marker-not-for-evidence"));
        let blockers = host_assisted::blockers(&observation);
        for code in [
            "billing_units_and_maximum_cost_unverified",
            "no_paid_fallback_enforcement_unverified",
            "private_authenticated_session_state_unverified",
        ] {
            assert!(blockers.contains(&code));
        }
        assert_eq!(shutdown(&client).await, "graceful");
        drop(client);
        let summary: Value = serde_json::from_slice(
            &std::fs::read(dir.path().join("metadata-summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(summary["session_operations"], 0);
        assert_eq!(summary["send_calls"], 0);
        assert_eq!(summary["unexpected_methods"], 0);
        assert!(!dir.path().join("lr10a-a9-host-attempt.json").exists());
    }
}
