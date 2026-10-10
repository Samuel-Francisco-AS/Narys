#!/usr/bin/python3
"""H1 passive context inspection. No SDK/CLI launch, unlock or host transition.

Run `h1_context.py inspect /absolute/pinned/copilot`. The unchanged ownership
harness bounds the passive commands. Only already-owned Secret Service boolean
properties and its PID are queried; no activation, credential items or contents.
"""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from confirm_a9_metadata import proc_identity
from diagnose_a9_auth import CONTEXT, PRESENCE, attempt_state
from diagnose_a9_gui_auth import (MANIFEST, gui_environment, gui_services,
                                 require_runtime, require_cleanup)
from measure import measure
from run_a9_host import ROOT
from run_fix2 import digest

OUTPUT = ROOT / 'evidence/h1-context.json'
HISTORY = ROOT / 'evidence/a9-fix-4r-metadata-confirmation.json'


def properties(argv, context, allowed):
    """Private parsing: retain only exact enumerations, never raw errors/output."""
    p = subprocess.run(argv, env=gui_environment(context), stdin=subprocess.DEVNULL,
                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    result = {'exit_code': p.returncode, 'values': {}}
    if p.returncode == 0:
        for line in p.stdout.decode('ascii', errors='replace').splitlines():
            key, sep, value = line.partition('=')
            if sep and key in allowed and re.fullmatch(allowed[key], value):
                result['values'][key] = value
    return result


def sessions(context):
    user = properties(['/usr/bin/loginctl', 'show-user', str(os.getuid()),
        '-p', 'Linger', '-p', 'State', '-p', 'Sessions'], context,
        {'Linger': 'yes|no', 'State': 'active|online|closing|lingering',
         'Sessions': r'[0-9 ]{0,128}'})
    rows = []
    for sid in user['values'].get('Sessions', '').split()[:16]:
        row = properties(['/usr/bin/loginctl', 'show-session', sid,
            '-p', 'Type', '-p', 'Class', '-p', 'State', '-p', 'Remote', '-p', 'Scope'],
            context, {'Type': 'wayland|x11|tty|unspecified',
                      'Class': 'user|manager|greeter|background|user-early',
                      'State': 'active|online|closing', 'Remote': 'yes|no',
                      'Scope': r'session-[0-9]+\.scope'})
        rows.append(dict(row, id=sid))
    return user, rows


def lifecycle(pid, session_rows, proc=Path('/proc')):
    """Validate PID/start-time and cgroup twice; publish scope booleans only."""
    entry = proc / str(pid)
    try:
        if entry.stat().st_uid != os.getuid():
            return {'state': 'UNKNOWN_OR_EXTERNAL'}
        first = proc_identity(entry)
        group = (entry / 'cgroup').read_text()
        second = proc_identity(entry)
        if (any(first[k] != second[k] for k in ('pid', 'ppid', 'start_ticks', 'comm'))
                or entry.stat().st_uid != os.getuid() or first['pid'] != pid
                or group != (entry / 'cgroup').read_text()):
            return {'state': 'IDENTITY_CHANGED'}
        def member(row):
            scope = row['values'].get('Scope')
            return bool(scope and re.search(r'/' + re.escape(scope) + r'(?:/|\n|$)', group))
        return {'state': 'OBSERVED', 'pid': pid, 'start_ticks': first['start_ticks'],
            'in_graphical_login_scope': any(member(s) and s['values'].get('Type')
                in ('wayland', 'x11') for s in session_rows),
            'in_remote_login_scope': any(member(s) and s['values'].get('Remote')
                == 'yes' for s in session_rows),
            'in_user_manager': f'/user@{os.getuid()}.service/' in group}
    except (OSError, ValueError, IndexError):
        return {'state': 'UNAVAILABLE'}


def owner(context, services, session_rows):
    if services.get('secrets_service_already_owned') is not True:
        return {'state': 'SERVICE_NOT_OWNED'}
    p = subprocess.run(['/usr/bin/busctl', '--user', '--timeout=2', '--auto-start=no',
        '--allow-interactive-authorization=no', 'call', 'org.freedesktop.DBus',
        '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
        's', 'org.freedesktop.secrets'], env=gui_environment(context),
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    match = re.fullmatch(rb'u ([1-9][0-9]*)\s*', p.stdout) if p.returncode == 0 else None
    if not match:
        return {'state': 'OWNER_UNAVAILABLE', 'exit_code': p.returncode}
    return lifecycle(int(match[1]), session_rows)


def assess(services, session_rows, credential_owner, target):
    """Preparation only: never emit an authentication PASS or execution grant."""
    graphical = any(s['values'].get('Type') in ('x11', 'wayland') for s in session_rows)
    gui = services.get('gnome-shell_running') is True or graphical or target == 'active'
    absence = (services.get('gnome-shell_running') is False and not graphical
               and target == 'inactive' and bool(session_rows))
    available = (services.get('runtime_bus_socket_accessible') is True
                 and services.get('secrets_service_already_owned') is True
                 and services.get('login_collection_locked') is False)
    reasons = []
    if credential_owner.get('state') != 'OBSERVED':
        reasons.append('credential_owner_unverified')
    elif credential_owner.get('in_graphical_login_scope') is True:
        reasons.append('credential_owner_in_graphical_login_scope')
    if not available:
        reasons.append('unlocked_credential_service_unverified')
    if not any(s['values'].get('Remote') == 'yes' and s['values'].get('State')
               in ('active', 'online') for s in session_rows):
        reasons.append('remote_session_unverified')
    return {'scenario': ('A_GUI_PRESENT' if gui else 'B_CONTEXT_CANDIDATE' if absence
                         else 'CONTEXT_INCONCLUSIVE'),
        'credential_service_available': available, 'transition_blockers': reasons,
        'transition': ('BLOCKED_UNSAFE_OR_UNVERIFIED' if reasons else
                       'REQUIRES_EXPLICIT_APPROVAL_AND_RECOVERY_PLAN'),
        'GUI_ABSENT_AUTH': 'NOT_TESTED', 'COLD_START_HEADLESS_AUTH': 'NOT_TESTED',
        'CREDENTIAL_STORAGE_SAFETY': 'INCONCLUSIVE',
        'SESSION_ADMISSION': 'BLOCKED', 'FINANCIAL_ADMISSION': 'BLOCKED',
        'AGENT_ACTION_ADMISSION': 'BLOCKED', 'REAL_INFERENCE': 'NOT_TESTED',
        'sdk_invocations': 0, 'sdk_send_calls': 0, 'session_operations': 0,
        'marker_claimed': False}


def observe(cli):
    context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
    manifest = json.loads(MANIFEST.read_text())
    require_runtime(manifest, cli)
    marker_before = attempt_state(context['HOME'])
    before = gui_services(context)
    user, rows = sessions(context)
    service_owner = owner(context, before, rows)
    target = properties(['/usr/bin/systemctl', '--user', '--no-pager', 'show',
        'graphical-session.target', '-p', 'ActiveState'], context,
        {'ActiveState': 'active|inactive|activating|deactivating|failed'})
    ssh = properties(['/usr/bin/systemctl', '--no-pager', 'show', 'sshd.service',
        '-p', 'ActiveState'], context, {'ActiveState': 'active|inactive|failed'})
    # Selected essential roles only; no executable, argv, environ or full cgroup.
    roles = []
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            role = (entry / 'comm').read_text().strip()
            if role in ('codex', 'tmux: server', 'gnome-keyring-d'):
                roles.append(dict(lifecycle(int(entry.name), rows), role=role))
        except OSError:
            continue
    after = gui_services(context)
    after_owner = owner(context, after, rows)
    marker_after = attempt_state(context['HOME'])
    result = assess(before, rows, service_owner, target['values'].get('ActiveState'))
    result.update(schema_version=1, phase='LR-10A H1',
        observed_at=datetime.now(timezone.utc).isoformat(),
        environment_presence={k: k in os.environ for k in PRESENCE},
        runtime_pin_validated=True, native_sha256=manifest['native_sha256'],
        native_version_source='historical_pinned_manifest_not_new_cli_execution',
        native_version=manifest['native_version'], sdk_version=manifest['sdk_version'],
        services_before=before, services_after=after, credential_owner=service_owner,
        credential_owner_after=after_owner, user_manager=user, sessions=rows,
        graphical_target=target, sshd=ssh, essential_roles=roles,
        marker_present_before=marker_before, marker_present_after=marker_after,
        services_and_owner_stable=before == after and service_owner == after_owner,
        cli_started=False, credentials_or_config_contents_read=False,
        host_services_modified=False, signals_to_external_processes=0)
    if marker_before or marker_after or before != after or service_owner != after_owner:
        result['transition_blockers'].append('marker_or_context_changed_or_unavailable')
        result['transition'] = 'BLOCKED_UNSAFE_OR_UNVERIFIED'
    require_runtime(manifest, cli)
    return result


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ('inspect', 'observe'):
        raise SystemExit('Use inspect /absolute/pinned/copilot')
    cli = Path(sys.argv[2])
    if sys.argv[1] == 'observe':
        if os.environ.get('NARYS_LR10A_OWNED_HARNESS') != '1':
            raise SystemExit('Ownership harness required')
        print(json.dumps(observe(cli)))
        return
    fd = os.open(OUTPUT, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as output:
        report, code = measure(Path(__file__).resolve(), 'observe', cli, 25)
        try:
            require_cleanup(report)
            state = 'PASS_PASSIVE_OBSERVATION' if code == 0 and report.get('sdk_report') else 'BLOCKED'
        except (OSError, ValueError):
            state = 'BLOCKED'
        result = {'phase': 'LR-10A H1', 'state': state, 'passive_ownership_measurement': report,
            'driver_exit_code': code, 'sdk_invocations': 0, 'historical_gui_auth_reference':
            {'path': str(HISTORY.relative_to(ROOT)), 'sha256': digest(HISTORY)},
            'source_sha256': {p: digest(ROOT / p) for p in ('h1_context.py',
                'diagnose_a9_gui_auth.py', 'diagnose_a9_auth.py', 'measure.py',
                'runtime-gui-candidate.json', 'src/host_assisted.rs', 'Cargo.toml', 'Cargo.lock')}}
        output.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'state': state, 'sdk_invocations': 0}))


if __name__ == '__main__':
    main()
