"""Passive H3 headless preconditions. No unlock, SDK launch or service activation."""
import hashlib
import os
from pathlib import Path
import sys

from confirm_a9_metadata import proc_identity
from diagnose_a9_auth import CONTEXT, attempt_state
from diagnose_a9_gui_auth import gui_services, require_runtime, MANIFEST
from h1_context import owner, properties, sessions
from h2_manual_unlock import require_headless_service, DAEMON_SHA, UnlockBlocked


def validate(cli, *, unlocked):
    import json
    context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
    require_runtime(json.loads(MANIFEST.read_text()), cli)
    if attempt_state(context['HOME']):
        raise ValueError('attempt_marker_present')
    services = gui_services(context)
    user, rows = sessions(context)
    role = owner(context, services, rows)
    target = properties(['/usr/bin/systemctl', '--user', 'show',
        'graphical-session.target', '-p', 'ActiveState'], context,
        {'ActiveState': 'active|inactive'})['values'].get('ActiveState')
    ps_exit = properties(['/usr/bin/ps', '-C', 'gnome-shell', '-o', 'pid='], context, {})['exit_code']
    require_headless_service(services, rows, role, target, ps_exit)
    if services.get('runtime_bus_socket_accessible') is not True:
        raise ValueError('user_bus_unavailable')
    entry = Path('/proc') / str(role['pid'])
    if (entry.stat().st_uid != os.getuid() or
        (entry / 'exe').resolve() != Path('/usr/bin/gnome-keyring-daemon') or
        hashlib.sha256((entry / 'exe').read_bytes()).hexdigest() != DAEMON_SHA or
        proc_identity(entry)['start_ticks'] != role['start_ticks']):
        raise ValueError('credential_daemon_identity_unverified')
    # Only the alias path and Locked property; no items or contents.
    # Parse only the exact existing collection path, never item data.
    import subprocess
    from diagnose_a9_gui_auth import gui_environment
    p = subprocess.run(['/usr/bin/busctl', '--user', '--timeout=2', '--auto-start=no',
        '--allow-interactive-authorization=no', 'call', 'org.freedesktop.secrets',
        '/org/freedesktop/secrets', 'org.freedesktop.Secret.Service', 'ReadAlias', 's', 'login'],
        env=gui_environment(context), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, timeout=4)
    if p.returncode != 0 or p.stdout.strip() != b'o "/org/freedesktop/secrets/collection/login"':
        raise ValueError('existing_login_alias_unverified')
    after = gui_services(context)
    after_role = owner(context, after, rows)
    if after_role != role or after != services:
        raise ValueError('credential_context_changed')
    if unlocked and services.get('login_collection_locked') is not False:
        raise ValueError('manual_unlock_required')
    return {'profile': 'HOST_ASSISTED_HEADLESS_NOT_SANDBOX', 'services': services,
        'credential_owner': role, 'user': user, 'sessions': rows,
        'graphical_target': target, 'gnome_shell_ps_exit': ps_exit,
        'existing_login_alias_verified': True, 'daemon_pin_verified': True,
        'attempt_marker_present': False, 'credentials_read': False}


if __name__ == '__main__':
    import json
    if len(sys.argv) != 3 or sys.argv[1] != 'validate':
        raise SystemExit(2)
    try:
        print(json.dumps(validate(Path(sys.argv[2]), unlocked=True)))
    except (OSError, ValueError, KeyError, UnlockBlocked):
        print(json.dumps({'state': 'BLOCKED_HEADLESS_PRECONDITIONS'}))
        raise SystemExit(1)
