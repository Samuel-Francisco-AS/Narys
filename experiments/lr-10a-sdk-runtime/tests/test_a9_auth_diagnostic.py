"""Synthetic environment/marker controls. Never start Copilot or acquire auth."""
import os
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import diagnose_a9_auth as diagnostic
from run_a9_host import marker_directory


class AuthContextTests(unittest.TestCase):
    def test_baseline_excludes_credentials_and_unapproved_context(self):
        context = {'HOME': '/synthetic', 'PATH': '/usr/bin',
                   'GH_TOKEN': 'public-fixture-marker', 'DISPLAY': 'fixture-display',
                   'COPILOT_HOME': '/fixture-private-store',
                   'UNRECOGNIZED': 'public-fixture-marker'}
        rows = list(diagnostic.variants(context))
        self.assertEqual(len(rows), 1)
        self.assertEqual(set(rows[0][2]), {'HOME', 'PATH', 'LANG', 'COPILOT_SKIP_CLI_DOWNLOAD'})
        self.assertNotIn('public-fixture-marker', repr(rows))

    def test_every_variant_changes_exactly_one_dimension(self):
        with tempfile.TemporaryDirectory() as root:
            context = {name: root for name in diagnostic.CONTEXT}
            context['PATH'] = root + ':/usr/bin'
            context['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=/synthetic/bus'
            rows = list(diagnostic.variants(context))
            self.assertEqual(len(rows), 8)
            base = rows[0][2]
            for _, dimension, env in rows[1:]:
                changed = {k for k in base.keys() | env.keys() if base.get(k) != env.get(k)}
                self.assertEqual(changed, {dimension})
                self.assertNotIn('GH_TOKEN', env)

    def test_path_rejects_empty_relative_or_writable_directories(self):
        for path in ('', '.:/usr/bin', '/usr/bin:', 'relative'):
            with self.assertRaises(ValueError):
                diagnostic.checked_path(path)
        with tempfile.TemporaryDirectory() as root:
            Path(root).chmod(0o777)
            with self.assertRaises(ValueError):
                diagnostic.checked_path(root)

    def test_path_missing_entries_omitted_without_listing_or_contents(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(diagnostic.checked_path(root + ':' + root + '/missing:' + root), root)

    def test_invalid_plan_rejects_before_real_probe(self):
        with mock.patch.object(diagnostic, 'measure') as measure:
            for context in ({'PATH': '/usr/bin'},
                            {'HOME': '/synthetic', 'PATH': '/usr/bin', 'GH_CONFIG_DIR': 'relative'}):
                with self.assertRaises(ValueError):
                    list(diagnostic.variants(context))
            measure.assert_not_called()

    def test_read_only_marker_check_never_creates_directory_or_file(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            with self.assertRaises(FileNotFoundError):
                diagnostic.attempt_state(home)
            self.assertFalse((home / '.local').exists())
            marker_directory(home)
            before = (home / '.local/state/narys').stat()
            self.assertFalse(diagnostic.attempt_state(home))
            after = (home / '.local/state/narys').stat()
            self.assertEqual(before.st_mtime_ns, after.st_mtime_ns)
            self.assertEqual(before.st_ctime_ns, after.st_ctime_ns)

    def test_marker_any_entry_blocks_without_reading(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            marker_directory(home)
            marker = home / '.local/state/narys/lr10a-a9-host-attempt.json'
            marker.symlink_to('/not-readable-synthetic-target')
            self.assertTrue(diagnostic.attempt_state(home))
            marker.unlink()
            marker.write_bytes(b'public-corrupt-fixture')
            marker.chmod(0o000)
            self.assertTrue(diagnostic.attempt_state(home))
            marker.chmod(0o600)

    def test_marker_symlink_directory_or_mode_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            (home / '.local').symlink_to('/nonexistent')
            with self.assertRaises(OSError):
                diagnostic.attempt_state(home)
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            marker_directory(home)
            (home / '.local/state/narys').chmod(0o755)
            with self.assertRaises(ValueError):
                diagnostic.attempt_state(home)

    def test_pid_reuse_is_not_a_surviving_owned_identity(self):
        report = {'attributed_processes': [{'pid': 123, 'start_ticks': 10}]}
        with mock.patch.object(diagnostic, 'identity', return_value={'start_ticks': 11}):
            self.assertTrue(diagnostic.identities_absent(report))
        with mock.patch.object(diagnostic, 'identity', return_value={'start_ticks': 10}):
            self.assertFalse(diagnostic.identities_absent(report))
        with mock.patch.object(diagnostic, 'identity', side_effect=FileNotFoundError):
            self.assertTrue(diagnostic.identities_absent(report))

    def test_existing_attempt_blocks_main_before_any_cli_or_service_probe(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            marker_directory(home)
            marker = home / '.local/state/narys/lr10a-a9-host-attempt.json'
            marker.write_bytes(b'public-corrupt-attempt-fixture')
            cli = home / 'synthetic-cli'
            cli.touch()
            output = home / 'safe-evidence.json'
            with mock.patch.dict(os.environ, {'HOME': root, 'PATH': '/usr/bin',
                                 'GH_TOKEN': 'synthetic-do-not-export'}, clear=True), \
                    mock.patch.object(sys, 'argv', ['diagnostic', str(cli), '--output', str(output)]), \
                    mock.patch.object(diagnostic, 'digest', return_value=diagnostic.CLI_SHA), \
                    mock.patch.object(diagnostic, 'config_stat', return_value={'inode': 1}), \
                    mock.patch.object(diagnostic, 'headless_metadata') as service, \
                    mock.patch.object(diagnostic, 'measure') as measure, \
                    mock.patch('sys.stdout', new=io.StringIO()):
                self.assertEqual(diagnostic.main(), 1)
                service.assert_not_called()
                measure.assert_not_called()
            text = output.read_text()
            result = json.loads(text)
            self.assertEqual(result['diagnostic_error'], 'attempt_marker_exists')
            self.assertTrue(result['marker_present_before'])
            self.assertTrue(result['environment_presence']['GH_TOKEN'])
            self.assertNotIn('synthetic-do-not-export', text)
            self.assertEqual(output.stat().st_mode & 0o777, 0o600)
            self.assertEqual(marker.read_bytes(), b'public-corrupt-attempt-fixture')


if __name__ == '__main__':
    unittest.main()
