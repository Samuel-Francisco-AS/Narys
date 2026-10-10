#!/usr/bin/python3
"""H2 synthetic-only launcher. No personal unlock, service modification or SDK."""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys
import tempfile

from boundary import directory
from diagnose_a9_auth import CONTEXT, attempt_state, identities_absent
from diagnose_a9_gui_auth import gui_services, require_cleanup
from measure import measure
from run_a9_host import ROOT
from run_fix2 import digest

OUTPUT = ROOT / 'evidence/h2-keyring-synthetic.json'
DBUS_CONFIG = '''<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:path=/run/h2/bus</listen>
<auth>EXTERNAL</auth><policy context="default"><allow user="*"/><allow send_destination="*"/>
<allow receive_sender="*"/><allow own="*"/></policy></busconfig>
'''


def plan(job, binary):
    job = directory(job)
    if job.parent != Path('/tmp') or job.stat().st_mode & 0o077:
        raise ValueError('private_tmp_job_required')
    if binary != ROOT / 'target/debug/h2-stronghold-fixture' or not binary.is_file():
        raise ValueError('synthetic_backend_binary_required')
    # Trusted installed /usr software only, no host /home,/run,/etc store or bus.
    # Wider /usr mount is for this synthetic system-component fixture, not Copilot.
    return ['/usr/bin/bwrap', '--unshare-all', '--die-with-parent', '--new-session',
        '--cap-drop', 'ALL', '--clearenv', '--tmpfs', '/', '--ro-bind', '/usr', '/usr',
        '--symlink', 'usr/lib64', '/lib64', '--symlink', 'usr/lib', '/lib',
        '--symlink', 'usr/bin', '/bin', '--dir', '/etc', '--dir', '/run/h2',
        '--dir', '/home/fixture', '--tmpfs', '/tmp', '--dev', '/dev', '--proc', '/proc',
        '--ro-bind', str(ROOT / 'fixtures/h2_keyring_fixture.py'), '/h2/test.py',
        '--ro-bind', str(ROOT / 'measure.py'), '/h2/measure.py',
        '--ro-bind', str(ROOT / 'h2_manual_unlock.py'), '/h2/h2_manual_unlock.py',
        '--ro-bind', str(binary), '/h2/stronghold-fixture',
        '--ro-bind', str(job / 'dbus.conf'), '/h2/dbus.conf',
        '--ro-bind', str(job / 'synthetic-only'), '/h2/synthetic-only',
        '--ro-bind', str(job / 'passwd'), '/etc/passwd',
        '--ro-bind', str(job / 'group'), '/etc/group',
        '--ro-bind', str(job / 'nsswitch.conf'), '/etc/nsswitch.conf',
        '--ro-bind', str(job / 'machine-id'), '/etc/machine-id',
        '--bind', str(job / 'state'), '/state', '--setenv', 'HOME', '/home/fixture',
        '--setenv', 'XDG_DATA_HOME', '/state/data', '--setenv', 'XDG_CONFIG_HOME', '/state/config',
        '--setenv', 'XDG_CACHE_HOME', '/state/cache', '--setenv', 'XDG_RUNTIME_DIR', '/run/h2',
        '--setenv', 'PATH', '/usr/bin', '--setenv', 'LANG', 'C', '--chdir', '/h2',
        '--', '/usr/bin/python3', '/h2/test.py']


def main():
    if len(sys.argv) != 1:
        raise SystemExit('Synthetic-only; no arguments or personal mode')
    fd = os.open(OUTPUT, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
    binary = ROOT / 'target/debug/h2-stronghold-fixture'
    before = gui_services(context)
    marker_before = attempt_state(context['HOME'])
    with os.fdopen(fd, 'w') as output, tempfile.TemporaryDirectory(prefix='narys-h2-') as tmp:
        job = Path(tmp)
        (job / 'state').mkdir(mode=0o700)
        (job / 'dbus.conf').write_text(DBUS_CONFIG)
        (job / 'synthetic-only').write_bytes(b'H2_SYNTHETIC_ONLY')
        (job / 'passwd').write_text(f'fixture:x:{os.getuid()}:{os.getgid()}:Synthetic:/home/fixture:/bin/false\n')
        (job / 'group').write_text(f'fixture:x:{os.getgid()}:\n')
        (job / 'nsswitch.conf').write_text('passwd: files\ngroup: files\n')
        (job / 'machine-id').write_text('00000000000000000000000000000001\n')
        argv = plan(job, binary)
        launcher = job / 'launch.py'
        launcher.write_text('#!/usr/bin/python3\nimport os\nos.execve("/usr/bin/bwrap",'
                            + repr(argv) + ',{"PATH":"/usr/bin","LANG":"C"})\n')
        launcher.chmod(0o700)
        report, code = measure(launcher, 'synthetic', Path('/unused'), 45)
        after = gui_services(context)
        marker_after = attempt_state(context['HOME'])
        state = 'BLOCKED'
        try:
            require_cleanup(report)
            if (code == 0 and report.get('sdk_report', {}).get('state') == 'PASS_SYNTHETIC_REAL_COMPONENTS'
                    and before == after and marker_before == marker_after is False):
                state = 'PASS_SYNTHETIC_ONLY'
        except (OSError, ValueError):
            pass
        record = {'phase': 'LR-10A H2', 'observed_at': datetime.now(timezone.utc).isoformat(),
            'state': state, 'measurement': report, 'harness_exit': code,
            'host_services_before': before, 'host_services_after': after,
            'marker_present_before': marker_before, 'marker_present_after': marker_after,
            'personal_keyring_or_stronghold_accessed': False, 'copilot_sdk_invocations': 0,
            'production_files_modified': False, 'binary_sha256': digest(binary),
            'source_sha256': {p: digest(ROOT / p) for p in ('h2_keyring.py',
                'h2_manual_unlock.py', 'fixtures/h2_keyring_fixture.py',
                'fixtures/h2_stronghold.rs', 'measure.py',
                '../../src-tauri/src/security/secrets.rs', '../../src-tauri/src/security/audit.rs')},
            'owned_identities_absent': identities_absent(report)}
        output.write(json.dumps(record, indent=2) + '\n')
    print(json.dumps({'state': state, 'copilot_sdk_invocations': 0}))
    return 0 if state == 'PASS_SYNTHETIC_ONLY' else 1


if __name__ == '__main__':
    raise SystemExit(main())
