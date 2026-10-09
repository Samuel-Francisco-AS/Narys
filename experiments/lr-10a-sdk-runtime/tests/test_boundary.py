"""Real bwrap/kernel tests using owned canaries; no Copilot/inference/credentials."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest
from unittest import mock

EXPERIMENT = Path(__file__).resolve().parents[1]
for name in ('measure', 'boundary'):
    spec = importlib.util.spec_from_file_location(name, EXPERIMENT / (name + '.py'))
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    globals()[name] = loaded


class BoundaryTests(unittest.TestCase):
    observations = []

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.job = Path(self.tmp.name)
        self.workspace = self.job / 'workspace'
        self.state = self.job / 'state'
        self.store = self.state / 'session-state'
        self.logs = self.job / 'logs'
        for path in (self.workspace, self.store, self.logs):
            path.mkdir(parents=True)
        self.outside = self.job / 'outside-canary'
        self.outside.write_text('synthetic-secret-canary-never-export')
        self.fixture = self.workspace / 'fixture.txt'
        self.fixture.write_text('alpha=2 beta=3\n')
        (self.workspace / 'escape-link').symlink_to(self.outside)
        self.tcp = socket.socket()
        self.tcp.bind(('127.0.0.1', 0))
        self.tcp.listen(4)
        self.addCleanup(self.tcp.close)
        self.unix = socket.socket(socket.AF_UNIX)
        self.unix.bind(str(self.job / 'canary.socket'))
        self.unix.listen(4)
        self.addCleanup(self.unix.close)
        cfg = {'outside': str(self.outside), 'socket': str(self.job / 'canary.socket'),
               'tcp': '127.0.0.1:' + str(self.tcp.getsockname()[1])}
        (self.workspace / 'probe.json').write_text(json.dumps(cfg))
        self.program = EXPERIMENT / 'target/debug/boundary-fixture'

    def built(self, **kwargs):
        return boundary.plan(self.program, self.workspace, self.state, self.store, self.logs, **kwargs)

    def invocation(self, mode='normal', deadline=5):
        built = self.built()
        wrapper = self.job / 'owned-launcher.py'
        # Trusted host launcher never reads secrets or passes the inherited env.
        wrapper.write_text('#!/usr/bin/python3\nimport sys\n'
            + f'sys.path.insert(0, {str(EXPERIMENT)!r})\nimport boundary\n'
            + f'boundary.execute({built!r}, [{mode!r}])\n')
        wrapper.chmod(0o700)
        result, code = measure.measure(wrapper, 'metadata', Path('/unused'), deadline)
        self.assertTrue(result['cleanup_complete'], result.get('cleanup_status'))
        for row in result['attributed_processes']:
            try:
                now = measure.identity(row['pid'])
            except FileNotFoundError:
                continue
            self.assertNotEqual(now['start_ticks'], row['start_ticks'])
        self.assertEqual(self.outside.read_text(), 'synthetic-secret-canary-never-export')
        self.assertEqual(self.fixture.read_text(), 'alpha=2 beta=3\n')
        self.observations.append({'test': self.id(), 'result': result,
                                  'harness_exit_code': code, 'canaries_unchanged': True,
                                  'attributed_identities_absent_after': True})
        return result, code

    def checks(self):
        result, code = self.invocation()
        self.assertEqual(code, 0)
        self.assertTrue(result['sdk_report']['passed'])
        self.assertNotIn('synthetic-secret', json.dumps(result))
        return result['sdk_report']['checks']

    def test_G1_authorized_fixture_read_and_private_writes(self):
        checks = self.checks()
        self.assertTrue(checks['fixture_read'] and checks['fixture_write_denied'])
        self.assertTrue((self.state / 'probe-state').exists())
        self.assertTrue((self.logs / 'probe-log').exists())

    def test_G2_external_canary_and_personal_roots_hidden(self):
        checks = self.checks()
        self.assertTrue(checks['outside_read_denied'] and checks['outside_write_denied'])
        self.assertTrue(checks['personal_roots_absent'] and checks['root_write_denied'])

    def test_G3_symlink_and_traversal_cannot_expose_host(self):
        checks = self.checks()
        self.assertTrue(checks['symlink_read_denied'] and checks['traversal_read_denied'])

    def test_G4_environment_allowlist_discards_sensitive_names(self):
        with mock.patch.dict(os.environ, {'GH_TOKEN': 'synthetic-secret',
                'COPILOT_GITHUB_TOKEN': 'synthetic-secret',
                'COPILOT_PROVIDER_API_KEY': 'synthetic-secret',
                'COPILOT_PROVIDER_BASE_URL': 'synthetic-secret',
                'AWS_SECRET_ACCESS_KEY': 'synthetic-secret', 'SYNTHETIC_SECRET': 'synthetic-secret',
                'DBUS_SESSION_BUS_ADDRESS': 'synthetic-secret'}):
            checks = self.checks()
        self.assertTrue(checks['environment_absent'])

    def test_G5_external_shell_absent_kernel_keyring_denied(self):
        checks = self.checks()
        self.assertTrue(checks['shell_binary_absent'] and checks['kernel_keyring_denied'])
        self.assertTrue(checks['no_new_privileges'])

    def test_G6_plan_has_no_host_mcp_plugin_or_auth_mount(self):
        built = self.built()
        sources = [built['args'][i + 1] for i, arg in enumerate(built['args'])
                   if arg in ('--bind', '--ro-bind', '--dev-bind')]
        self.assertNotIn('/', sources)
        self.assertFalse(any(p in sources for p in ('/home', '/home/sam', '/root', '/run')))
        self.assertEqual(built['auth'], 'no_host_credentials_or_bus')
        self.assertEqual(set(built['environment_names']), set(boundary.SAFE_ENV))

    def test_G7_no_credential_proxy_or_bus_bypass(self):
        checks = self.checks()
        self.assertTrue(checks['host_unix_socket_denied'])
        with self.assertRaisesRegex(boundary.BoundaryError, 'unsupported_security_policy'):
            self.built(policy='host-keyring')

    def test_G8_timeout_active_tree_external_process_preserved(self):
        external = subprocess.Popen(['/usr/bin/python3', '-I', '-c', 'import time; time.sleep(30)'])
        try:
            original = measure.identity(external.pid)
            def ready(proc):
                deadline = time.monotonic() + 3
                while not (self.state / 'tree-ready').exists():
                    if proc.poll() is not None or time.monotonic() >= deadline:
                        raise AssertionError('synthetic tree readiness failed')
                    time.sleep(0.005)
            with mock.patch.object(measure, 'after_launch', side_effect=ready):
                result, code = self.invocation('timeout', 0.2)
            self.assertEqual(code, 1)
            self.assertEqual(result['cleanup_status'], 'timeout_recovered')
            self.assertTrue((self.state / 'child-ready').exists())
            self.assertIsNone(external.poll())
            self.assertEqual(measure.identity(external.pid)['start_ticks'], original['start_ticks'])
            self.observations[-1]['external_identity_unchanged_and_alive'] = True
        finally:
            external.terminate()  # Only the external control's direct test owner.
            external.wait(timeout=3)

    def test_G9_evidence_contains_only_safe_checks(self):
        result, code = self.invocation()
        self.assertEqual(code, 0)
        self.assertNotIn('synthetic-secret', json.dumps(result))
        self.assertFalse(result['sdk_report']['sensitive_values_exported'])

    def test_G10_unknown_policy_non_elf_and_missing_dependency_fail_closed(self):
        with self.assertRaisesRegex(boundary.BoundaryError, 'unsupported_security_policy'):
            self.built(policy='share-net')
        with self.assertRaises(boundary.BoundaryError):
            boundary.plan(self.outside, self.workspace, self.state, self.store, self.logs)
        with mock.patch.object(boundary, 'native_dependencies') as inventory:
            with self.assertRaisesRegex(boundary.BoundaryError, 'unapproved_native_program'):
                boundary.plan('/usr/bin/true', self.workspace, self.state, self.store, self.logs)
            forged = self.job / 'copilot'
            forged.write_bytes(b'\x7fELF-not-the-pinned-runtime')
            with self.assertRaisesRegex(boundary.BoundaryError, 'cli_pin_mismatch'):
                boundary.plan(forged, self.workspace, self.state, self.store, self.logs)
            inventory.assert_not_called()
        with mock.patch.object(boundary, 'native_dependencies', side_effect=boundary.BoundaryError('native_elf_required')):
            with self.assertRaisesRegex(boundary.BoundaryError, 'native_elf_required'):
                self.built()
        with mock.patch.object(boundary, 'native_dependencies', side_effect=boundary.BoundaryError('dependency_unavailable')):
            with self.assertRaisesRegex(boundary.BoundaryError, 'dependency_unavailable'):
                self.built()
        with mock.patch.object(boundary, 'BWRAP', Path('/nonexistent/bwrap')):
            with self.assertRaisesRegex(boundary.BoundaryError, 'bubblewrap_unavailable'):
                self.built()
        for args in (['-p', 'not-a-prompt-to-send'], ['--yolo'], ['--autopilot'], ['--plugin-dir', '/home']):
            with self.assertRaises(boundary.BoundaryError):
                boundary.sdk_arguments(args)

    def test_G10_mount_failure_does_not_execute_target(self):
        built = self.built()
        index = built['args'].index(str(self.workspace))
        built['args'][index] = str(self.job / 'missing-mount')
        wrapper = self.job / 'failed-mount.py'
        wrapper.write_text('#!/usr/bin/python3\nimport sys\n'
            + f'sys.path.insert(0,{str(EXPERIMENT)!r})\nimport boundary\n'
            + f'boundary.execute({built!r}, ["normal"])\n')
        wrapper.chmod(0o700)
        result, code = measure.measure(wrapper, 'metadata', Path('/unused'), 3)
        self.assertEqual(code, 1)
        self.assertTrue(result['cleanup_complete'])
        self.assertNotEqual(result['exit_code'], 0)
        self.assertFalse((self.state / 'probe-state').exists())
        self.observations.append({'test': self.id(), 'result': result, 'target_not_executed': True})

    def test_G10_symlink_mount_source_and_external_data_rejected(self):
        alias = self.job / 'alias'
        alias.symlink_to(self.workspace)
        with self.assertRaises(boundary.BoundaryError):
            boundary.plan(self.program, alias, self.state, self.store, self.logs)
        with self.assertRaises(boundary.BoundaryError):
            boundary.plan(self.program, Path('/home'), self.state, self.store, self.logs)

    def test_G10_missing_kernel_capability_and_wrong_arch_fail_closed(self):
        with mock.patch.object(boundary.platform, 'machine', return_value='unknown'):
            with self.assertRaisesRegex(boundary.BoundaryError, 'unsupported_seccomp_architecture'):
                self.built()
        with mock.patch.object(boundary.os, 'memfd_create', side_effect=OSError('synthetic failure')):
            with self.assertRaises(OSError):
                boundary.execute(self.built(), ['normal'])
        with self.assertRaisesRegex(boundary.BoundaryError, 'unapproved_fixture_arguments'):
            boundary.execute(self.built(), ['a9'])

    def test_G12_no_executable_inference_route(self):
        for path in list((EXPERIMENT / 'src').rglob('*.rs')) + [EXPERIMENT / 'boundary.py']:
            source = path.read_text()
            self.assertNotRegex(source, r'\.(?:send|send_and_wait|send_and_wait_structured)\s*\(')
        source = (EXPERIMENT / 'src/main.rs').read_text()
        self.assertNotIn('"a9"', source)
        self.assertNotIn('"inference"', source)

    def test_G12_runtime_rejects_inference_mode_before_cli_start(self):
        result, code = measure.measure(EXPERIMENT / 'target/debug/narys-lr10a-poc',
                                       'a9', Path('/unused'), 3)
        self.assertEqual(code, 1)
        self.assertEqual(result['exit_code'], 2)
        self.assertTrue(result['cleanup_complete'])
        self.assertFalse(result['sdk_shutdown_verified'])
        self.observations.append({'test': self.id(), 'result': result,
                                  'inference_mode_rejected': True})


def record(path):
    log = io.StringIO()
    outcomes = []
    class Results(unittest.TextTestResult):
        def addSuccess(self, test):
            super().addSuccess(test)
            outcomes.append({'test': test.id(), 'status': 'PASS'})
        def addFailure(self, test, err):
            super().addFailure(test, err)
            outcomes.append({'test': test.id(), 'status': 'FAIL'})
        def addError(self, test, err):
            super().addError(test, err)
            outcomes.append({'test': test.id(), 'status': 'FAIL'})
    result = unittest.TextTestRunner(stream=log, verbosity=2, resultclass=Results).run(
        unittest.defaultTestLoader.loadTestsFromTestCase(BoundaryTests))
    evidence = {'phase': 'LR-10A FIX-3', 'tests': outcomes, 'tests_run': result.testsRun,
                'successful': result.wasSuccessful(), 'observations': BoundaryTests.observations,
                'inference_calls': 0, 'real_copilot_invocations_in_suite': 0,
                'boundary_fixture_sha256': hashlib.file_digest((EXPERIMENT / 'target/debug/boundary-fixture').open('rb'), 'sha256').hexdigest(),
                'boundary_source_sha256': hashlib.sha256((EXPERIMENT / 'boundary.py').read_bytes()).hexdigest()}
    path.write_text(json.dumps(evidence, indent=2) + '\n')
    path.with_suffix('.txt').write_text(log.getvalue().rstrip() + '\n')
    print(log.getvalue())
    return int(not result.wasSuccessful())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence', type=Path)
    args = parser.parse_args()
    if args.evidence:
        raise SystemExit(record(args.evidence))
    unittest.main(argv=['test_boundary'])
