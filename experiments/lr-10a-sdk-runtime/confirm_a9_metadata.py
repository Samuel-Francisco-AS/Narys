#!/usr/bin/python3
"""A9-FIX-4 one-shot GUI-present metadata confirmation; no model/quota/session/send.

Fixed fresh evidence plus a separate binary reservation prevent diagnostic reruns.
Neither reservation reads/writes/claims the A9 inference marker. Concurrency is a
point observation, not exclusion of future same-UID activity or a sandbox.
"""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys

from diagnose_a9_auth import CONTEXT, PRESENCE, attempt_state
from diagnose_a9_gui_auth import (MANIFEST, gui_environment, gui_services,
                                 require_gui_context, require_runtime, require_cleanup)
from measure import measure
import metadata_policy as policy
from run_a9_host import ROOT
from run_fix2 import digest

OUTPUT = ROOT / 'evidence/a9-fix-4-metadata-confirmation.json'
VERIFICATION = ROOT / 'evidence/a9-fix-4-offline-verification.json'
SOURCE_FILES = ('config_integrity.py', 'metadata_policy.py', 'confirm_a9_metadata.py',
                'src/metadata_confirmation.rs', 'src/bin/a9-metadata-confirm.rs',
                'src/host_assisted.rs', 'measure.py', 'diagnose_a9_auth.py',
                'diagnose_a9_gui_auth.py', 'runtime-gui-candidate.json', 'Cargo.toml', 'Cargo.lock')


ESSENTIAL = frozenset(('systemd', '(sd-pam)', 'sshd', 'sshd-session', 'sshd-auth',
    'tmux', 'tmux: server', 'tmux: client', 'gnome-shell', 'gnome-keyring-d',
    'gdm', 'gdm-session-wor', 'dbus-broker', 'dbus-daemon'))
RUNTIMES = frozenset(('node', 'bun', 'npm', 'npx'))


def proc_identity(entry):
    """Nonsecret identity fields only. No arguments/environment/executable bytes."""
    raw = (entry / 'stat').read_text()
    tail = raw[raw.rindex(')') + 2:].split()
    return {'pid': int(raw[:raw.index('(')].strip()), 'ppid': int(tail[1]),
            'start_ticks': int(tail[19]), 'state': tail[0],
            'comm': raw[raw.index('(') + 1:raw.rindex(')')]}


def process_category(row, own):
    if row.get('identity_stable') is not True:
        return 'SUSPICIOUS_OR_UNASSESSABLE', True
    name = row['comm'].lower()
    if row.get('native_image_match') is True:
        return 'CONFIRMED_COPILOT', True
    if 'copilot' in name:
        return 'POTENTIALLY_CONCURRENT_RUNTIME', True
    if name in ESSENTIAL:
        return 'ESSENTIAL_PROCESS', False
    if row['pid'] in own:
        return 'OWN_CODEX_OR_HARNESS', False
    if name in RUNTIMES:
        return 'POTENTIALLY_CONCURRENT_RUNTIME', True
    if row['exe_state'] != 'observed':
        return 'PARTIALLY_INACCESSIBLE', False
    return 'OBSERVED_UNRELATED', False


def termination_admission(row):
    """Observation is not permission to kill. No terminator implemented here."""
    if row.get('category') in ('ESSENTIAL_PROCESS', 'OWN_CODEX_OR_HARNESS'):
        return 'DENIED_PROTECTED_PROCESS'
    return 'DENIED_NO_PROVEN_DISPOSABILITY'


def concurrency(cli, proc=Path('/proc')):
    """METADATA_READ_ONLY survey, not exclusivity, isolation or kill authority.

    Retain UID/comm/start-time when exe is denied. Stable identity with no known
    Copilot/runtime indicator is sufficient for this limited operation, even
    partially inaccessible; missing core identity, PID reuse and relevant runtime
    indicators block. No sensitive process data or signals are accessed.
    """
    native = cli.stat()
    rows, vanished = [], 0
    for entry in sorted(proc.iterdir(), key=lambda p: p.name):
        if not entry.name.isdigit():
            continue
        row = {'pid': int(entry.name), 'identity_stable': False}
        try:
            row['uid'] = entry.stat().st_uid
            if row['uid'] != os.getuid():
                continue
            row['comm'] = (entry / 'comm').read_text().strip()
            first = proc_identity(entry)
            row.update(ppid=first['ppid'], start_ticks=first['start_ticks'])
            try:
                image = (entry / 'exe').stat()
                row.update(exe_state='observed', native_image_match=(image.st_dev, image.st_ino)
                           == (native.st_dev, native.st_ino))
            except FileNotFoundError:
                row.update(exe_state='missing', native_image_match=False)
            except PermissionError:
                row.update(exe_state='access_denied', native_image_match=None)
            except OSError:
                row.update(exe_state='unavailable', native_image_match=None)
            second = proc_identity(entry)
            row['identity_stable'] = (all(first[k] == second[k] for k in
                ('pid', 'ppid', 'start_ticks', 'comm')) and first['pid'] == row['pid']
                and first['comm'] == row['comm'] and entry.stat().st_uid == row['uid'])
        except FileNotFoundError:
            vanished += 1
            continue
        except (OSError, ValueError, IndexError):
            row['inspection_error'] = 'core_identity_unavailable'
        rows.append(row)
    # Own ancestors and descendants of an observed Codex ancestor are protected.
    # Essential processes retain their category. Copilot indicators take priority.
    stable = {r['pid']: r for r in rows if r['identity_stable']}
    own, codex = set(), None
    pid = os.getpid()
    while pid in stable and pid not in own:
        own.add(pid)
        if stable[pid]['comm'] == 'codex':
            codex = pid
        pid = stable[pid]['ppid']
    if codex is not None:
        descendants = {codex}
        while True:
            expanded = descendants | {pid for pid, row in stable.items() if row['ppid'] in descendants}
            if expanded == descendants:
                break
            descendants = expanded
        own |= descendants
    counts = {'copilot_processes': 0, 'ambiguous_runtimes': 0, 'unavailable': 0,
              'suspicious_or_unassessable': 0}
    for row in rows:
        row['category'], row['blocking'] = process_category(row, own)
        row['termination_admission'] = termination_admission(row)
        counts['copilot_processes'] += int(row['category'] == 'CONFIRMED_COPILOT')
        counts['ambiguous_runtimes'] += int(row['category'] == 'POTENTIALLY_CONCURRENT_RUNTIME')
        counts['unavailable'] += int(row.get('exe_state') != 'observed')
        counts['suspicious_or_unassessable'] += int(row['category'] == 'SUSPICIOUS_OR_UNASSESSABLE')
    return dict(counts, state=('BLOCKED_RELEVANT_CONCURRENCY' if any(r['blocking'] for r in rows)
        else 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS'), processes=rows, vanished_during_scan=vanished,
        scope='METADATA_READ_ONLY', exclusivity_proven=False, processes_terminated=0)


def offline_verified(record, sources, binary):
    return (record.get('state') == 'PASS_OFFLINE' and record.get('real_cli_started') is False
            and record.get('source_sha256') == sources and record.get('binary_sha256') == binary)


def execution_plan(execution):
    # Closed identities, not an output-path, retry or force parameter.
    if execution == 'A9-FIX-4':
        return OUTPUT, VERIFICATION, 'a9-metadata-confirm', SOURCE_FILES
    if execution == 'A9-FIX-4R':
        return (ROOT / 'evidence/a9-fix-4r-metadata-confirmation.json',
                ROOT / 'evidence/a9-fix-4r-offline-verification.json',
                'a9-metadata-confirm-r', SOURCE_FILES +
                ('confirm_a9_metadata_recovery.py', 'src/bin/a9-metadata-confirm-r.rs'))
    raise ValueError('unknown_execution_identity')


def main(execution='A9-FIX-4'):
    # No output-path override: prior evidence blocks rerun, even after a failure.
    output_path, verification_path, binary_name, source_files = execution_plan(execution)
    fd = os.open(output_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    result = {'phase': execution, 'profile': 'HOST_ASSISTED_WITH_GUI_NOT_SANDBOX',
        'METADATA_AUTH_OBSERVATION': 'NOT_RUN', 'CONFIG_STRUCTURAL_CHECK': 'NOT_RUN',
        'CONFIG_DRIFT_OBSERVED': None, 'CONFIG_WRITER_ATTRIBUTION': 'INCONCLUSIVE',
        'SESSION_ADMISSION': 'BLOCKED', 'FINANCIAL_ADMISSION': 'BLOCKED',
        'AGENT_ACTION_ADMISSION': 'BLOCKED', 'HEADLESS_AUTH': 'NOT_PROVEN',
        'REAL_INFERENCE': 'NOT_RUN', 'PROCESS_CLEANUP': 'NOT_RUN',
        'ATTEMPT_MARKER': 'NOT_VERIFIED', 'sdk_invocations': 0, 'sdk_send_calls': 0,
        'session_operations': 0, 'models_quota_calls': 0, 'marker_claimed': False}
    context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
    result['environment_presence'] = {k: k in os.environ for k in PRESENCE}
    stage = 'offline_verification'
    try:
        sources = {name: digest(ROOT / name) for name in source_files}
        binary = ROOT / 'target/debug' / binary_name
        result['source_sha256'] = sources
        result['binary_sha256'] = digest(binary)
        record = json.loads(verification_path.read_text())
        if not offline_verified(record, sources, result['binary_sha256']):
            raise ValueError('offline_contract_unverified')
        home = Path(context['HOME'])
        cli = home / '.local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot'
        stage = 'native_identity'
        manifest = json.loads(MANIFEST.read_text())
        require_runtime(manifest, cli)
        result['runtime_manifest'] = manifest
        stage = 'marker'
        result['marker_present_before'] = attempt_state(home)
        if result['marker_present_before']:
            raise ValueError('attempt_marker_exists')
        result['ATTEMPT_MARKER'] = 'ABSENT_NOT_CLAIMED'
        stage = 'structural_check'
        before = policy.observe(home / '.copilot/config.json')
        result['before'] = before
        result['CONFIG_STRUCTURAL_CHECK'] = before['structural_check']
        if before['structural_check'] != 'PASS_METADATA_ACCESS':
            raise ValueError('structural_precondition_failed')
        stage = 'concurrency'
        result['concurrency_before_services'] = concurrency(cli)
        if result['concurrency_before_services']['state'] != 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS':
            raise ValueError('NOT_RUN_CONCURRENCY_UNVERIFIED')
        stage = 'credential_service'
        services = gui_services(context)
        result['services_before'] = services
        require_gui_context(services)
        stage = 'concurrency_final'
        result['concurrency_before_start'] = concurrency(cli)
        if result['concurrency_before_start']['state'] != 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS':
            raise ValueError('NOT_RUN_CONCURRENCY_UNVERIFIED')
        require_runtime(manifest, cli)
        if attempt_state(home):
            raise ValueError('attempt_marker_exists')
        # Filter by known non-secret context, never acquire token values.
        os.environ.clear()
        os.environ.update(gui_environment(context))
        stage = 'sdk_confirmation'
        result['sdk_invocations'] = 1
        measured, code = measure(binary, 'confirm', cli, 65)
        result['measurement'] = measured
        result['harness_exit_code'] = code
        report = measured.get('sdk_report', {})
        protocol_complete = (report.get('start_calls') == 1 and report.get('status_calls') == 1
            and report.get('auth_calls') == 1 and report.get('sdk_send_calls') == 0
            and report.get('session_operations') == 0 and report.get('model_quota_calls') == 0
            and report.get('attempt_marker_claimed') is False
            and [r.get('phase') for r in report.get('phases', [])] ==
                ['before_start', 'after_start', 'after_status', 'after_auth', 'after_shutdown'])
        result['protocol_complete'] = protocol_complete
        phases = [before['snapshot']] + [r['observation']['snapshot']
                  for r in report.get('phases', []) if 'snapshot' in r.get('observation', {})]
        result['marker_present_after'] = attempt_state(home)
        result['ATTEMPT_MARKER'] = ('ABSENT_NOT_CLAIMED' if not result['marker_present_after']
                                    else 'UNEXPECTED_ENTRY')
        result['services_after'] = gui_services(context)
        try:
            require_cleanup(measured)
            cleanup = report.get('shutdown') == 'graceful' and code == 0
        except (ValueError, OSError):
            cleanup = False
        result['PROCESS_CLEANUP'] = 'PASS' if cleanup else 'BLOCKED'
        auth = report.get('authenticated')
        result['METADATA_AUTH_OBSERVATION'] = ('PASS_REAL' if auth is True else
            'AUTH_FALSE_REAL' if auth is False else 'INCONCLUSIVE')
        result['policy'] = policy.evaluate(phases, auth,
            service_ok=result['services_after'] == services and protocol_complete,
            cleanup_ok=cleanup)
        for key in ('CONFIG_STRUCTURAL_CHECK', 'CONFIG_DRIFT_OBSERVED', 'CONFIG_WRITER_ATTRIBUTION'):
            result[key] = result['policy'][key]
        result['verification'] = ('METADATA_OBSERVATION_ONLY' if cleanup
            and result['policy']['metadata_verification'] == 'OBSERVATION_ACCEPTED'
            and result['ATTEMPT_MARKER'] == 'ABSENT_NOT_CLAIMED'
            and report.get('runtime_identity_matches') is True else 'BLOCKED')
        stage = 'complete'
    except (OSError, ValueError, KeyError):
        result['verification'] = 'BLOCKED'
        # Fixed stage only; never stringify a private exception/path.
        result['blocked_stage'] = stage
        if stage.startswith('concurrency'):
            result['METADATA_AUTH_OBSERVATION'] = 'NOT_RUN_CONCURRENCY_UNVERIFIED'
    finally:
        result['observed_at'] = datetime.now(timezone.utc).isoformat()
        with os.fdopen(fd, 'w') as output:
            json.dump(result, output, indent=2)
            output.write('\n')
            output.flush()
            os.fsync(output.fileno())
    print(json.dumps({k: result[k] for k in ('METADATA_AUTH_OBSERVATION',
        'CONFIG_STRUCTURAL_CHECK', 'CONFIG_DRIFT_OBSERVED', 'FINANCIAL_ADMISSION', 'REAL_INFERENCE')}))
    return 0 if stage == 'complete' else 1


if __name__ == '__main__':
    if sys.argv[1:]:
        raise SystemExit(2)
    raise SystemExit(main())
