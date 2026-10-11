//! Offline boundary only. x86_64 classic BPF, installed by Bubblewrap AFTER
//! namespace setup and inherited by every descendant. No host-wide settings.
use super::Result;
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    os::fd::{AsRawFd, FromRawFd},
};
pub(super) fn filter() -> Result<File> {
    if std::env::consts::ARCH != "x86_64" {
        return Err("sandbox_seccomp_arch_unavailable");
    }
    let denied = [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_recvfrom,
        libc::SYS_recvmsg,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_io_uring_setup,
        libc::SYS_userfaultfd,
        libc::SYS_perf_event_open,
        libc::SYS_bpf,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
    ];
    let mut instructions: Vec<(u16, u8, u8, u32)> = vec![
        (0x20, 0, 0, 4),          // LD seccomp_data.arch
        (0x15, 1, 0, 0xc000003e), // JEQ AUDIT_ARCH_X86_64, skip kill
        (0x06, 0, 0, 0x80000000), // KILL_PROCESS other ABI
        (0x20, 0, 0, 0),          // LD syscall number
        (0x35, 0, 1, 0x40000000), // JGE x32 bit, reject alternate ABI
        (0x06, 0, 0, 0x00050001), // ERRNO EPERM
    ];
    for nr in denied {
        instructions.push((0x15, 0, 1, nr as u32));
        instructions.push((0x06, 0, 0, 0x00050001));
    }
    instructions.push((0x06, 0, 0, 0x7fff0000)); // ALLOW others; not a complete syscall allowlist
    let fd = unsafe {
        libc::memfd_create(
            c"narys-offline-seccomp".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err("sandbox_seccomp_unavailable");
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    for (code, jt, jf, k) in instructions {
        bytes.extend(code.to_ne_bytes());
        bytes.push(jt);
        bytes.push(jf);
        bytes.extend(k.to_ne_bytes());
    }
    file.write_all(&bytes)
        .map_err(|_| "sandbox_seccomp_unavailable")?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "sandbox_seccomp_unavailable")?;
    if unsafe {
        libc::fcntl(
            file.as_raw_fd(),
            libc::F_ADD_SEALS,
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
        )
    } < 0
    {
        return Err("sandbox_seccomp_unavailable");
    }
    Ok(file)
}
