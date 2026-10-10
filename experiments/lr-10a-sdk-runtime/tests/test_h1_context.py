"""H1 synthetic context gates; no real CLI, service change or credential access."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import h1_context as h1


def services(gui=True, locked=False):
    return {'gnome-shell_running': gui, 'gdm_running': gui,
            'runtime_bus_socket_accessible': True,
            'secrets_service_already_owned': True, 'login_collection_locked': locked}


def session(kind='tty', remote='yes', scope='session-4.scope'):
    return {'values': {'Type': kind, 'Remote': remote, 'Scope': scope, 'State': 'active'}}


def known_owner(graphical=False):
    return {'state': 'OBSERVED', 'in_graphical_login_scope': graphical}


class ContextTests(unittest.TestCase):
    def evaluate(self, s=None, rows=None, owner=None, target='inactive'):
        return h1.assess(s or services(False), rows or [session()],
                         owner or known_owner(), target)

    def test_gui_absence_never_confirms_authentication_or_cold_start(self):
        result = self.evaluate()
        self.assertEqual(result['scenario'], 'B_CONTEXT_CANDIDATE')
        for gate in ('GUI_ABSENT_AUTH', 'COLD_START_HEADLESS_AUTH', 'REAL_INFERENCE'):
            self.assertEqual(result[gate], 'NOT_TESTED')

    def test_without_display_is_not_gui_absence(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertEqual(self.evaluate(services())['scenario'], 'A_GUI_PRESENT')

    def test_graphical_session_or_target_detects_gui_without_shell(self):
        self.assertEqual(self.evaluate(rows=[session('wayland')])['scenario'], 'A_GUI_PRESENT')
        self.assertEqual(self.evaluate(target='active')['scenario'], 'A_GUI_PRESENT')

    def test_unknown_target_or_missing_session_data_is_inconclusive(self):
        self.assertEqual(self.evaluate(target=None)['scenario'], 'CONTEXT_INCONCLUSIVE')
        self.assertEqual(h1.assess(services(False), [], known_owner(), 'inactive')['scenario'],
                         'CONTEXT_INCONCLUSIVE')

    def test_dbus_or_service_presence_does_not_prove_unlock(self):
        for locked in (True, None):
            r = self.evaluate(services(False, locked))
            self.assertFalse(r['credential_service_available'])
            self.assertIn('unlocked_credential_service_unverified', r['transition_blockers'])

    def test_missing_service_or_bus_blocks(self):
        for key in ('runtime_bus_socket_accessible', 'secrets_service_already_owned'):
            s = services(False)
            s[key] = False
            self.assertEqual(self.evaluate(s)['transition'], 'BLOCKED_UNSAFE_OR_UNVERIFIED')

    def test_graphical_credential_owner_blocks_transition(self):
        r = self.evaluate(owner=known_owner(True))
        self.assertIn('credential_owner_in_graphical_login_scope', r['transition_blockers'])
        self.assertEqual(r['transition'], 'BLOCKED_UNSAFE_OR_UNVERIFIED')

    def test_unknown_owner_and_remote_session_block(self):
        for owner in ({'state': 'UNAVAILABLE'}, {'state': 'IDENTITY_CHANGED'}):
            self.assertIn('credential_owner_unverified', self.evaluate(owner=owner)['transition_blockers'])
        self.assertIn('remote_session_unverified',
                      self.evaluate(rows=[session(remote='no')])['transition_blockers'])

    def test_available_service_never_grants_transition_or_sensitive_operations(self):
        r = self.evaluate()
        self.assertEqual(r['transition'], 'REQUIRES_EXPLICIT_APPROVAL_AND_RECOVERY_PLAN')
        for gate in ('SESSION_ADMISSION', 'FINANCIAL_ADMISSION', 'AGENT_ACTION_ADMISSION'):
            self.assertEqual(r[gate], 'BLOCKED')
        self.assertEqual(r['CREDENTIAL_STORAGE_SAFETY'], 'INCONCLUSIVE')
        self.assertEqual(r['sdk_invocations'], 0)
        self.assertFalse(r['marker_claimed'])

    def test_unowned_service_is_never_activated(self):
        with mock.patch.object(h1.subprocess, 'run') as command:
            self.assertEqual(h1.owner({}, {}, [])['state'], 'SERVICE_NOT_OWNED')
            command.assert_not_called()

    def test_owner_query_is_noninteractive_and_never_reads_secrets(self):
        with mock.patch.object(h1.subprocess, 'run', return_value=
                mock.Mock(returncode=0, stdout=b'u 123\n')) as command, \
                mock.patch.object(h1, 'lifecycle', return_value=known_owner()) as observed:
            h1.owner({'HOME': '/synthetic'}, services(), [session()])
            observed.assert_called_once_with(123, [session()])
            args = command.call_args.args[0]
            self.assertIn('--auto-start=no', args)
            self.assertIn('--allow-interactive-authorization=no', args)
            self.assertIn('GetConnectionUnixProcessID', args)
            self.assertNotIn('GetSecrets', args)

    def test_raw_output_and_errors_are_not_published(self):
        for code in (0, 1):
            with mock.patch.object(h1.subprocess, 'run', return_value=mock.Mock(
                    returncode=code, stdout=b'State=public-synthetic-secret\nLinger=no\n')):
                r = h1.properties(['synthetic'], {'HOME': '/synthetic'}, {'Linger': 'yes|no'})
                self.assertNotIn('public-synthetic-secret', json.dumps(r))
                self.assertNotIn('State', r['values'])

    def test_pid_reuse_and_missing_identity_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            entry = Path(tmp) / '123'
            entry.mkdir()
            (entry / 'cgroup').write_text('0::/session-2.scope\n')
            first = {'pid': 123, 'ppid': 1, 'start_ticks': 10, 'comm': 'synthetic'}
            with mock.patch.object(h1, 'proc_identity', side_effect=[first, dict(first, start_ticks=11)]):
                self.assertEqual(h1.lifecycle(123, [], Path(tmp))['state'], 'IDENTITY_CHANGED')
            self.assertEqual(h1.lifecycle(456, [], Path(tmp))['state'], 'UNAVAILABLE')

    def test_scope_observation_is_anchored_and_sanitized(self):
        with tempfile.TemporaryDirectory() as tmp:
            entry = Path(tmp) / '123'
            entry.mkdir()
            (entry / 'cgroup').write_text('0::/synthetic/private/session-2.scope\n')
            identity = {'pid': 123, 'ppid': 1, 'start_ticks': 10, 'comm': 'synthetic'}
            rows = [session('wayland', 'no', 'session-2.scope'), session()]
            with mock.patch.object(h1, 'proc_identity', return_value=identity):
                r = h1.lifecycle(123, rows, Path(tmp))
                self.assertTrue(r['in_graphical_login_scope'])
                self.assertFalse(r['in_remote_login_scope'])
                self.assertNotIn('private', json.dumps(r))

    def test_observe_entry_cannot_run_without_ownership_harness(self):
        with mock.patch.object(sys, 'argv', ['h1_context.py', 'observe', '/synthetic']), \
                mock.patch.dict(os.environ, {}, clear=True), \
                mock.patch.object(h1, 'observe') as operation:
            with self.assertRaises(SystemExit):
                h1.main()
            operation.assert_not_called()


if __name__ == '__main__':
    unittest.main()
