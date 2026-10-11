//! Diagnostic boundary for offline, disposable subprocesses only.
//! This is NOT an admission certificate for the Copilot runtime. In particular,
//! mounting credentials or enabling provider networking invalidates this proof.
#[cfg(test)]
use std::{os::unix::fs::MetadataExt, path::Path, process::Command};

#[cfg(test)]
pub(super) fn command(
    workspace: &Path,
    executable: &str,
    arguments: &[&str],
) -> super::Result<Command> {
    super::validate_workspace(workspace)?;
    // A writable bind must not contain host sockets, devices, hardlinked files
    // or entries on another filesystem. Same-device bind mounts are not detected;
    // this diagnostic accepts only a fresh ordinary disposable tree.
    // Concurrent hostile host writers are outside this diagnostic's guarantee;
    // production admission remains unavailable.
    fn tree(path: &Path, device: u64, budget: &mut usize) -> super::Result<()> {
        if *budget == 0 {
            return Err("sandbox_workspace_limit");
        }
        *budget -= 1;
        let m = std::fs::symlink_metadata(path).map_err(|_| "sandbox_workspace_unavailable")?;
        if m.dev() != device
            || m.file_type().is_symlink()
            || (!m.is_file() && !m.is_dir())
            || (m.is_file() && m.nlink() != 1)
        {
            return Err("sandbox_workspace_alias_denied");
        }
        if m.is_dir() {
            for entry in std::fs::read_dir(path).map_err(|_| "sandbox_workspace_unavailable")? {
                tree(
                    &entry.map_err(|_| "sandbox_workspace_unavailable")?.path(),
                    device,
                    budget,
                )?;
            }
        }
        Ok(())
    }
    tree(
        workspace,
        std::fs::metadata(workspace)
            .map_err(|_| "sandbox_workspace_unavailable")?
            .dev(),
        &mut 4096,
    )?;
    let bwrap = Path::new("/usr/bin/bwrap");
    let m = std::fs::symlink_metadata(bwrap).map_err(|_| "sandbox_unavailable")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err("sandbox_binary_untrusted");
    }
    let mut command = Command::new(bwrap);
    command
        .env_clear()
        .args([
            "--unshare-all",
            "--unshare-user",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--disable-userns",
            "--assert-userns-disabled",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/lib64",
            "/lib64",
            "--symlink",
            "usr/lib",
            "/lib",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--tmpfs",
            "/home",
            "--tmpfs",
            "/run",
            "--clearenv",
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "HOME",
            "/home",
            "--setenv",
            "LANG",
            "C.UTF-8",
            "--bind",
        ])
        .arg(workspace)
        .args([
            "/workspace",
            "--chdir",
            "/workspace",
            "--remount-ro",
            "/",
            "--",
            executable,
        ])
        .args(arguments);
    Ok(command)
}
