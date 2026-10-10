//! Real Unix IPC/lifecycle. Synthetic data, no provider/credential/inference calls.
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Server {
    child: Child,
    home: tempfile::TempDir,
    runtime: tempfile::TempDir,
}
impl Server {
    fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let child = Self::launch(home.path(), runtime.path());
        let s = Self {
            child,
            home,
            runtime,
        };
        s.ready();
        s
    }
    fn launch(home: &std::path::Path, runtime: &std::path::Path) -> Child {
        Command::new(env!("CARGO_BIN_EXE_narys-core"))
            .arg("serve")
            .env_clear()
            .env("HOME", home)
            .env("XDG_RUNTIME_DIR", runtime)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent-narys-synthetic-bus",
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
    fn socket(&self) -> std::path::PathBuf {
        self.runtime.path().join("narys-core/control.sock")
    }
    fn ready(&self) {
        let end = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(self.socket()).is_err() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn raw(&self, bytes: &[u8]) -> Value {
        let mut s = UnixStream::connect(self.socket()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(bytes).unwrap();
        s.shutdown(std::net::Shutdown::Write).unwrap();
        let mut out = vec![];
        s.read_to_end(&mut out).unwrap();
        serde_json::from_slice(&out).unwrap()
    }
    fn call(&self, command: Value) -> Value {
        self.raw(
            &serde_json::to_vec(&json!({"version":1,"request_id":"test","command":command}))
                .unwrap(),
        )
    }
    fn stop(&mut self, signal: i32) {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, self.child.id(), 0) } as i32;
        assert!(fd >= 0);
        assert_eq!(
            unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, signal, 0, 0) },
            0
        );
        unsafe {
            libc::close(fd);
        }
        let _ = self.child.wait().unwrap();
    }
    fn restart(&mut self) {
        self.stop(libc::SIGTERM);
        self.child = Self::launch(self.home.path(), self.runtime.path());
        self.ready();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.stop(libc::SIGTERM);
        }
    }
}
#[test]
fn typed_protocol_bounds_versions_and_future_authority_are_enforced() {
    let s = Server::start();
    let r = s.call(json!({"operation":"capabilities"}));
    assert_eq!(r["version"], 1);
    assert_eq!(r["request_id"], "test");
    assert_eq!(r["data"]["agent_tools"], false);
    for raw in [
        json!({"version":2,"request_id":"test","command":{"operation":"status"}}),
        json!({"version":1,"request_id":"test","command":{"operation":"status","authority":"HumanLocal"}}),
        json!({"version":1,"request_id":"test","command":{"operation":"result","task_id":0}}),
    ] {
        assert_eq!(s.raw(&serde_json::to_vec(&raw).unwrap())["ok"], false);
    }
    assert_eq!(
        s.raw(&vec![b'x'; narys_core::ipc::MAX_REQUEST_BYTES + 1])["error_code"],
        "request_limit_or_timeout"
    );
    for op in [
        json!({"operation":"approval","approval_id":"a","decision":"approve_once"}),
        json!({"operation":"tool-request","task":{"namespace":"product","id":1},"invocation":{"tool":"read_file","workspace_id":"w","relative_path":"/etc/passwd"}}),
    ] {
        assert_eq!(s.call(op)["error_code"], "capability_not_integrated");
    }
    assert_eq!(
        s.call(json!({"operation":"status"}))["data"]["execution_workers"],
        0
    );
}
#[test]
fn duplicate_daemon_cannot_recover_or_take_socket_from_authority() {
    let s = Server::start();
    let mut duplicate = Server::launch(s.home.path(), s.runtime.path());
    assert!(!duplicate.wait().unwrap().success());
    assert_eq!(s.call(json!({"operation":"status"}))["ok"], true);
}
#[test]
fn disconnect_cancellation_and_restart_preserve_durable_events_without_replay() {
    let mut s = Server::start();
    let r=s.call(json!({"operation":"prepare","task":{"objective":"synthetic","model":"auto","included_only_approval":true},"expected":"5"}));
    let id = r["data"]["task_id"].as_u64().unwrap();
    let workspace = r["data"]["workspace"].as_str().unwrap().to_owned();
    let cancel = json!({"operation":"cancel","task_id":id});
    assert_eq!(s.call(cancel.clone())["data"]["state"], "cancelled");
    assert_eq!(s.call(cancel)["data"]["already_terminal"], true);
    let events = s.call(json!({"operation":"events"}));
    let cursor = events["data"]["next_sequence"].as_u64().unwrap();
    s.restart();
    assert_eq!(
        s.call(json!({"operation":"result","task_id":id}))["data"]["state"],
        "cancelled"
    );
    let persisted = s.call(json!({"operation":"events","after":0}));
    assert!(persisted["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["code"] == "cancelled_before_send"));
    let newer = s.call(json!({"operation":"events","after":cursor,"limit":1}));
    assert_eq!(newer["data"]["events"].as_array().unwrap().len(), 1);
    assert!(!std::path::Path::new(&workspace)
        .parent()
        .unwrap()
        .join("send-attempt.json")
        .exists());
    fs::remove_dir_all(std::path::Path::new(&workspace).parent().unwrap()).unwrap();
}
#[test]
fn crashed_running_task_is_interrupted_once_and_never_retried() {
    let mut s = Server::start();
    s.stop(libc::SIGKILL);
    let path = s
        .home
        .path()
        .join(".local/state/narys/core/db/luna.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO headless_tasks VALUES(9,'/nonexistent','uncertain','5','running',NULL,NULL)",
        [],
    )
    .unwrap();
    drop(db);
    s.child = Server::launch(s.home.path(), s.runtime.path());
    s.ready();
    assert_eq!(
        s.call(json!({"operation":"result","task_id":9}))["data"]["state"],
        "interrupted"
    );
    s.restart();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM server_events WHERE task_id=9 AND code='restart_never_retries'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn conversation_sessions_product_errors_and_free_permission_are_real_ipc() {
    let mut s = Server::start();
    let providers = s.call(json!({"operation":"providers"}));
    assert_eq!(providers["data"]["providers"].as_array().unwrap().len(), 4);
    assert_eq!(providers["data"]["credential_store_available"], false);
    assert_eq!(
        s.call(json!({"operation":"provider-configure","provider_id":"groq","enabled":true}))
            ["error_code"],
        "free_provider_authorization_required"
    );
    let created = s.call(json!({"operation":"session-create"}));
    let session = created["data"]["session_id"].as_i64().unwrap();
    assert_eq!(
        s.call(json!({"operation":"sessions","limit":1}))["data"]["sessions"][0]["session_id"],
        session
    );
    let accepted = s.call(
        json!({"operation":"conversation","session_id":session,"text":"synthetic-persisted-input"}),
    );
    assert_eq!(accepted["ok"], true);
    assert_eq!(accepted["data"]["disconnect_cancels"], false);
    let id = accepted["data"]["task_id"].as_u64().unwrap();
    let get = json!({"operation":"task-get","task":{"namespace":"product","id":id}});
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let t = s.call(get.clone());
        if t["data"]["state"] == "failed" {
            assert_eq!(
                t["data"]["error_code"],
                "free_provider_authorization_required"
            );
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        s.call(json!({"operation":"task-cancel","task":{"namespace":"product","id":id}}))["data"]
            ["already_terminal"],
        true
    );
    let messages = json!({"operation":"session-get","session_id":session});
    let before = s.call(messages.clone());
    assert_eq!(
        before["data"]["messages"][0]["content"],
        "synthetic-persisted-input"
    );
    s.restart();
    assert_eq!(s.call(messages)["data"], before["data"]);
    assert_eq!(s.call(get)["data"]["state"], "failed");
    let next = s.call(json!({"operation":"conversation","session_id":session,"text":"second"}));
    assert!(next["data"]["task_id"].as_u64().unwrap() > id);
    assert_eq!(
        s.call(json!({"operation":"conversation","session_id":999999,"text":"invalid"}))
            ["error_code"],
        "session_invalid"
    );
    assert_eq!(
        s.call(json!({"operation":"status"}))["data"]["agent_execution_authority"],
        false
    );
}
#[test]
fn product_crash_recovery_keeps_input_and_does_not_reuse_ids_or_replay() {
    let mut s = Server::start();
    let session = s.call(json!({"operation":"session-create"}))["data"]["session_id"]
        .as_i64()
        .unwrap();
    // Fault injection in the existing authoritative database: uncertain run at SIGKILL.
    let dbpath = s
        .home
        .path()
        .join(".local/state/narys/core/db/luna.sqlite3");
    let db = rusqlite::Connection::open(dbpath).unwrap();
    db.execute(
        "INSERT INTO conversation_messages(session_id,role,content) VALUES(?1,'user','uncertain')",
        [session],
    )
    .unwrap();
    let message = db.last_insert_rowid();
    db.execute("INSERT INTO conversation_runs(task_id,session_id,user_message_id,state) VALUES(711,?1,?2,'running')",rusqlite::params![session,message]).unwrap();
    drop(db);
    s.stop(libc::SIGKILL);
    s.child = Server::launch(s.home.path(), s.runtime.path());
    s.ready();
    let get = json!({"operation":"task-get","task":{"namespace":"product","id":711}});
    let recovered = s.call(get.clone());
    assert_eq!(recovered["data"]["state"], "interrupted");
    assert_eq!(recovered["data"]["error_code"], "restart_never_retries");
    s.restart();
    assert_eq!(s.call(get)["data"], recovered["data"]);
    let events = s.call(json!({"operation":"events"}));
    assert_eq!(
        events["data"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["task_id"] == 711 && e["code"] == "restart_never_retries")
            .count(),
        1
    );
    let accepted =
        s.call(json!({"operation":"conversation","session_id":session,"text":"after-recovery"}));
    assert!(accepted["data"]["task_id"].as_u64().unwrap() > 711);
}

impl Server {
    fn cli(&self, args: &[&str]) -> std::process::Output {
        std::fs::set_permissions(
            self.runtime.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_narys"))
            .args(args)
            .env_clear()
            .env("HOME", self.home.path())
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .output()
            .unwrap()
    }
}
#[test]
fn official_cli_human_json_pages_permissions_and_unlock_denial() {
    let s = Server::start();
    for args in [
        vec!["status"],
        vec!["doctor"],
        vec!["models"],
        vec!["providers"],
        vec!["sessions"],
        vec!["tasks"],
        vec!["events"],
        vec!["credentials", "status"],
    ] {
        let human = s.cli(&args);
        assert!(
            human.status.success(),
            "{:?} {}",
            args,
            String::from_utf8_lossy(&human.stderr)
        );
        assert!(!human.stdout.starts_with(b"{"));
        let mut json_args = args.clone();
        json_args.push("--json");
        let structured = s.cli(&json_args);
        assert!(structured.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&structured.stdout).unwrap()["ok"],
            true
        );
    }
    for args in [
        vec!["sessions", "--limit", "101"],
        vec!["events", "--limit", "129"],
        vec!["tasks", "--after", "18446744073709551615"],
        vec!["provider", "groq", "enable"],
        vec!["approval", "future", "approve-once"],
        vec!["credentials", "unlock"],
        vec!["chat"],
    ] {
        let mut args = args;
        args.push("--json");
        let out = s.cli(&args);
        assert!(!out.status.success(), "accepted {:?}", args);
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["ok"], false);
        if args[0] == "approval" {
            assert_eq!(value["error_code"], "capability_not_integrated");
        }
    }
    for operation in ["unlock", "credentials-unlock"] {
        assert_eq!(
            s.call(json!({"operation":operation,"password":"synthetic-must-not-be-admitted"}))
                ["ok"],
            false
        );
    }
    assert_eq!(
        s.call(json!({"operation":"status"}))["data"]["graphical_environment_present"],
        false
    );
}
#[test]
fn official_cli_disconnected_clients_recover_cancelled_task_without_replay() {
    let mut s = Server::start();
    let created = s.cli(&["session", "new", "--json"]);
    let session: Value = serde_json::from_slice(&created.stdout).unwrap();
    let id = session["data"]["session_id"].as_i64().unwrap();
    // Synthetic product execution is admitted directly: no production keyring involved.
    let admitted = s
        .call(json!({"operation":"conversation","session_id":id,"text":"synthetic-cli-reconnect"}));
    let task = admitted["data"]["task_id"].as_u64().unwrap().to_string();
    let result = s.cli(&["task", &task, "--wait", "--timeout", "10", "--json"]);
    assert!(result.status.success());
    let before: Value = serde_json::from_slice(&result.stdout).unwrap();
    let cancel = s.cli(&["cancel", &task, "--json"]);
    assert!(cancel.status.success());
    s.restart();
    let after = s.cli(&["task", &task, "--json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&after.stdout).unwrap()["data"],
        before["data"]
    );
    let listed = s.cli(&["tasks", "--limit", "1", "--json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&listed.stdout).unwrap()["data"]["tasks"][0]["task_id"],
        task.parse::<u64>().unwrap()
    );
    let history = s.cli(&["session", &id.to_string(), "--json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&history.stdout).unwrap()["data"]["messages"][0]["content"],
        "synthetic-cli-reconnect"
    );
    assert_eq!(
        s.call(json!({"operation":"status"}))["data"]["product_active_tasks"],
        0
    );
}
#[test]
fn specialist_queries_disconnect_reentry_and_restart_are_lazy_and_durable() {
    let mut s = Server::start();
    for _ in 0..8 {
        let status = s.call(json!({"operation":"agent-status"}));
        assert_eq!(status["data"]["runtime"]["state"], "dormant");
        assert_eq!(status["data"]["runtime"]["process_id"], Value::Null);
        assert_eq!(status["data"]["runtime"]["generation"], 0);
    }
    // The temporary HOME deliberately has no CLI. Disconnect before reading
    // the admission response; the server-owned task survives the connection.
    let mut client = UnixStream::connect(s.socket()).unwrap();
    client.write_all(&serde_json::to_vec(&json!({"version":1,"request_id":"detached","command":{"operation":"agent-session-create"}})).unwrap()).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    drop(client);
    let deadline = Instant::now() + Duration::from_secs(5);
    let id = loop {
        let tasks = s.call(json!({"operation":"tasks","namespace":"product"}));
        if let Some(task) = tasks["data"]["tasks"].as_array().unwrap().first() {
            break task["task_id"].as_u64().unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    let task = loop {
        let task = s.call(json!({"operation":"task-get","task":{"namespace":"product","id":id}}));
        if task["data"]["state"] == "failed" {
            break task["data"].clone();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(task["error_code"], "cli_unavailable");
    assert_eq!(task["sdk_send_calls"], 0);
    let reference = task["session_ref"].clone();
    let attach = s.call(json!({"operation":"agent-session-attach","session_ref":reference}));
    assert_eq!(s.call(json!({"operation":"agent-session-detach","attachment_id":attach["data"]["attachment_id"]}))["data"]["task_cancelled"],false);
    let event = s.call(json!({"operation":"events"}));
    let text = event.to_string();
    assert!(text.contains("agent_failed"));
    assert!(text.contains("copilot"));
    s.restart();
    assert_eq!(
        s.call(json!({"operation":"task-get","task":{"namespace":"product","id":id}}))["data"]
            ["state"],
        "failed"
    );
    assert_eq!(
        s.call(json!({"operation":"agent-session-get","session_ref":reference}))["data"]
            ["attachments"],
        0
    );
    assert_eq!(
        s.call(json!({"operation":"agent-session-resume","session_ref":reference}))["error_code"],
        "session_not_resumable"
    );
    assert_eq!(
        s.call(json!({"operation":"agent-status"}))["data"]["runtime"]["generation"],
        0
    );
}
#[test]
fn specialist_maintenance_stop_is_typed_and_keeps_conversation_available() {
    let s = Server::start();
    let stopped = s.call(json!({"operation":"agent-runtime-stop"}));
    assert_eq!(
        stopped["data"]["runtime"]["admission_closed"], true,
        "{stopped}"
    );
    assert_eq!(
        s.call(json!({"operation":"agent-session-create"}))["error_code"],
        "supervisor_stopping"
    );
    assert_eq!(s.call(json!({"operation":"session-create"}))["ok"], true);
    assert_eq!(
        s.call(json!({"operation":"status"}))["data"]["core"],
        "running"
    );
}
