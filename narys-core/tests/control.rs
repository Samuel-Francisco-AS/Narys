use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
struct OwnedChild(std::process::Child);
impl std::ops::Deref for OwnedChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, self.0.id(), 0) } as i32;
            if fd >= 0 {
                unsafe {
                    libc::syscall(libc::SYS_pidfd_send_signal, fd, libc::SIGTERM, 0, 0);
                    libc::close(fd);
                }
            }
            let _ = self.0.wait();
        }
    }
}
fn call(home: &std::path::Path, runtime: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_narys-core"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", runtime)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent-narys-synthetic-bus",
        )
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)))
}
#[test]
fn real_local_core_restarts_tasks_without_inference_and_blocks_unreviewed_submit() {
    let home = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    fs::set_permissions(runtime.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let launch = || {
        OwnedChild(
            Command::new(env!("CARGO_BIN_EXE_narys-core"))
                .arg("serve")
                .env_clear()
                .env("HOME", home.path())
                .env("XDG_RUNTIME_DIR", runtime.path())
                .env(
                    "DBUS_SESSION_BUS_ADDRESS",
                    "unix:path=/nonexistent-narys-synthetic-bus",
                )
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let mut child = launch();
    let socket = runtime.path().join("narys-core/control.sock");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        call(home.path(), runtime.path(), &["status"])["data"]["core"],
        "running"
    );
    assert_eq!(
        call(home.path(), runtime.path(), &["credentials"])["data"]["login_unlocked"],
        false
    );
    let prepared = call(
        home.path(),
        runtime.path(),
        &["prepare", "Responda 5 sem ferramentas.", "5"],
    );
    let id = prepared["data"]["task_id"].as_u64().unwrap().to_string();
    assert_eq!(
        call(home.path(), runtime.path(), &["submit", &id])["ok"],
        false
    );
    assert_eq!(
        call(home.path(), runtime.path(), &["result", &id])["data"]["state"],
        "prepared"
    );
    let workspace = prepared["data"]["workspace"].as_str().unwrap();
    assert!(!std::path::Path::new(workspace)
        .parent()
        .unwrap()
        .join("send-attempt.json")
        .exists());
    // Signal the exact owned child identity via pidfd, never a numeric external PID.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) } as i32;
    assert!(fd >= 0);
    assert_eq!(
        unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, libc::SIGTERM, 0, 0) },
        0
    );
    unsafe {
        libc::close(fd);
    }
    assert!(child.wait().unwrap().success());
    assert!(!socket.exists());
    let mut child = launch();
    while !socket.exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        call(home.path(), runtime.path(), &["result", &id])["data"]["state"],
        "prepared"
    );
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) } as i32;
    assert_eq!(
        unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, libc::SIGTERM, 0, 0) },
        0
    );
    unsafe {
        libc::close(fd);
    }
    assert!(child.wait().unwrap().success());
    fs::remove_dir_all(std::path::Path::new(workspace).parent().unwrap()).unwrap();
}
