//! Credential unlock is a local human operation, never an IPC command.
use serde_json::{json, Value};
use std::process::{Command, Stdio};
const HELPER: &str = include_str!("../ops/credential_manager.py");
fn helper(operation: &str) -> Command {
    let uid = unsafe { libc::geteuid() };
    let mut c = Command::new("/usr/bin/python3");
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path=/run/user/{uid}/bus"),
        )
        .args(["-I", "-c", HELPER, operation]);
    c
}
pub async fn status(runtime: &std::path::Path) -> Value {
    // Dedicated metadata-only code path; no session/key/prompt/item lookup.
    let mut command = helper("status");
    command.env(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={}/bus", runtime.display()),
    );
    let mut c = tokio::process::Command::from(command);
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        c.stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    match r {
        Ok(Ok(o))=>serde_json::from_slice(&o.stdout).unwrap_or_else(|_|json!({"service_available":false,"login_unlocked":false,"code":"credential_status_invalid"})),
        _=>json!({"service_available":false,"login_unlocked":false,"code":"credential_service_unavailable"}),
    }
}
pub fn unlock(structured: bool) -> Result<(), &'static str> {
    let operation = if structured { "unlock-json" } else { "unlock" };
    let status = helper(operation)
        .status()
        .map_err(|_| "credential_helper_unavailable")?;
    if status.success() {
        Ok(())
    } else {
        Err("human_unlock_refused_or_failed")
    }
}
