//! Boundary for explicitly offline, Core-owned disposable subprocesses only.
//! This is NOT an admission certificate for the Copilot runtime. In particular,
//! mounting credentials or enabling provider networking invalidates this proof.
use std::{
    fs::File,
    os::fd::AsRawFd,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    process::Command,
};

#[cfg(test)]
pub(super) fn command(
    workspace: &Path,
    executable: &str,
    arguments: &[&str],
) -> super::Result<Command> {
    command_mode(workspace, executable, arguments, false)
}
#[cfg(test)]
pub(super) fn command_mode(
    workspace: &Path,
    executable: &str,
    arguments: &[&str],
    readonly: bool,
) -> super::Result<Command> {
    build(workspace, executable, arguments, readonly, None)
}
fn build(
    workspace: &Path,
    executable: &str,
    arguments: &[&str],
    readonly: bool,
    fd: Option<i32>,
) -> super::Result<Command> {
    super::validate_workspace(workspace)?;
    // Host mount aliases are not admissible in the operational local boundary.
    let mounts = std::fs::read_to_string("/proc/self/mountinfo")
        .map_err(|_| "sandbox_mounts_unavailable")?;
    for line in mounts.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if let Some(mount) = fields.get(4) {
            let decoded = mount
                .replace("\\040", " ")
                .replace("\\011", "\t")
                .replace("\\012", "\n")
                .replace("\\134", "\\");
            let path = Path::new(&decoded);
            if path == workspace || path.starts_with(workspace) {
                return Err("sandbox_workspace_submount_denied");
            }
        }
    }
    // A writable bind must not contain host sockets, devices, hardlinked files
    // or entries on another filesystem. Operational admission additionally rejects
    // submounts using mountinfo; only Core-owned fresh disposable trees qualify.
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
            "--dir",
            "/usr",
            "--ro-bind",
            "/usr/bin",
            "/usr/bin",
            "--ro-bind",
            "/usr/lib64",
            "/usr/lib64",
            "--ro-bind",
            "/usr/lib",
            "/usr/lib",
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
            "--size",
            "16777216",
            "--tmpfs",
            "/tmp",
            "--size",
            "16777216",
            "--tmpfs",
            "/home",
            "--size",
            "16777216",
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
        ])
        .arg(match (readonly, fd.is_some()) {
            (true, true) => "--ro-bind-fd",
            (false, true) => "--bind-fd",
            (true, false) => "--ro-bind",
            (false, false) => "--bind",
        })
        .arg(
            fd.map(|n| n.to_string())
                .unwrap_or_else(|| workspace.to_string_lossy().into_owned()),
        )
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

pub(super) fn pinned(
    workspace: &Path,
    executable: &str,
    arguments: &[&str],
    readonly: bool,
) -> super::Result<(Command, File)> {
    let identity = super::validate_workspace(workspace)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(workspace)
        .map_err(|_| "workspace_unavailable")?;
    let m = file.metadata().map_err(|_| "workspace_unavailable")?;
    if identity != (m.dev(), m.ino()) {
        return Err("workspace_identity_changed");
    }
    Ok((
        build(
            workspace,
            executable,
            arguments,
            readonly,
            Some(file.as_raw_fd()),
        )?,
        file,
    ))
}
