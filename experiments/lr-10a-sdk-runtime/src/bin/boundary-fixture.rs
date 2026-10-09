//! Synthetic kernel-boundary probe. No SDK, credentials, inference or tools.
use serde_json::{json, Value};
use std::{
    fs,
    net::{SocketAddr, TcpStream},
    os::unix::net::UnixStream,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
unsafe extern "C" {
    fn syscall(number: i64, ...) -> i64;
}
fn denied_read(path: &str) -> bool {
    fs::read(path).is_err()
}
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "child" {
        fs::write("/state/child-ready", "synthetic readiness").unwrap();
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    if mode == "timeout" {
        let _child = Command::new("/runtime/program")
            .arg("child")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Readiness barrier, not a timing-dependent creation race.
        while !Path::new("/state/child-ready").exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        fs::write("/state/tree-ready", "ready").unwrap();
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    let cfg: Value = serde_json::from_slice(&fs::read("/fixture/probe.json").unwrap()).unwrap();
    let canary = cfg["outside"].as_str().unwrap();
    let names = [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "COPILOT_SDK_AUTH_TOKEN",
        "COPILOT_GITHUB_TOKEN",
        "COPILOT_PROVIDER_API_KEY",
        "COPILOT_PROVIDER_BASE_URL",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "SSH_AUTH_SOCK",
        "DBUS_SESSION_BUS_ADDRESS",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "PYTHONPATH",
        "GIT_CONFIG_GLOBAL",
        "AWS_SECRET_ACCESS_KEY",
        "SYNTHETIC_SECRET",
        "DISPLAY",
        "WAYLAND_DISPLAY",
    ];
    let absent: Vec<_> = names
        .iter()
        .filter(|name| std::env::var_os(name).is_none())
        .collect();
    let address: SocketAddr = cfg["tcp"].as_str().unwrap().parse().unwrap();
    let socket = cfg["socket"].as_str().unwrap();
    // Query-only keyctl GET_KEYRING_ID, create=false; deny before inspecting any key.
    let keyring_denied = unsafe { syscall(250, 0i64, -3i64, 0i64) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(1);
    let no_new_privs = fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .any(|line| line == "NoNewPrivs:\t1");
    let devices: Vec<_> = fs::read_dir("/dev")
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let checks = json!({
        "fixture_read":fs::read("/fixture/fixture.txt").is_ok(),
        "fixture_write_denied":fs::write("/fixture/fixture.txt", "unauthorized").is_err(),
        "private_state_write":fs::write("/state/probe-state", "allowed").is_ok(),
        "private_logs_write":fs::write("/logs/probe-log", "allowed").is_ok(),
        "outside_read_denied":denied_read(canary),
        "outside_write_denied":fs::write(canary,"unauthorized").is_err(),
        "personal_roots_absent":(["/home/sam", "/root", "/home/poc/.ssh", "/etc/gitconfig", "/run/user"].iter().all(|p| !Path::new(p).exists())),
        "symlink_read_denied":denied_read("/fixture/escape-link"),
        "traversal_read_denied":denied_read("/fixture/../outside-canary"),
        "root_write_denied":fs::write("/outside-canary", "unauthorized").is_err(),
        "environment_absent":absent.len()==names.len(),
        "host_tcp_denied":TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_err(),
        "host_unix_socket_denied":UnixStream::connect(socket).is_err(),
        "shell_binary_absent":Command::new("/bin/sh").arg("-c").arg("exit 0").status().is_err(),
        "namespace_proc_only":fs::read_dir("/proc").unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().parse::<u32>().is_ok()).count() <= 4,
        "minimal_devices":devices.len()==2 && devices.iter().all(|s| s=="null" || s=="urandom"),
        "kernel_keyring_denied":keyring_denied,"no_new_privileges":no_new_privs});
    let pass = checks
        .as_object()
        .unwrap()
        .values()
        .all(|v| v == &json!(true));
    println!(
        "{}",
        json!({"checks":checks,"sensitive_environment_names_absent":absent,
        "sensitive_values_exported":false,"inference_calls":0,"passed":pass})
    );
    if !pass {
        std::process::exit(1);
    }
}
