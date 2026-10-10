"""Synthetic profile/identity/failure tests. No Copilot, credential or real send."""
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import diagnose_a9_auth as headless
import diagnose_a9_gui_auth as gui
from run_a9_host import marker_directory


def services():
    return {'gnome-shell_running': True, 'gdm_running': True,
            'gnome-keyring-daemon_running': True,
            'secrets_service_already_owned': True,
            'runtime_bus_socket_accessible': True, 'login_collection_locked': False}


def measurement(report):
    return {'cleanup_complete': True, 'kernel_children_exhausted': True,
            'ownership_errors': [], 'timed_out': False, 'stdout_truncated': False,
            'attributed_processes': [], 'sdk_report': report}


def identity_report(version='1.0.95'):
    return measurement({'native_version': version, 'version_exit_code': 0,
        'help_exit_code': 0, 'help_flags_present': {
            '--disable-builtin-mcps': True, '--log-dir': True, '--no-auto-update': True}})


class GuiAuthTests(unittest.TestCase):
    def test_environment_excludes_display_tokens_home_overrides_and_unknowns(self):
        env = gui.gui_environment({'HOME': '/synthetic', 'PATH': '/extra',
            'DISPLAY': ':synthetic', 'WAYLAND_DISPLAY': 'synthetic',
            'GH_TOKEN': 'public-fake-secret', 'COPILOT_HOME': '/unapproved',
            'UNAPPROVED': 'public-fake-secret'})
        self.assertEqual(set(env), {'HOME', 'PATH', 'LANG', 'COPILOT_SKIP_CLI_DOWNLOAD'})
        self.assertEqual(env['PATH'], '/usr/bin')
        self.assertNotIn('public-fake-secret', repr(env))

    def test_gui_profile_requires_gui_existing_service_and_unlocked_boolean(self):
        gui.require_gui_context(services())
        for key, value in [('gnome-shell_running', False),
                           ('secrets_service_already_owned', False),
                           ('runtime_bus_socket_accessible', False),
                           ('login_collection_locked', True),
                           ('login_collection_locked', None)]:
            s = services()
            s[key] = value
            with self.assertRaises(ValueError):
                gui.require_gui_context(s)

    def test_locked_query_never_activates_absent_service_or_queries_contents(self):
        s = services()
        s['secrets_service_already_owned'] = False
        with mock.patch.object(gui, 'headless_metadata', return_value=s), \
                mock.patch.object(gui.subprocess, 'run') as command:
            self.assertIsNone(gui.gui_services({'HOME': '/synthetic'})['login_collection_locked'])
            command.assert_not_called()
        for stdout, expected in [(b'b false\n', False), (b'b true\n', True),
                                 (b'public-invalid-private-prose', None)]:
            with mock.patch.object(gui, 'headless_metadata', return_value=services()), \
                    mock.patch.object(gui.subprocess, 'run', return_value=
                        mock.Mock(returncode=0, stdout=stdout)) as command:
                result = gui.gui_services({'HOME': '/synthetic'})
                self.assertIs(result['login_collection_locked'], expected)
                argv = command.call_args.args[0]
                self.assertIn('get-property', argv)
                self.assertIn('--auto-start=no', argv)
                self.assertIn('--allow-interactive-authorization=no', argv)
                self.assertEqual(argv[-1], 'Locked')
                self.assertNotIn('Items', argv)
                self.assertNotIn('GetSecrets', argv)
                self.assertNotIn('public-invalid-private-prose', repr(result))

    def test_native_identity_rejects_changed_pin_non_elf_and_unknown_manifest(self):
        manifest = json.loads(gui.MANIFEST.read_text())
        with tempfile.TemporaryDirectory() as tmp:
            cli = Path(tmp) / 'synthetic-native'
            cli.write_bytes(b'\x7fELFpublic-fixture')
            with self.assertRaisesRegex(ValueError, 'candidate_pin_mismatch'):
                gui.require_runtime(manifest, cli)
            cli.write_bytes(b'#!/synthetic')
            with self.assertRaisesRegex(ValueError, 'native_elf_required'):
                gui.require_runtime(manifest, cli)
            with self.assertRaisesRegex(ValueError, 'invalid_runtime_manifest'):
                gui.require_runtime(dict(manifest, sdk_version='unknown'), cli)

    def test_version_and_required_flags_never_fallback_on_unknown_or_old_runtime(self):
        manifest = json.loads(gui.MANIFEST.read_text())
        gui.require_version(identity_report(), manifest)
        for version in ('1.0.91', 'unrecognized', None):
            with self.assertRaises(ValueError):
                gui.require_version(identity_report(version), manifest)
        for flag in ('--disable-builtin-mcps', '--log-dir', '--no-auto-update'):
            report = identity_report()
            report['sdk_report']['help_flags_present'][flag] = False
            with self.assertRaises(ValueError):
                gui.require_version(report, manifest)

    def test_cleanup_timeout_missing_exhaustion_or_survivor_fails_closed(self):
        for key, value in [('timed_out', True), ('cleanup_complete', False),
                           ('kernel_children_exhausted', False),
                           ('ownership_errors', ['identity_unavailable']),
                           ('stdout_truncated', True)]:
            report = measurement({})
            report[key] = value
            with self.assertRaises(ValueError):
                gui.require_cleanup(report)
        with mock.patch.object(gui, 'identities_absent', return_value=False):
            with self.assertRaises(ValueError):
                gui.require_cleanup(measurement({}))

    def test_historical_headless_main_still_rejects_gui_before_runtime_start(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            marker_directory(home)
            cli = home / 'synthetic-cli'
            cli.touch()
            out = home / 'headless.json'
            with mock.patch.dict(os.environ, {'HOME': tmp, 'PATH': '/usr/bin'}, clear=True), \
                    mock.patch.object(sys, 'argv', ['headless', str(cli), '--output', str(out)]), \
                    mock.patch.object(headless, 'digest', return_value=headless.CLI_SHA), \
                    mock.patch.object(headless, 'config_stat', return_value={'inode': 1}), \
                    mock.patch.object(headless, 'headless_metadata', return_value=services()), \
                    mock.patch.object(headless, 'measure') as run, \
                    mock.patch('sys.stdout', new=io.StringIO()):
                self.assertEqual(headless.main(), 1)
                run.assert_not_called()
            self.assertEqual(json.loads(out.read_text())['diagnostic_error'], 'unexpected_graphical_session')

    def run_main(self, marker=False, version='1.0.95', config_changed=False):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            marker_directory(home)
            attempt = home / '.local/state/narys/lr10a-a9-host-attempt.json'
            if marker:
                attempt.write_bytes(b'public-corrupt-attempt-fixture')
            cli = home / 'synthetic-cli'
            cli.touch()
            out = home / 'gui.json'
            with mock.patch.dict(os.environ, {'HOME': tmp, 'PATH': '/usr/bin',
                                 'GH_TOKEN': 'public-fake-secret'}, clear=True), \
                    mock.patch.object(sys, 'argv', ['gui', str(cli), '--output', str(out)]), \
                    mock.patch.object(gui, 'require_runtime'), \
                    mock.patch.object(gui, 'digest', return_value='synthetic-digest'), \
                    mock.patch.object(gui, 'config_stat', side_effect=[{'inode': 1},
                        {'inode': 1}, {'inode': 2 if config_changed else 1}]), \
                    mock.patch.object(gui, 'gui_services', side_effect=lambda _: services()) as s, \
                    mock.patch.object(gui, 'measure', side_effect=[(identity_report(version), 0),
                        (measurement({'preflight': {'auth': {'authenticated': True}}}), 1)]) as run, \
                    mock.patch('sys.stdout', new=io.StringIO()):
                code = gui.main()
            text = out.read_text()
            self.assertNotIn('public-fake-secret', text)
            self.assertEqual(out.stat().st_mode & 0o777, 0o600)
            self.assertEqual(attempt.exists(), marker)
            if marker:
                self.assertEqual(attempt.read_bytes(), b'public-corrupt-attempt-fixture')
            return code, json.loads(text), run.call_count, s.call_count

    def test_existing_marker_blocks_all_services_and_runtime_without_claim(self):
        code, result, calls, services_calls = self.run_main(marker=True)
        self.assertEqual(code, 1)
        self.assertEqual(calls, 0)
        self.assertEqual(services_calls, 0)
        self.assertEqual(result['diagnostic_error'], 'attempt_marker_exists')

    def test_synthetic_gui_auth_does_not_claim_headless_financial_or_inference(self):
        code, result, calls, _ = self.run_main()
        self.assertEqual(code, 0)
        self.assertEqual(calls, 2)
        self.assertEqual(result['sdk_authenticated_with_gui'], 'PASS')
        self.assertEqual(result['verification_state'], 'PASS')
        self.assertEqual(result['sdk_authenticated_headless'], 'NOT_PROVEN')
        self.assertEqual(result['financial_admission'], 'BLOCKED')
        self.assertEqual(result['a9_real_inference'], 'NOT_RUN')
        self.assertEqual(result['real_session_operations'], 0)
        self.assertFalse(result['marker_claimed'])

    def test_auth_observation_survives_integrity_failure_without_admission(self):
        code, result, calls, _ = self.run_main(config_changed=True)
        self.assertEqual(code, 1)
        self.assertEqual(calls, 2)
        self.assertEqual(result['sdk_authenticated_with_gui'], 'PASS')
        self.assertEqual(result['verification_state'], 'BLOCKED')
        self.assertEqual(result['diagnostic_error'], 'configuration_or_context_changed')
        self.assertEqual(result['financial_admission'], 'BLOCKED')
        self.assertEqual(result['inference_calls'], 0)
        self.assertEqual(result['real_session_operations'], 0)
        self.assertFalse(result['marker_claimed'])

    def test_incompatible_runtime_stops_before_sdk_metadata(self):
        code, result, calls, _ = self.run_main(version='1.0.91')
        self.assertEqual(code, 1)
        self.assertEqual(calls, 1)
        self.assertEqual(result['diagnostic_error'], 'version_or_required_cli_flags_unverified')
        self.assertNotIn('sdk_metadata', result)


if __name__ == '__main__':
    unittest.main()
