#!/usr/bin/python3
"""Fixed H3 one-shot metadata driver; only after coordinated human unlock.

No retry/output override. Separate evidence AND binary reservation; no A9 claim.
"""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys

from confirm_a9_metadata import concurrency, offline_verified
from diagnose_a9_auth import CONTEXT, attempt_state
from diagnose_a9_gui_auth import gui_environment, require_cleanup
from h3_headless import validate
from h2_manual_unlock import UnlockBlocked
from measure import measure
import metadata_policy as policy
from run_a9_host import ROOT
from run_fix2 import digest

SOURCES = ('h3_metadata.py', 'h3_headless.py', 'src/bin/h3-metadata-confirm.rs',
    'src/metadata_confirmation.rs', 'src/host_assisted.rs', 'metadata_policy.py',
    'config_integrity.py', 'h2_manual_unlock.py', 'h1_context.py', 'measure.py',
    'confirm_a9_metadata.py', 'diagnose_a9_gui_auth.py', 'diagnose_a9_auth.py',
    'runtime-gui-candidate.json', 'Cargo.toml', 'Cargo.lock')


def main():
    if sys.argv[1:]:
        return 2
    fd = os.open(ROOT / 'evidence/h3-metadata-real.json',
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    result = {'phase': 'LR-10A H3', 'profile': 'HOST_ASSISTED_HEADLESS_NOT_SANDBOX',
        'COPILOT_SDK_HEADLESS_AUTH': 'NOT_TESTED', 'PROCESS_CLEANUP': 'NOT_TESTED',
        'SESSION_ADMISSION': 'BLOCKED', 'FINANCIAL_ADMISSION': 'BLOCKED',
        'REAL_INFERENCE': 'NOT_RUN', 'sdk_invocations': 0, 'sdk_send_calls': 0,
        'session_operations': 0, 'models_quota_calls': 0, 'marker_claimed': False,
        'STRONGHOLD_PERSONAL_ACCESS': 'NOT_TESTED', 'COLD_START_HEADLESS': 'NOT_TESTED'}
    stage = 'offline_verification'
    context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
    home = Path(context['HOME'])
    cli = home / '.local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot'
    binary = ROOT / 'target/debug/h3-metadata-confirm'
    try:
        sources = {p: digest(ROOT / p) for p in SOURCES}
        result.update(source_sha256=sources, binary_sha256=digest(binary))
        offline = json.loads((ROOT / 'evidence/h3-offline-verification.json').read_text())
        if not offline_verified(offline, sources, result['binary_sha256']):
            raise ValueError('offline_contract_unverified')
        stage = 'headless_preconditions'
        result['before_context'] = validate(cli, unlocked=True)
        result['before_config'] = policy.observe(home / '.copilot/config.json')
        if result['before_config']['structural_check'] != 'PASS_METADATA_ACCESS':
            raise ValueError('config_structure_unverified')
        result['concurrency'] = concurrency(cli)
        if result['concurrency']['state'] != 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS':
            raise ValueError('relevant_concurrency_unverified')
        os.environ.clear()
        os.environ.update(gui_environment(context))
        stage = 'single_sdk_invocation'
        result['sdk_invocations'] = 1
        measured, code = measure(binary, 'confirm', cli, 65)
        result.update(measurement=measured, harness_exit_code=code)
        report = measured.get('sdk_report', {})
        stage = 'shutdown_verification'
        require_cleanup(measured)
        result['PROCESS_CLEANUP'] = 'PASS_REAL'
        result['after_context'] = validate(cli, unlocked=True)
        protocol = (code == 0 and report.get('shutdown') == 'graceful'
            and report.get('start_calls') == 1 and report.get('status_calls') == 1
            and report.get('auth_calls') == 1 and report.get('session_operations') == 0
            and report.get('sdk_send_calls') == 0 and report.get('model_quota_calls') == 0
            and report.get('attempt_marker_claimed') is False
            and report.get('runtime_identity_matches') is True
            and [r.get('phase') for r in report.get('phases', [])] ==
                ['before_start', 'after_start', 'after_status', 'after_auth', 'after_shutdown'])
        rows = [result['before_config']['snapshot']] + [r['observation']['snapshot']
            for r in report.get('phases', []) if 'snapshot' in r.get('observation', {})]
        service_stable = all(result['before_context'][key] == result['after_context'][key]
                             for key in ('services', 'credential_owner'))
        result['metadata_policy'] = policy.evaluate(rows, report.get('authenticated'),
            service_ok=service_stable and protocol,
            cleanup_ok=True)
        accepted = result['metadata_policy']['metadata_verification'] == 'OBSERVATION_ACCEPTED'
        result['COPILOT_SDK_HEADLESS_AUTH'] = ('PASS_REAL' if accepted and
            report.get('authenticated') is True else 'AUTH_FALSE_REAL' if accepted and
            report.get('authenticated') is False else 'INCONCLUSIVE')
        result['A9_ATTEMPT_MARKER'] = ('ABSENT_NOT_CLAIMED' if not attempt_state(home)
                                      else 'UNEXPECTED_ENTRY')
        stage = 'complete'
    except (OSError, ValueError, KeyError, UnlockBlocked):
        result['blocked_stage'] = stage
    finally:
        result['observed_at'] = datetime.now(timezone.utc).isoformat()
        with os.fdopen(fd, 'w') as f:
            json.dump(result, f, indent=2)
            f.write('\n')
            f.flush()
            os.fsync(f.fileno())
    print(json.dumps({k: result[k] for k in ('COPILOT_SDK_HEADLESS_AUTH',
        'PROCESS_CLEANUP', 'FINANCIAL_ADMISSION', 'REAL_INFERENCE')}))
    return 0 if stage == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())
