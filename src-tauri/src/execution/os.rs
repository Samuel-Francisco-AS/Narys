//! Linux mechanisms, deliberately not a sandbox. Only owned child/session IDs
//! are signalled; the unreaped leader pins the ID until the final cleanup signal.
use super::*;
use std::{
    io::{self, Read},
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    path::Path,
};

pub(super) fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: borrowed live descriptor; fcntl changes flags, never ownership.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
/// Observe without reaping, so cleanup cannot signal a reused leader PID/PGID.
pub(super) fn exited(pid: u32) -> io::Result<bool> {
    // SAFETY: initialized output struct, waitid only observes our direct child.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { info.si_pid() } != 0)
    }
}
pub(super) fn signal_group(group: i32, signal: i32) {
    if group > 1 {
        // SAFETY: positive, internally owned group; negative PID means group.
        unsafe {
            libc::kill(-group, signal);
        }
    }
}
fn pidfd(pid: u32) -> io::Result<OwnedFd> {
    // SAFETY: Linux syscall with documented integer args. A nonnegative return
    // is a newly owned descriptor, transferred exactly once to OwnedFd.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
    }
}
fn pidfd_signal(fd: &OwnedFd, signal: i32) -> io::Result<()> {
    // SAFETY: live owned pidfd, null siginfo means ordinary signal, flags zero.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
/// Fail closed before PTY spawn on kernels/seccomp policies lacking safe session
/// member signalling. pidfd_open needs Linux >=5.3. Signal 0 changes no state.
pub(super) fn check_session_cleanup() -> io::Result<()> {
    std::fs::read_dir("/proc")?;
    pidfd_signal(&pidfd(std::process::id())?, 0)
}
fn group_and_session(path: &Path) -> Option<(i32, u32)> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_PROC_STAT_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    // comm can contain non-UTF8 bytes, whitespace and ')'. Numeric fields are
    // ASCII after the final ')', independently of the comm representation.
    let text = String::from_utf8_lossy(&bytes);
    let (_, fields) = text.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    fields.next()?;
    fields.next()?;
    Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
}
/// PTY shells use job control: jobs may have different PGIDs within the session.
/// The leader's unreaped PID pins its group. Other members use pidfds, never
/// scan-derived kill(-PGID): a reaped job PGID could otherwise be reused between
/// scan and signal. Revalidate session after opening the pidfd; a dying/reused
/// numeric PID cannot redirect the signal to an unrelated live process.
/// setsid/daemon escape remains unsupported, not a sandbox.
pub(super) fn signal_session(pid: u32, signal: i32) -> bool {
    signal_group(pid as i32, signal);
    let Ok(mut entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    let mut complete = true;
    for entry in entries.by_ref().take(MAX_PROC_SCAN_ENTRIES).flatten() {
        let Some(member) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let stat = entry.path().join("stat");
        let Some((group, session)) = group_and_session(&stat) else {
            continue;
        };
        if session != pid || group == pid as i32 {
            continue;
        }
        if let Ok(fd) = pidfd(member) {
            if group_and_session(&stat).is_some_and(|(_, session)| session == pid) {
                if let Err(e) = pidfd_signal(&fd, signal) {
                    if e.raw_os_error() != Some(libc::ESRCH) {
                        complete = false;
                    }
                }
            }
        } else if group_and_session(&stat).is_some_and(|(_, session)| session == pid) {
            complete = false;
        }
    }
    if entries.next().is_some() {
        complete = false;
    }
    complete
}
pub(super) fn poll_pause() {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

/// Exceptional kernel-stuck child: retain the owned child in the existing,
/// counted supervisor/slot. No unbounded reaper queue or extra thread. Terminal
/// cancellation is already published; app shutdown waits only its deadline.
/// A process in uninterruptible kernel sleep cannot be synchronously reaped by
/// any userspace deadline. Once it exits, reap and update the cleanup facts.
pub(super) fn deferred_reap(
    pid: u32,
    mut wait: impl FnMut() -> io::Result<(Option<i32>, Option<String>)>,
    handle: &ExecutionHandle,
) {
    loop {
        match exited(pid) {
            Ok(true) => {
                if let Ok((exit_code, signal)) = wait() {
                    let mut data = handle
                        .control
                        .data
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    if let Some(result) = &data.result {
                        let mut updated = (**result).clone();
                        updated.exit_code = exit_code;
                        updated.signal = signal;
                        updated.reaped = true;
                        updated.cleanup_pending = false;
                        data.result = Some(std::sync::Arc::new(updated));
                    }
                }
                return;
            }
            Ok(false) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
}

pub(super) fn drain<R: std::io::Read>(
    mut reader: R,
    cap: usize,
    stop: &std::sync::atomic::AtomicBool,
) -> CapturedOutput {
    let mut output = CapturedOutput::default();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            output.incomplete = true;
            break;
        }
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => output.append(&chunk[..n], cap),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => poll_pause(),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                output.read_error = true;
                break;
            }
        }
    }
    output
}
