"""LR-10B private Linux runtime owner, embedded in the Core binary.
SDK stdio is inherited, never interpreted or logged here. Two private subreapers
cover either owner's death; only unreaped kernel children are signalled via pidfd.
No process-name, process-group, /proc scan, or external PID is authority.
"""
import ctypes
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time

WAIT_ALL = 0x40000000

def inventory():
    return [int(p) for p in Path(f'/proc/self/task/{os.getpid()}/children').read_text().split()]

def identity(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return int(fields[1]), int(fields[19])

def pin_child(pid):
    before = identity(pid)
    if before[0] != os.getpid():
        raise RuntimeError('not_owned')
    fd = os.pidfd_open(pid)
    if identity(pid) != before:
        os.close(fd)
        raise RuntimeError('identity_changed')
    return fd

def setup():
    signal.signal(signal.SIGCHLD, signal.SIG_DFL)
    if ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) != 0:
        raise RuntimeError('subreaper_unavailable')
    fd = os.pidfd_open(os.getpid())
    signal.pidfd_send_signal(fd, 0)
    os.close(fd)

def cleanup():
    deadline = time.monotonic() + 4
    reaped = killed = 0
    errors = set()
    exhausted = False
    while time.monotonic() < deadline:
        for pid in inventory():
            try:
                fd = pin_child(pid)
                try:
                    signal.pidfd_send_signal(fd, signal.SIGKILL)
                    killed += 1
                finally:
                    os.close(fd)
            except ProcessLookupError:
                pass
            except Exception:
                errors.add('ownership_or_signal_failed')
        while True:
            try:
                pid, _ = os.waitpid(-1, os.WNOHANG | WAIT_ALL)
                if pid == 0:
                    break
                reaped += 1
            except ChildProcessError:
                exhausted = not inventory()
                break
        if exhausted:
            break
        time.sleep(.01)  # Only during bounded recovery, never idle polling.
    return {'cleanup_complete': exhausted and not errors,
            'kernel_children_exhausted': exhausted,
            'reaped': reaped, 'recovery_signals': killed,
            'errors': sorted(errors)}

def save(directory, name, value):
    path = Path(directory) / name
    temporary = path.with_suffix('.new')
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as out:
        json.dump(value, out)
        out.flush()
        os.fsync(out.fileno())
    os.replace(temporary, path)
    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
    os.fsync(fd)
    os.close(fd)

def wait_for(child_fd, parent_fd):
    poller = select.poll()
    poller.register(child_fd, select.POLLIN)
    poller.register(parent_fd, select.POLLIN)
    return dict(poller.poll()).get(parent_fd) is not None

def run():
    os.umask(0o077)
    cli, directory = sys.argv[1:3]
    setup()
    parent_pid = os.getppid()
    parent = os.pidfd_open(parent_pid)
    if os.getppid() != parent_pid:
        return 1
    primary_pid = os.getpid()
    save(directory, 'primary-owner.json', {'primary_pid': primary_pid, 'primary_start_ticks': identity(primary_pid)[1]})
    guardian = os.fork()
    if guardian == 0:
        os.close(parent)
        setup()
        parent = os.pidfd_open(primary_pid)
        # If the primary died before pinning it, refuse to create the CLI.
        if os.getppid() != primary_pid:
            save(directory, 'cleanup.json', cleanup())
            os._exit(1)
        try:
            child = subprocess.Popen([cli, *sys.argv[3:]], stderr=subprocess.DEVNULL)
            child_fd = pin_child(child.pid)
            save(directory, 'owner.json', {'guardian_pid': os.getpid(),
                 'guardian_start_ticks': identity(os.getpid())[1],
                 'runtime_pid': child.pid, 'runtime_start_ticks': identity(child.pid)[1]})
            lost_parent = wait_for(child_fd, parent)
            if not lost_parent:
                code = child.wait()
            else:
                code = 1
            os.close(child_fd)
            evidence = cleanup()
            evidence['primary_owner_lost'] = lost_parent
            evidence['runtime_exit_code'] = code
            save(directory, 'cleanup.json', evidence)
            os._exit(0 if evidence['cleanup_complete'] else 1)
        except BaseException:
            evidence = cleanup()
            evidence['runtime_error'] = 'runtime_owner_failed'
            save(directory, 'cleanup.json', evidence)
            os._exit(1)
    guardian_fd = pin_child(guardian)
    lost_parent = wait_for(guardian_fd, parent)
    if not lost_parent:
        _, status = os.waitpid(guardian, 0)
        code = os.waitstatus_to_exitcode(status)
    else:
        code = 1
    os.close(guardian_fd)
    # A killed guardian reparents the CLI here; never infer cleanup from its exit.
    evidence = cleanup()
    path = Path(directory) / 'cleanup.json'
    if not path.exists():
        evidence['guardian_lost'] = True
        evidence['runtime_exit_code'] = 1
        save(directory, 'cleanup.json', evidence)
    elif not evidence['cleanup_complete']:
        save(directory, 'primary-cleanup.json', evidence)
        code = 1
    return code

if __name__ == '__main__':
    try:
        raise SystemExit(run())
    except Exception:
        # Fail closed, no exception prose/raw CLI content in journal or transport.
        raise SystemExit(1)
