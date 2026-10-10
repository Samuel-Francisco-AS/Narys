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
        json!({"operation":"conversation","session_id":1,"text":"hello"}),
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
