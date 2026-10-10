import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import h3_headless as headless
import h3_metadata as driver
from h2_manual_unlock import UnlockBlocked


class HeadlessTests(unittest.TestCase):
    def test_marker_blocks_before_service_access(self):
        with mock.patch.object(headless, 'require_runtime'), \
             mock.patch.object(headless, 'attempt_state', return_value=True), \
             mock.patch.object(headless, 'gui_services') as services:
            with self.assertRaisesRegex(ValueError, 'attempt_marker_present'):
                headless.validate(Path('/synthetic'), unlocked=True)
            services.assert_not_called()

    def test_gui_never_becomes_headless_from_filtered_environment(self):
        for services, rows, ps_exit in (
            ({'gnome-shell_running': True}, [{'values': {'Type': 'tty'}}], 0),
            ({'gnome-shell_running': False}, [{'values': {'Type': 'wayland'}}], 1),
            ({'gnome-shell_running': False}, [{'values': {'Type': 'tty'}}], 2)):
            role = {'state': 'OBSERVED', 'in_user_manager': True,
                    'in_graphical_login_scope': False}
            with mock.patch.object(headless, 'require_runtime'), \
                 mock.patch.object(headless, 'attempt_state', return_value=False), \
                 mock.patch.object(headless, 'gui_services', return_value=services), \
                 mock.patch.object(headless, 'sessions', return_value=({}, rows)), \
                 mock.patch.object(headless, 'owner', return_value=role), \
                 mock.patch.object(headless, 'properties', side_effect=[
                     {'values': {'ActiveState': 'inactive'}}, {'exit_code': ps_exit}]):
                with self.assertRaises(UnlockBlocked):
                    headless.validate(Path('/synthetic'), unlocked=True)

    def test_pin_error_blocks_before_marker_and_service(self):
        with mock.patch.object(headless, 'require_runtime', side_effect=ValueError('pin')), \
             mock.patch.object(headless, 'attempt_state') as marker:
            with self.assertRaises(ValueError):
                headless.validate(Path('/synthetic'), unlocked=True)
            marker.assert_not_called()

    def test_fixed_evidence_is_burned_even_on_preflight_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root/'evidence').mkdir()
            with mock.patch.object(driver, 'ROOT', root), \
                 mock.patch.object(sys, 'argv', ['h3_metadata.py']), \
                 mock.patch.object(driver, 'validate') as validate, \
                 mock.patch.object(driver, 'measure') as measure, \
                 mock.patch('builtins.print'):
                self.assertEqual(driver.main(), 1)
                with self.assertRaises(FileExistsError):
                    driver.main()
            validate.assert_not_called()
            measure.assert_not_called()
            row = json.loads((root/'evidence/h3-metadata-real.json').read_text())
            self.assertEqual(row['sdk_invocations'], 0)
            self.assertEqual(row['FINANCIAL_ADMISSION'], 'BLOCKED')
            self.assertFalse(row['marker_claimed'])

    def test_no_retry_identity_override_or_sensitive_method(self):
        rust = (driver.ROOT/'src/bin/h3-metadata-confirm.rs').read_text()
        for forbidden in ('send_and_wait', '.send(', 'create_session', 'resume_session',
                          'delete_session', 'get_quota', 'list_models', 'claim_attempt('):
            self.assertNotIn(forbidden, rust)
        self.assertIn('h3-runtime-reservation.json', rust)
        self.assertIn('.create_new(true)', rust)
        with mock.patch.object(sys, 'argv', ['h3_metadata.py', '--force']), \
             mock.patch.object(driver.os, 'open') as opened:
            self.assertEqual(driver.main(), 2)
            opened.assert_not_called()


if __name__ == '__main__':
    unittest.main()
