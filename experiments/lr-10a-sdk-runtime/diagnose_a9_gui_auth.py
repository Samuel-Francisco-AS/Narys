#!/usr/bin/env python3
"""A9-FIX-2: independent GUI-present host metadata, never a send/claim path.

The headless profile, its runtime pin and all prior evidence remain untouched.
Only normal CLI credential resolution is allowed. Locked is a boolean property;
no secret service contents, items, labels or credential values are queried.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import tempfile

from diagnose_a9_auth import (CONTEXT, PRESENCE, attempt_state,
                             headless_metadata, identities_absent)
from measure import measure
from run_a9_host import CLI_SHA, ROOT
from run_fix2 import config_stat, digest

BASE = 'f0ae5787c7cde7b54a1970727e2a52b43f4612f8'
MANIFEST = ROOT / 'runtime-gui-candidate.json'

# Fixed status/help/version only. Raw stdout is consumed privately then discarded.
# Child HOME is disposable and keytar disabled for this identity-only stage.
# The unchanged ownership worker enforces the deadline with pidfds, not Popen.kill.
IDENTITY_PROGRAM = '''#!/usr/bin/python3
import json,os,re,subprocess,sys,tempfile
with tempfile.TemporaryDirectory(prefix="narys-cli-identity-") as home:
 env={"HOME":home,"COPILOT_HOME":home,"PATH":"/usr/bin","LANG":"C",
      "COPILOT_DISABLE_KEYTAR":"1","COPILOT_SKIP_CLI_DOWNLOAD":"1"}
 p=subprocess.run([sys.argv[2],"--no-auto-update","--version"],env=env,
     stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
 versions=re.findall(rb"\\b([0-9]+\\.[0-9]+\\.[0-9]+)\\b",p.stdout)
 version=versions[0].decode("ascii") if p.returncode==0 and len(versions)==1 else None
 h=subprocess.run([sys.argv[2],"--no-auto-update","--help"],env=env,
     stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
 flags={f:f.encode() in h.stdout for f in ("--disable-builtin-mcps","--log-dir","--no-auto-update")}
 print(json.dumps({"native_version":version,"version_exit_code":p.returncode,
     "help_exit_code":h.returncode,"help_flags_present":flags,
     "raw_output_persisted":False,"normal_auth_used":False,"inference_calls":0}))
'''


def gui_environment(context):
    """Match the A9 allowlist. GUI exists elsewhere; do not supply display/token."""
    if 'HOME' not in context:
        raise ValueError('home_unavailable')
    env = {k: context[k] for k in ('HOME', 'DBUS_SESSION_BUS_ADDRESS',
                                  'XDG_RUNTIME_DIR') if k in context}
    env.update(PATH='/usr/bin', LANG='C', COPILOT_SKIP_CLI_DOWNLOAD='1')
    return env


def gui_services(context):
    result = headless_metadata(context)  # observation helper, NOT headless admission
    result['login_collection_locked'] = None
    if result['secrets_service_already_owned']:
        p = subprocess.run(['/usr/bin/busctl', '--user', '--timeout=2',
            'get-property', 'org.freedesktop.secrets',
            '/org/freedesktop/secrets/collection/login',
            'org.freedesktop.Secret.Collection', 'Locked'],
            env=gui_environment(context), stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        result['locked_property_exit_code'] = p.returncode
        if p.returncode == 0 and p.stdout.strip() in (b'b true', b'b false'):
            result['login_collection_locked'] = p.stdout.strip() == b'b true'
    return result


def require_gui_context(services):
    if services.get('gnome-shell_running') is not True:
        raise ValueError('gui_profile_not_observed')
    if (services.get('secrets_service_already_owned') is not True
            or services.get('runtime_bus_socket_accessible') is not True):
        raise ValueError('existing_credential_service_unavailable')
    if services.get('login_collection_locked') is not False:
        raise ValueError('login_collection_locked_or_unknown')


def require_runtime(manifest, cli):
    if (manifest.get('schema_version') != 1 or manifest.get('sdk_version') != '1.0.17'
            or manifest.get('profile') != 'HOST_ASSISTED_WITH_GUI_NOT_SANDBOX'):
        raise ValueError('invalid_runtime_manifest')
    if not cli.is_absolute() or not cli.is_file():
        raise ValueError('native_runtime_unavailable')
    with cli.open('rb') as stream:
        if stream.read(4) != b'\x7fELF':
            raise ValueError('native_elf_required')
    if digest(cli) != manifest.get('native_sha256'):
        raise ValueError('candidate_pin_mismatch')


def require_cleanup(measurement):
    if (not measurement.get('cleanup_complete') or not measurement.get('kernel_children_exhausted')
            or measurement.get('ownership_errors') or measurement.get('timed_out')
            or measurement.get('stdout_truncated') or not identities_absent(measurement)):
        raise ValueError('incomplete_or_unsafe_probe')


def require_version(measurement, manifest):
    require_cleanup(measurement)
    report = measurement.get('sdk_report', {})
    if (report.get('native_version') != manifest.get('native_version')
            or report.get('version_exit_code') != 0 or report.get('help_exit_code') != 0
            or report.get('help_flags_present', {}).get('--disable-builtin-mcps') is not True
            or report.get('help_flags_present', {}).get('--log-dir') is not True):
        raise ValueError('version_or_required_cli_flags_unverified')


ERROR_CODES = frozenset(('home_unavailable', 'gui_profile_not_observed',
    'existing_credential_service_unavailable', 'login_collection_locked_or_unknown',
    'invalid_runtime_manifest', 'native_runtime_unavailable', 'native_elf_required',
    'candidate_pin_mismatch', 'incomplete_or_unsafe_probe',
    'version_or_required_cli_flags_unverified', 'attempt_marker_exists',
    'config_metadata_unavailable', 'configuration_or_context_changed'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cli', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as output:
        result = {'phase': 'LR-10A A9-FIX-2', 'base': BASE, 'schema_version': 1,
            'profile': 'HOST_ASSISTED_WITH_GUI_NOT_SANDBOX',
            'cli_interactive_authenticated': 'USER_REPORTED_not_reproduced',
            'sdk_authenticated_with_gui': 'NOT_RUN',
            'sdk_authenticated_headless': 'NOT_PROVEN', 'financial_admission': 'BLOCKED',
            'a9_real_inference': 'NOT_RUN', 'inference_calls': 0,
            'real_session_operations': 0, 'marker_claimed': False,
            'gui_or_service_started_stopped_by_poc': False}
        context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
        result['environment_presence'] = {k: k in os.environ for k in PRESENCE}
        try:
            result['stage'] = 'runtime_identity'
            cli = args.cli.resolve(strict=True)
            manifest = json.loads(MANIFEST.read_text())
            require_runtime(manifest, cli)
            result['runtime_manifest'] = manifest
            result['legacy_pin_matches_selected_image'] = digest(cli) == CLI_SHA
            result['source_sha256'] = {p: digest(ROOT / p) for p in (
                'diagnose_a9_gui_auth.py', 'runtime-gui-candidate.json', 'diagnose_a9_auth.py',
                'src/host_assisted.rs', 'src/bin/a9-host-assisted.rs', 'measure.py',
                'Cargo.toml', 'Cargo.lock')}
            result['binary_sha256'] = digest(ROOT / 'target/debug/a9-host-assisted')
            before = config_stat()
            result['config_stat_before'] = before
            if before.get('state') == 'unavailable':
                raise ValueError('config_metadata_unavailable')
            result['marker_present_before'] = attempt_state(context['HOME'])
            if result['marker_present_before']:
                raise ValueError('attempt_marker_exists')
            os.environ.clear()
            os.environ.update(gui_environment(context))
            result['stage'] = 'gui_context'
            services = gui_services(context)
            result['services_before'] = services
            require_gui_context(services)
            result['stage'] = 'native_version_and_help'
            with tempfile.TemporaryDirectory(prefix='narys-a9-gui-identity-') as tmp:
                wrapper = Path(tmp) / 'identity.py'
                wrapper.write_text(IDENTITY_PROGRAM)
                wrapper.chmod(0o700)
                identity_report, code = measure(wrapper, 'identity', cli, 15)
            result['runtime_inspection'] = identity_report
            result['runtime_inspection_harness_exit'] = code
            require_version(identity_report, manifest)
            if before != config_stat() or attempt_state(context['HOME']):
                raise ValueError('configuration_or_context_changed')
            result['stage'] = 'sdk_metadata'
            measurement, code = measure(ROOT / 'target/debug/a9-host-assisted', 'preflight', cli, 65)
            result['sdk_metadata'] = measurement
            result['sdk_metadata_harness_exit'] = code  # financial blocks intentionally nonzero
            require_cleanup(measurement)
            result['config_stat_after'] = config_stat()
            result['marker_present_after'] = attempt_state(context['HOME'])
            result['services_after'] = gui_services(context)
            require_runtime(manifest, cli)
            if (before != result['config_stat_after'] or result['marker_present_after']
                    or result['services_after'] != services):
                raise ValueError('configuration_or_context_changed')
            result['sdk_authenticated_with_gui'] = ('PASS' if measurement.get('sdk_report', {})
                .get('preflight', {}).get('auth', {}).get('authenticated') is True else 'BLOCKED')
            result['stage'] = 'complete'
        except (OSError, ValueError, KeyError) as error:
            code = error.args[0] if len(error.args) == 1 else None
            result['diagnostic_error'] = (code if isinstance(code, str) and code in ERROR_CODES
                                          else 'precondition_or_verification_failed')
            result['sdk_authenticated_with_gui'] = 'BLOCKED'
        result['observed_at'] = datetime.now(timezone.utc).isoformat()
        output.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: result[k] for k in ('sdk_authenticated_with_gui',
                     'financial_admission', 'a9_real_inference', 'inference_calls')}))
    return 0 if result['stage'] == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())
