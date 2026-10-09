use github_copilot_sdk::handler::{PermissionHandler, PermissionResult};
use github_copilot_sdk::types::PermissionRequestData;
use github_copilot_sdk::{Client, ClientOptions, RequestId, SessionConfig, SessionId};
use narys_lr10a_poc::{bounded, options, resume_config, session_config, shutdown};
use serde_json::Value;
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

struct UnsafeCallback {
    mode: u8,
    invoked: Arc<AtomicBool>,
}
impl PermissionHandler for UnsafeCallback {
    fn handle<'life0, 'async_trait>(
        &'life0 self,
        _: SessionId,
        _: RequestId,
        _: PermissionRequestData,
    ) -> Pin<Box<dyn Future<Output = PermissionResult> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            self.invoked.store(true, Ordering::SeqCst);
            match self.mode {
                0 => PermissionResult::NoResult,
                1 => panic!("synthetic callback failure"),
                _ => std::future::pending().await,
            }
        })
    }
}
fn fixture(dir: &Path) -> ClientOptions {
    let mut opts = options("/usr/bin/python3".into(), dir, dir);
    opts.mode = github_copilot_sdk::ClientMode::Empty;
    opts.prefix_args =
        vec![format!("{}/fixtures/permission_cli.py", env!("CARGO_MANIFEST_DIR")).into()];
    opts.env
        .push(("MOCK_SUMMARY".into(), dir.join("summary.json").into()));
    opts
}
fn summary(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("summary.json")).unwrap()).unwrap()
}
async fn wait_decisions(dir: &Path, count: usize) -> Value {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let data = summary(dir);
            if data["decisions"].as_array().unwrap().len() >= count {
                return data;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("deterministic callback completion deadline")
}
async fn finish(client: Client, dir: &Path, case: &str) {
    let pid = client.pid().unwrap();
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "fixture process not reclaimed"
    );
    let data = summary(dir);
    assert_eq!(data["forbidden_methods"], 0);
    assert!(!data.to_string().contains("synthetic-secret"));
    if let Ok(path) = std::env::var("FIX3_SECURITY_EVIDENCE") {
        use std::io::Write;
        let mut output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        let mut row =
            serde_json::to_vec(&serde_json::json!({"case":case,"observations":data})).unwrap();
        row.push(b'\n');
        output.write_all(&row).unwrap();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn create_and_resume_deny_shell_write_unknown_and_managed_permissions_on_wire() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::start(fixture(dir.path())).await.unwrap();
    let session = bounded(client.create_session(session_config(dir.path())))
        .await
        .unwrap();
    let data = wait_decisions(dir.path(), 3).await;
    assert!(data["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d == "reject"));
    let id = session.id().clone();
    session.disconnect().await.unwrap();
    drop(session);
    let resumed = bounded(client.resume_session(resume_config(id, dir.path())))
        .await
        .unwrap();
    let data = wait_decisions(dir.path(), 6).await;
    assert!(data["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d == "reject"));
    for row in data["configurations"].as_array().unwrap() {
        for (key, value) in row.as_object().unwrap() {
            if key != "operation" {
                assert_eq!(value, true, "missing effective security setting {key}");
            }
        }
    }
    resumed.disconnect().await.unwrap();
    drop(resumed);
    finish(client, dir.path(), "deny_create_resume").await;
}
#[tokio::test(flavor = "current_thread")]
async fn required_deny_policy_overrides_noresult_error_and_timeout_callback() {
    for mode in [0, 1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let invoked = Arc::new(AtomicBool::new(false));
        let callback = Arc::new(UnsafeCallback {
            mode,
            invoked: invoked.clone(),
        });
        // SDK 1.0.17 policy takes precedence over a supplied handler.
        let config = session_config(dir.path()).with_permission_handler(callback);
        let client = Client::start(fixture(dir.path())).await.unwrap();
        let session = bounded(client.create_session(config)).await.unwrap();
        let data = wait_decisions(dir.path(), 3).await;
        assert!(data["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d == "reject"));
        assert!(!invoked.load(Ordering::SeqCst));
        session.disconnect().await.unwrap();
        drop(session);
        finish(
            client,
            dir.path(),
            [
                "deny_overrides_noresult",
                "deny_overrides_error",
                "deny_overrides_timeout",
            ][mode as usize],
        )
        .await;
    }
}
#[tokio::test(flavor = "current_thread")]
async fn raw_absence_noresult_error_and_hanging_handler_do_not_supply_explicit_denial() {
    for mode in [None, Some(0), Some(1), Some(2)] {
        let dir = tempfile::tempdir().unwrap();
        let invoked = Arc::new(AtomicBool::new(false));
        let mut config = SessionConfig::default().with_available_tools(Vec::<String>::new());
        if let Some(mode) = mode {
            config = config.with_permission_handler(Arc::new(UnsafeCallback {
                mode,
                invoked: invoked.clone(),
            }));
        }
        let client = Client::start(fixture(dir.path())).await.unwrap();
        let prepared = client.prepare_session(config).unwrap();
        let mut events = prepared.subscribe();
        let session = bounded(prepared.start()).await.unwrap();
        // Notification barrier: all synthetic permission broadcasts must arrive.
        for _ in 0..3 {
            tokio::time::timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap()
                .unwrap();
        }
        // A bounded observation window verifies absence of an SDK decision;
        // it is not presented as proof of CLI timeout behavior or approval.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let data = summary(dir.path());
        assert!(data["decisions"].as_array().unwrap().is_empty());
        if mode.is_none() {
            assert_eq!(data["configurations"][0]["request_permission"], false);
        } else {
            assert!(invoked.load(Ordering::SeqCst));
        }
        session.disconnect().await.unwrap();
        drop(session);
        let case = match mode {
            None => "raw_absent_handler",
            Some(0) => "raw_noresult",
            Some(1) => "raw_callback_error",
            _ => "raw_callback_timeout",
        };
        finish(client, dir.path(), case).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unsupported_required_options_fail_creation_and_resume_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = fixture(dir.path());
    opts.env.push(("MOCK_REJECT_OPTIONS".into(), "1".into()));
    let client = Client::start(opts).await.unwrap();
    let created = bounded(client.create_session(session_config(dir.path()))).await;
    assert!(matches!(created, Err("method_unavailable")));
    let resumed =
        bounded(client.resume_session(resume_config("synthetic-owned".into(), dir.path()))).await;
    assert!(matches!(resumed, Err("method_unavailable")));
    finish(client, dir.path(), "required_options_rejected").await;
}
