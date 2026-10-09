#!/usr/bin/env python3
"""FIX-3 offline least-visibility experiment, never production sandbox/A9 runner.
Only native ELF + individual system ELF dependencies + owned temporary data.
No host auth, session bus, shared networking, fallback mounts or inference flags.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys

BWRAP = Path('/usr/bin/bwrap')
SAFE_ENV = {'HOME': '/home/poc', 'PATH': '/nonexistent', 'LANG': 'C',
            'TMPDIR': '/tmp', 'COPILOT_HOME': '/state',
            'XDG_CONFIG_HOME': '/state/config', 'XDG_CACHE_HOME': '/state/cache',
            'XDG_DATA_HOME': '/state/data', 'COPILOT_DISABLE_KEYTAR': '1',
            'COPILOT_RUNTIME_PROCESS_FILE_LOGGING': '0'}
# Linux x86_64 UAPI syscall numbers. Reject every other ABI, including x32.
DENIED_SYSCALLS = (101, 248, 249, 250, 155, 165, 166, 272, 298, 303, 304,
                   308, 310, 311, 312, 321, 323, 425, 426, 427)


class BoundaryError(Exception):
    """Fixed code only, never raw loader, argv, environment or credential prose."""


def directory(path):
    path = Path(path)
    try:
        stat = path.lstat()
        if not path.is_absolute() or path.is_symlink() or not path.is_dir():
            raise BoundaryError('unsafe_directory')
        if path.resolve() != path or stat.st_uid != os.getuid():
            raise BoundaryError('unsafe_directory')
    except OSError:
        raise BoundaryError('directory_unavailable') from None
    return path


def native_dependencies(program):
    try:
        program = Path(program).resolve(strict=True)
        with program.open('rb') as stream:
            if stream.read(4) != b'\x7fELF':
                raise BoundaryError('native_elf_required')
        if not os.access(program, os.X_OK):
            raise BoundaryError('runtime_not_executable')
        # Trusted installed CLI or our fixture only. No shell/interpreter launcher.
        found = subprocess.run(['/usr/bin/ldd', str(program)],
                               env={'PATH': '/usr/bin', 'LC_ALL': 'C'},
                               capture_output=True, text=True, timeout=5)
        if found.returncode or 'not found' in found.stdout:
            raise BoundaryError('dependency_unavailable')
        paths = sorted(set(re.findall(r'(/[^\s()]+)', found.stdout)))
        if not paths:
            raise BoundaryError('dependency_inventory_empty')
        libraries = []
        for name in paths:
            source = Path(name).resolve(strict=True)
            if not source.is_file() or not any(source.is_relative_to(p) for p in
                                              (Path('/usr/lib64'), Path('/usr/lib'))):
                raise BoundaryError('dependency_outside_system_library_roots')
            libraries.append((str(source), name))
        return program, libraries
    except BoundaryError:
        raise
    except (OSError, subprocess.SubprocessError, UnicodeError):
        raise BoundaryError('dependency_inventory_failed') from None


def seccomp_bytes():
    if platform.machine() != 'x86_64':
        raise BoundaryError('unsupported_seccomp_architecture')
    # sock_filter {u16 code,u8 jt,u8 jf,u32 k}; seccomp_data arch offset=4,nr=0.
    code = [(0x20, 0, 0, 4), (0x15, 1, 0, 0xC000003E),
            (0x06, 0, 0, 0x80000000), (0x20, 0, 0, 0),
            (0x35, 0, 1, 0x40000000), (0x06, 0, 0, 0x00050001)]
    for number in DENIED_SYSCALLS:
        code.extend([(0x15, 0, 1, number), (0x06, 0, 0, 0x00050001)])
    code.append((0x06, 0, 0, 0x7FFF0000))
    return b''.join(struct.pack('=HBBI', *row) for row in code)


def plan(program, workspace, state, store, logs, policy='offline-deny-all'):
    if policy != 'offline-deny-all':
        raise BoundaryError('unsupported_security_policy')
    seccomp_bytes()  # Mandatory architecture support checked before launch.
    if not BWRAP.is_file() or not os.access(BWRAP, os.X_OK):
        raise BoundaryError('bubblewrap_unavailable')
    workspace, state, store, logs = map(directory, (workspace, state, store, logs))
    job = directory(state.parent)
    if job.parent != Path('/tmp') or job.stat().st_mode & 0o077:
        raise BoundaryError('private_tmp_job_required')
    if workspace.parent != job or logs.parent != job or not store.is_relative_to(state):
        raise BoundaryError('data_outside_owned_job')
    if len({workspace, state, logs}) != 3 or not store.name == 'session-state':
        raise BoundaryError('invalid_data_layout')
    try:
        program = Path(program).resolve(strict=True)
    except OSError:
        raise BoundaryError('runtime_unavailable') from None
    if program == Path(__file__).resolve().parent / 'target/debug/boundary-fixture':
        kind = 'synthetic-fixture'
    elif program.name == 'copilot':
        with program.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        if digest != 'be17b42705ca17490098d7b87f293300d72a094d125b6bb2b2557dc4a0a4f8a8':
            raise BoundaryError('cli_pin_mismatch')
        kind = 'sdk-cli'
    else:
        raise BoundaryError('unapproved_native_program')
    # ldd is not a safe parser for arbitrary untrusted ELF: approve/pin FIRST.
    program, libraries = native_dependencies(program)
    if kind == 'sdk-cli':
        # Bun/node-pty dlopen dependency observed in the protected startup probe.
        util = Path('/usr/lib64/libutil.so.1').resolve(strict=True)
        libraries.append((str(util), '/lib64/libutil.so.1'))
    args = [str(BWRAP), '--unshare-all', '--unshare-user', '--disable-userns', '--assert-userns-disabled',
            '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv',
            '--tmpfs', '/', '--dir', '/runtime', '--dir', '/home/poc', '--dir', '/run',
            '--ro-bind', str(program), '/runtime/program']
    for source, destination in libraries:
        args.extend(['--ro-bind', source, destination])
    args.extend(['--ro-bind', str(workspace), '/fixture',
                 '--bind', str(store.parent), '/state', '--bind', str(logs), '/logs',
                 '--tmpfs', '/tmp', '--tmpfs', '/dev',
                 '--dev-bind', '/dev/null', '/dev/null',
                 '--dev-bind', '/dev/urandom', '/dev/urandom',
                 '--proc', '/proc', '--remount-ro', '/proc'])
    for key, value in SAFE_ENV.items():
        args.extend(['--setenv', key, value])
    args.extend(['--chdir', '/fixture', '--remount-ro', '/'])
    return {'schema_version': 1, 'policy': policy, 'program_kind': kind, 'args': args,
            'library_mounts': libraries, 'environment_names': sorted(SAFE_ENV),
            'network': 'private_offline_namespace', 'auth': 'no_host_credentials_or_bus',
            'seccomp': 'x86_64_deny_keyring_introspection_namespace_mount_io_uring',
            'a9': 'BLOCKED_AUTH_BOUNDARY_AND_NETWORK', 'inference_calls': 0}


def sdk_arguments(args):
    # Strict pinned SDK 1.0.17 stdio launch. No prompts/login/YOLO/plugins/fallback.
    flags = {'--server', '--stdio', '--no-auto-update', '--no-auto-login',
             '--disable-builtin-mcps'}
    seen = set()
    result = []
    i = 0
    while i < len(args):
        value = args[i]
        if value in flags and value not in seen:
            seen.add(value)
            result.append(value)
        elif value in ('--log-level', '--log-dir') and value not in seen:
            seen.add(value)
            i += 1
            if i >= len(args) or args[i] != ('none' if value == '--log-level' else '/logs'):
                raise BoundaryError('unsafe_runtime_arguments')
            result.extend([value, args[i]])
        else:
            raise BoundaryError('unsafe_runtime_arguments')
        i += 1
    if not flags.issubset(seen) or not {'--log-level', '--log-dir'}.issubset(seen):
        raise BoundaryError('required_runtime_arguments_missing')
    return result


def execute(built, arguments):
    if built['program_kind'] == 'sdk-cli':
        arguments = sdk_arguments(arguments)
    elif built['program_kind'] != 'synthetic-fixture' or arguments not in (['normal'], ['timeout']):
        raise BoundaryError('unapproved_fixture_arguments')
    fd = os.memfd_create('narys-fix3-seccomp', 0)
    try:
        os.write(fd, seccomp_bytes())
        os.lseek(fd, 0, os.SEEK_SET)
        os.set_inheritable(fd, True)
        # Retain only stdio and this non-secret filter FD, never ambient sockets.
        for entry in Path('/proc/self/fd').iterdir():
            number = int(entry.name)
            if number > 2 and number != fd:
                try:
                    os.close(number)
                except OSError:
                    pass
        argv = built['args'] + ['--seccomp', str(fd), '--', '/runtime/program'] + arguments
        os.execve(BWRAP, argv, {'PATH': '/usr/bin', 'LANG': 'C'})
    finally:
        os.close(fd)


def main():
    try:
        if len(sys.argv) == 7 and sys.argv[1] == 'check':
            built = plan(*sys.argv[2:7])
            print(json.dumps({'state': 'valid', 'library_mounts': built['library_mounts']}))
            return 0
        if len(sys.argv) < 9 or sys.argv[1] != 'launch' or sys.argv[7] != '--':
            raise BoundaryError('invalid_launcher_invocation')
        built = plan(*sys.argv[2:7])
        execute(built, sdk_arguments(sys.argv[8:]))
    except (BoundaryError, OSError) as error:
        # SDK/harness consume no raw errors. stderr receives fixed code only.
        code = str(error) if isinstance(error, BoundaryError) else 'boundary_launch_failed'
        if len(sys.argv) > 1 and sys.argv[1] == 'check':
            print(json.dumps({'state': 'blocked', 'code': code}))
        else:
            print(code, file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
