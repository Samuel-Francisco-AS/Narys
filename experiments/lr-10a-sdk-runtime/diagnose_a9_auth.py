#!/usr/bin/env python3
"""A9-FIX-1 metadata matrix. No sessions, tokens, login or attempt claim.

Values of approved non-secret context variables stay in process memory only.
The real metadata binary has no send entry point. FIX-1 owns each invocation.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess

from measure import identity, measure
from run_a9_host import CLI_SHA, ROOT
from run_fix2 import config_stat, digest

CONTEXT = ('HOME', 'PATH', 'DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR',
           'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'GH_CONFIG_DIR')
PRESENCE = CONTEXT + ('COPILOT_HOME', 'DISPLAY', 'WAYLAND_DISPLAY', 'GH_TOKEN',
                     'GITHUB_TOKEN', 'COPILOT_GITHUB_TOKEN',
                     'COPILOT_SDK_AUTH_TOKEN', 'GITHUB_COPILOT_API_TOKEN',
                     'COPILOT_DISABLE_KEYTAR')
ERROR_CODES = frozenset(('unsafe_marker_directory', 'unsafe_helper_path',
    'helper_path_unavailable', 'home_unavailable', 'invalid_context_path',
    'cli_pin_mismatch', 'config_metadata_unavailable', 'attempt_marker_exists',
    'unexpected_graphical_session', 'credential_service_activation_not_authorized',
    'unsafe_or_incomplete_probe_stop', 'service_metadata_changed'))


def attempt_state(home):
    """Read-only, anchored metadata. Never prepare/claim/read an attempt file."""
    fd = os.open(home, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for name in ('.local', 'state', 'narys'):
            child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=fd)
            meta = os.fstat(child)
            if meta.st_uid != os.getuid() or (name == 'narys' and
                    stat.S_IMODE(meta.st_mode) != 0o700):
                os.close(child)
                raise ValueError('unsafe_marker_directory')
            os.close(fd)
            fd = child
        try:
            os.stat('lr10a-a9-host-attempt.json', dir_fd=fd, follow_symlinks=False)
            return True  # Including corrupt, unreadable and symlink entries.
        except FileNotFoundError:
            return False
    finally:
        os.close(fd)


def checked_path(value):
    """Reject relative/empty or writable helper directories; omit missing ones.

    Not executable integrity or a sandbox. Inherited PATH is diagnostic only.
    No directory listing or personal file contents are inspected.
    """
    result = []
    for entry in value.split(':'):
        p = Path(entry)
        if not entry or not p.is_absolute():
            raise ValueError('unsafe_helper_path')
        try:
            meta = p.stat()
        except FileNotFoundError:
            continue
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid not in (0, os.getuid()) or meta.st_mode & 0o022:
            raise ValueError('unsafe_helper_path')
        if entry not in result:
            result.append(entry)
    if not result:
        raise ValueError('helper_path_unavailable')
    return ':'.join(result)


def variants(context):
    base = {k: context[k] for k in ('HOME', 'DBUS_SESSION_BUS_ADDRESS',
                                  'XDG_RUNTIME_DIR') if k in context}
    if 'HOME' not in base:
        raise ValueError('home_unavailable')
    base.update(PATH='/usr/bin', LANG='C', COPILOT_SKIP_CLI_DOWNLOAD='1')
    yield 'a9_baseline', 'none', base
    if context.get('PATH'):
        restored = checked_path(context['PATH'])
        if restored != base['PATH']:
            yield 'validated_original_path', 'PATH', dict(base, PATH=restored)
    for name in ('DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR'):
        if name in base:
            yield 'without_' + name.lower(), name, {k: v for k, v in base.items() if k != name}
    for name in ('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'GH_CONFIG_DIR'):
        if name in context:
            if not Path(context[name]).is_absolute():
                raise ValueError('invalid_context_path')
            yield 'restore_' + name.lower(), name, dict(base, **{name: context[name]})


def headless_metadata(context):
    # Non-activating D-Bus query to the bus daemon, never the credential service.
    env = {k: context[k] for k in ('HOME', 'DBUS_SESSION_BUS_ADDRESS',
                                  'XDG_RUNTIME_DIR') if k in context}
    env.update(PATH='/usr/bin', LANG='C')
    result = {}
    for name in ('gnome-shell', 'gdm', 'gnome-keyring-daemon'):
        p = subprocess.run(['/usr/bin/ps', '-C', name, '-o', 'pid='], env=env,
                           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL)
        result[name + '_running'] = p.returncode == 0
    p = subprocess.run(['/usr/bin/busctl', '--user', '--timeout=2', 'call',
                        'org.freedesktop.DBus', '/org/freedesktop/DBus',
                        'org.freedesktop.DBus', 'NameHasOwner', 's',
                        'org.freedesktop.secrets'], env=env, stdin=subprocess.DEVNULL,
                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    result['secrets_service_already_owned'] = p.returncode == 0 and p.stdout.strip() == b'b true'
    result['bus_query_exit_code'] = p.returncode
    socket = Path(context.get('XDG_RUNTIME_DIR', '/nonexistent')) / 'bus'
    try:
        result['runtime_bus_socket_accessible'] = stat.S_ISSOCK(socket.stat().st_mode) and os.access(socket, os.R_OK | os.W_OK)
    except OSError:
        result['runtime_bus_socket_accessible'] = False
    return result


def identities_absent(report):
    for row in report.get('attributed_processes', []):
        try:
            if identity(row['pid'])['start_ticks'] == row['start_ticks']:
                return False
        except FileNotFoundError:
            pass
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cli', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    # Reserve fresh private evidence before doing any real probe. No raw output.
    fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as output:
        result = {'phase': 'LR-10A A9-FIX-1', 'schema_version': 1,
                  'base': '938dc40ad3575435b97479fa14b8f3a2b1f7506b',
                  'mode': 'HOST_ASSISTED_NOT_SANDBOX', 'variants': [],
                  'sdk_send_calls': 0, 'real_session_operations': 0,
                  'marker_claimed': False, 'a9_real': 'NOT_RUN'}
        # Deliberately not dict(os.environ): token values are never acquired.
        context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
        result['environment_presence'] = {k: k in os.environ for k in PRESENCE}
        result['gh_available_usr_bin'] = Path('/usr/bin/gh').is_file()
        result['gh_same_resolution_original_and_baseline'] = shutil.which('gh', path=context.get('PATH', '')) == shutil.which('gh', path='/usr/bin')
        try:
            result['verification_stage'] = 'installation_and_plan'
            cli = args.cli.resolve(strict=True)
            binary = ROOT / 'target/debug/a9-host-assisted'
            if digest(cli) != CLI_SHA:
                raise ValueError('cli_pin_mismatch')
            result['cli_sha256'] = digest(cli)
            result['binary_sha256'] = digest(binary)
            result['source_sha256'] = {p: digest(ROOT / p) for p in
                ('diagnose_a9_auth.py', 'run_a9_host.py', 'src/host_assisted.rs',
                 'src/bin/a9-host-assisted.rs', 'measure.py', 'Cargo.toml', 'Cargo.lock')}
            plan = list(variants(context))  # Validate the entire plan before launch.
            before = config_stat()
            result['config_stat_before'] = before
            if before.get('state') == 'unavailable':
                raise ValueError('config_metadata_unavailable')
            result['verification_stage'] = 'attempt_marker_guard'
            result['marker_present_before'] = attempt_state(context['HOME'])
            if result['marker_present_before']:
                raise ValueError('attempt_marker_exists')
            result['verification_stage'] = 'headless_preconditions'
            services = headless_metadata(context)
            result['headless_before'] = services
            if services['gnome-shell_running'] or services['gdm_running']:
                raise ValueError('unexpected_graphical_session')
            if not services['secrets_service_already_owned']:
                raise ValueError('credential_service_activation_not_authorized')
            for name, dimension, env in plan:
                result['verification_stage'] = name
                os.environ.clear()
                os.environ.update(env)
                report, code = measure(binary, 'preflight', cli, 65)
                after = config_stat()
                absent = identities_absent(report)
                result['variants'].append({'name': name, 'changed_dimension': dimension,
                    'observed_at': datetime.now(timezone.utc).isoformat(),
                    'config_stat_unchanged': before == after,
                    'marker_present_after': attempt_state(context['HOME']),
                    'owned_identities_absent_after': absent,
                    'harness_exit_code': code, 'measurement': report})
                if (before != after or not absent or not report.get('cleanup_complete')
                        or report.get('timed_out') or report.get('stdout_truncated')
                        or not report.get('kernel_children_exhausted')
                        or report.get('ownership_errors') or attempt_state(context['HOME'])):
                    raise ValueError('unsafe_or_incomplete_probe_stop')
            result['config_stat_after'] = config_stat()
            result['verification_stage'] = 'headless_final_verification'
            result['headless_after'] = headless_metadata(context)
            if result['headless_after'] != services:
                raise ValueError('service_metadata_changed')
            # Context variants are observations, never automatic wrapper changes.
            auth = [r['measurement'].get('sdk_report', {}).get('preflight', {}).get('auth', {}).get('authenticated') for r in result['variants']]
            result['authenticated_variants'] = [r['name'] for r, a in zip(result['variants'], auth) if a is True]
            result['decision'] = 'AUTH_RECOVERED_HEADLESS' if auth and auth[0] is True else 'AUTH_NOT_RECOVERED'
            result['cause'] = 'unknown_requires_controlled_positive_proof'
            result['verification_stage'] = 'complete'
        except (OSError, ValueError) as error:
            # Fixed vocabulary; exception text can contain a personal path.
            result['decision'] = 'AUTH_NOT_RECOVERED'
            code = error.args[0] if len(error.args) == 1 else None
            result['diagnostic_error'] = (code if isinstance(code, str) and code in ERROR_CODES
                                          else 'diagnostic_precondition_or_verification_failed')
        result['finished_at'] = datetime.now(timezone.utc).isoformat()
        output.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'decision': result['decision'], 'variants': len(result['variants']),
                      'sdk_send_calls': 0, 'a9_real': 'NOT_RUN'}))
    return 0 if result['decision'] == 'AUTH_RECOVERED_HEADLESS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
