"""H3 passive qualification of a fixed system-manager GUI rollback timer.

No start/stop, privilege escalation, secret access or session termination here.
The human arms it in a separate SSH terminal. Starting GDM restores the greeter,
not the authenticated graphical session. A timer is never proof of user login.
"""
import re
import subprocess

TIMER = 'narys-h3-gui-rollback.timer'
SERVICE = 'narys-h3-gui-rollback.service'


def show(unit):
    p = subprocess.run(['/usr/bin/systemctl', '--no-pager', 'show', unit,
        '-p', 'LoadState', '-p', 'ActiveState', '-p', 'SubState', '-p', 'Transient',
        '-p', 'User', '-p', 'DynamicUser', '-p', 'Type', '-p', 'Unit',
        '-p', 'ExecStart', '-p', 'TimersMonotonic', '-p', 'NextElapseUSecMonotonic'],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, timeout=5)
    if p.returncode != 0:
        return {}
    # Never persist unparsed command output.
    return dict(line.split('=', 1) for line in
                p.stdout.decode('ascii', errors='replace').splitlines() if '=' in line)


def qualify(timer, service):
    action = service.get('ExecStart', '')
    exact = bool(re.fullmatch(
        r'\{ path=/usr/bin/systemctl ; argv\[\]=/usr/bin/systemctl start gdm\.service ; [^{}]*\}',
        action))
    root = (service.get('LoadState') == 'loaded' and service.get('Transient') == 'yes'
            and service.get('User') in ('', 'root', '0') and service.get('DynamicUser') == 'no')
    registered = (timer.get('LoadState') == 'loaded' and
        timer.get('ActiveState') == 'active' and timer.get('SubState') == 'waiting' and
        timer.get('Transient') == 'yes' and timer.get('Unit') == SERVICE)
    bounded = ('OnActiveUSec=30min' in timer.get('TimersMonotonic', '') and
        timer.get('NextElapseUSecMonotonic') not in (None, '', '0', 'infinity'))
    valid = (service.get('LoadState') == 'loaded' and service.get('Transient') == 'yes'
             and service.get('Type') == 'oneshot' and root and exact and registered and bounded)
    return {'state': 'REGISTERED_ROOT_GDM_RETURN' if valid else 'BLOCKED_RECOVERY_UNVERIFIED',
        'timer_registered_waiting': registered, 'fixed_30_minute_timer': bounded,
        'root_authority_registered': root, 'exact_gdm_start_action': exact,
        'authenticated_gui_login_restored': False,
        'guaranteed_after_host_or_system_manager_failure': False}


def observe():
    return qualify(show(TIMER), show(SERVICE))
