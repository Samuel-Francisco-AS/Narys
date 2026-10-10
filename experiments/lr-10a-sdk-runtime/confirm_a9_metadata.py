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


def concurrency(cli, proc=Path('/proc')):
    """Same-UID comm and executable metadata only, no cmdline/environ or signals.

    Native identity detects a renamed pinned image. Known other Copilot names and
    Node/Bun ambiguity block. Unreadable same-UID entries are not presumed safe.
    A disguised unrelated runtime or a later launch is not excluded by this scan.
    """
    counts = {'copilot_processes': 0, 'ambiguous_runtimes': 0, 'unavailable': 0}
    native = cli.stat()
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if entry.stat().st_uid != os.getuid():
                continue
            name = (entry / 'comm').read_text().strip().lower()
            try:
                image = (entry / 'exe').stat()
                pinned = (image.st_dev, image.st_ino) == (native.st_dev, native.st_ino)
            except FileNotFoundError:
                # Disappeared process or zombie with no executable; never signal.
                pinned = False
            counts['copilot_processes'] += int('copilot' in name or pinned)
            counts['ambiguous_runtimes'] += int(name in ('node', 'bun', 'npm', 'npx'))
        except FileNotFoundError:
            continue
        except OSError:
            counts['unavailable'] += 1
    counts['state'] = ('OBSERVED_NO_CONCURRENT_COPILOT' if not any(counts.values())
                       else 'NOT_RUN_CONCURRENCY_UNVERIFIED')
    return counts


def offline_verified(record, sources, binary):
    return (record.get('state') == 'PASS_OFFLINE' and record.get('real_cli_started') is False
            and record.get('source_sha256') == sources and record.get('binary_sha256') == binary)


def main():
    # No output-path override: prior evidence blocks rerun, even after a failure.
    fd = os.open(OUTPUT, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    result = {'phase': 'A9-FIX-4', 'profile': 'HOST_ASSISTED_WITH_GUI_NOT_SANDBOX',
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
        sources = {name: digest(ROOT / name) for name in SOURCE_FILES}
        binary = ROOT / 'target/debug/a9-metadata-confirm'
        result['source_sha256'] = sources
        result['binary_sha256'] = digest(binary)
        record = json.loads(VERIFICATION.read_text())
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
        if result['concurrency_before_services']['state'] != 'OBSERVED_NO_CONCURRENT_COPILOT':
            raise ValueError('NOT_RUN_CONCURRENCY_UNVERIFIED')
        stage = 'credential_service'
        services = gui_services(context)
        result['services_before'] = services
        require_gui_context(services)
        stage = 'concurrency_final'
        result['concurrency_before_start'] = concurrency(cli)
        if result['concurrency_before_start']['state'] != 'OBSERVED_NO_CONCURRENT_COPILOT':
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
    raise SystemExit(main())
