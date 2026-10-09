//! Offline FIX-3 boundary. Host authentication is deliberately not exposed.
use crate::{explicit_program, options};
use github_copilot_sdk::{CliProgram, ClientMode, ClientOptions};
use std::{
    path::Path,
    process::{Command, Stdio},
};

pub fn isolated_options(
    cli: &Path,
    workspace: &Path,
    state: &Path,
    sessions: &Path,
) -> Result<ClientOptions, &'static str> {
    let cli = explicit_program(cli)?;
    let logs = state.parent().ok_or("invalid_state_root")?.join("logs");
    let script = format!("{}/boundary.py", env!("CARGO_MANIFEST_DIR"));
    let preflight = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(&script)
        .arg("check")
        .args([cli.as_path(), workspace, state, sessions, logs.as_path()])
        .env_clear()
        .env("PATH", "/usr/bin")
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "boundary_preflight_unavailable")?;
    if !preflight.status.success() {
        // Fixed allowlist only. Never echo arbitrary helper output or paths.
        let data: serde_json::Value =
            serde_json::from_slice(&preflight.stdout).map_err(|_| "boundary_preflight_invalid")?;
        return Err(match data["code"].as_str() {
            Some("private_tmp_job_required") => "private_tmp_job_required",
            Some("unsafe_directory") => "unsafe_directory",
            Some("data_outside_owned_job") => "data_outside_owned_job",
            Some("cli_pin_mismatch") => "cli_pin_mismatch",
            Some("native_elf_required") => "native_elf_required",
            Some("dependency_unavailable") => "dependency_unavailable",
            _ => "boundary_preflight_failed",
        });
    }
    let mut opts = options(
        cli.clone(),
        workspace,
        sessions.parent().ok_or("invalid_state_root")?,
    );
    opts.program = CliProgram::Path(explicit_program(Path::new("/usr/bin/python3"))?);
    opts.prefix_args = vec![
        "-I".into(),
        format!("{}/boundary.py", env!("CARGO_MANIFEST_DIR")).into(),
        "launch".into(),
        cli.into(),
        workspace.into(),
        state.into(),
        sessions.into(),
        logs.into(),
        "--".into(),
    ];
    opts.mode = ClientMode::Empty;
    opts.use_logged_in_user = Some(false);
    opts.extra_args = vec![
        "--disable-builtin-mcps".into(),
        "--log-dir".into(),
        "/logs".into(),
    ];
    opts.env_remove.extend(
        [
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "PYTHONPATH",
            "PYTHONHOME",
            "DBUS_SESSION_BUS_ADDRESS",
            "SSH_AUTH_SOCK",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
        ]
        .into_iter()
        .map(Into::into),
    );
    Ok(opts)
}
