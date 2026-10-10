"""H2 deterministic guards. Never unlock personal storage or start host services."""
import io
import fcntl
import os
from pathlib import Path
import subprocess
import pty
import select
import signal
import sys
import tempfile
import termios
import time
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import h2_keyring as launcher
import h2_manual_unlock as manual


class UnlockTests(unittest.TestCase):
    def test_real_synthetic_pty_disables_echo_and_keeps_password_out_of_output(self):
        # Only a PUBLIC synthetic password. Never run this fixture on personal input.
        master, slave = pty.openpty()
        child = None
        output = b''
        try:
            original = termios.tcgetattr(slave)
            script = "from h2_manual_unlock import human_password; p=human_password(); print('SYNTHETIC_TTY_PASS' if p=='public-h2-tty' else 'FAIL')"
            def controlling_tty():
                os.setsid()
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)
            child = subprocess.Popen(['/usr/bin/python3', '-c', script],
                stdin=slave, stdout=slave, stderr=subprocess.DEVNULL,
                env={'PATH': '/usr/bin', 'PYTHONPATH': str(launcher.ROOT)},
                preexec_fn=controlling_tty)
            deadline = time.monotonic() + 3
            while b'sem eco): ' not in output and time.monotonic() < deadline:
                if select.select([master], [], [], 0.05)[0]:
                    output += os.read(master, 4096)
                if child.poll() is not None:
                    break
            self.assertIn(b'sem eco): ', output)
            self.assertFalse(termios.tcgetattr(slave)[3] & termios.ECHO)
            os.write(master, b'public-h2-tty\n')
            child.wait(timeout=3)
            while select.select([master], [], [], 0)[0]:
                output += os.read(master, 4096)
            self.assertEqual(child.returncode, 0)
            self.assertIn(b'SYNTHETIC_TTY_PASS', output)
            self.assertNotIn(b'public-h2-tty', output)
            self.assertEqual(termios.tcgetattr(slave), original)
        finally:
            if child is not None:
                if child.poll() is None:
                    from measure import owned_handle
                    fd, _ = owned_handle(child.pid)
                    try:
                        signal.pidfd_send_signal(fd, signal.SIGTERM)
                    finally:
                        os.close(fd)
                child.wait(timeout=3)
            os.close(master)
            os.close(slave)

    def test_gui_or_unknown_context_never_admits_personal_unlock(self):
        services = {'gnome-shell_running': False}
        rows = [{'values': {'Type': 'tty'}}]
        role = {'state': 'OBSERVED', 'in_user_manager': True,
                'in_graphical_login_scope': False}
        manual.require_headless_service(services, rows, role, 'inactive')
        for s, r, o, t in (({'gnome-shell_running': True}, rows, role, 'inactive'),
                (services, [{'values': {'Type': 'wayland'}}], role, 'inactive'),
                (services, [], role, 'inactive'), (services, rows, role, None),
                (services, rows, dict(role, in_user_manager=False), 'inactive'),
                (services, rows, dict(role, in_graphical_login_scope=True), 'inactive'),
                (services, rows, dict(role, state='IDENTITY_CHANGED'), 'inactive')):
            with self.assertRaises(manual.UnlockBlocked):
                manual.require_headless_service(s, r, o, t)

    def client(self, locked=True, algorithm=manual.ALGORITHM):
        client = manual.ExistingLogin.__new__(manual.ExistingLogin)
        client.owner = ':synthetic.1'
        client.bus = mock.Mock()
        client.service = mock.Mock()
        client.service.get_session_algorithms.return_value = algorithm
        client.locked = mock.Mock(side_effect=[locked, False])
        client.call = mock.Mock()
        return client

    def test_noninteractive_terminal_rejected_before_password_or_dbus(self):
        with mock.patch.object(sys, 'argv', ['h2_manual_unlock.py', 'unlock-existing-login']), \
             mock.patch.object(sys.stdin, 'isatty', return_value=False), \
             mock.patch.object(manual, 'ExistingLogin') as service:
            with self.assertRaisesRegex(manual.UnlockBlocked, 'human_private_terminal_required'):
                manual.main()
            service.assert_not_called()

    def test_no_password_arguments_or_unknown_mode(self):
        for args in (['unlock-existing-login', 'public-synthetic-password'], ['force'], []):
            with mock.patch.object(sys, 'argv', ['h2_manual_unlock.py'] + args):
                with self.assertRaisesRegex(manual.UnlockBlocked, 'fixed_human_operation_required'):
                    manual.main()

    def test_automated_ownership_worker_cannot_prompt(self):
        with mock.patch.object(sys, 'argv', ['h2_manual_unlock.py', 'unlock-existing-login']), \
             mock.patch.object(sys.stdin, 'isatty', return_value=True), \
             mock.patch.object(sys.stdout, 'isatty', return_value=True), \
             mock.patch.dict(os.environ, {'NARYS_LR10A_OWNED_HARNESS': '1'}):
            with self.assertRaisesRegex(manual.UnlockBlocked, 'human_private_terminal_required'):
                manual.main()

    def test_noninteractive_password_has_no_stdin_fallback(self):
        with mock.patch.object(sys.stdin, 'isatty', return_value=False), \
             mock.patch('builtins.open') as opened:
            with self.assertRaises(manual.UnlockBlocked):
                manual.human_password()
            opened.assert_not_called()

    def terminal(self):
        tty = mock.MagicMock()
        tty.__enter__.return_value = tty
        tty.fileno.return_value = 42
        original = [0, 0, 0, termios.ECHO | termios.ISIG, 0, 0, []]
        return tty, original

    def test_echo_disabled_and_restored_without_password_output(self):
        tty, original = self.terminal()
        tty.readline.return_value = 'public-h2-test-password\n'
        with mock.patch.object(sys.stdin, 'isatty', return_value=True), \
             mock.patch.object(sys.stdout, 'isatty', return_value=True), \
             mock.patch('builtins.open', return_value=tty), \
             mock.patch.object(manual.termios, 'tcgetattr', return_value=original), \
             mock.patch.object(manual.termios, 'tcsetattr') as attributes:
            self.assertEqual(manual.human_password(), 'public-h2-test-password')
        self.assertFalse(attributes.call_args_list[0].args[2][3] & termios.ECHO)
        self.assertEqual(attributes.call_args_list[1].args[2], original)
        self.assertNotIn('public-h2-test-password', str(tty.write.call_args_list))

    def test_interruption_restores_terminal(self):
        tty, original = self.terminal()
        tty.readline.side_effect = KeyboardInterrupt
        with mock.patch.object(sys.stdin, 'isatty', return_value=True), \
             mock.patch.object(sys.stdout, 'isatty', return_value=True), \
             mock.patch('builtins.open', return_value=tty), \
             mock.patch.object(manual.termios, 'tcgetattr', return_value=original), \
             mock.patch.object(manual.termios, 'tcsetattr') as attributes:
            with self.assertRaises(KeyboardInterrupt):
                manual.human_password()
        self.assertEqual(attributes.call_args_list[-1].args[2], original)

    def test_unlocked_collection_never_receives_password(self):
        client = self.client(locked=False)
        with self.assertRaisesRegex(manual.UnlockBlocked, 'locked_existing_login_required'):
            client.unlock('public-synthetic-password')
        client.service.encode_dbus_secret.assert_not_called()
        client.call.assert_not_called()

    def test_invalid_password_rejected_without_sending(self):
        for password in ('', 'x\x00y', 'x' * 8193):
            client = self.client()
            with self.assertRaisesRegex(manual.UnlockBlocked, 'invalid_password_input'):
                client.unlock(password)
            client.call.assert_not_called()

    def test_plain_payload_rejected_before_unlock(self):
        client = self.client()
        encoded = mock.Mock()
        encoded.get_type_string.return_value = '(oayays)'
        encoded.get_child_value.return_value.unpack.return_value = []
        client.service.encode_dbus_secret.return_value = encoded
        with self.assertRaisesRegex(manual.UnlockBlocked, 'encrypted_payload_required'):
            client.unlock('public-synthetic-password')
        client.call.assert_not_called()

    def test_constructor_does_not_activate_missing_service(self):
        bus = mock.Mock()
        bus.call_sync.return_value.unpack.return_value = (False,)
        with mock.patch.object(manual.Gio, 'bus_get_sync', return_value=bus), \
             mock.patch.object(manual.Secret.Service, 'open_sync') as opened:
            with self.assertRaisesRegex(manual.UnlockBlocked, 'existing_secret_service_required'):
                manual.ExistingLogin()
            opened.assert_not_called()
        self.assertEqual(bus.call_sync.call_args.args[-3], manual.Gio.DBusCallFlags.NO_AUTO_START)

    def test_missing_collection_never_opens_secret_session(self):
        bus = mock.Mock()
        bus.call_sync.return_value.unpack.side_effect = [(True,), (':1.3',)]
        with mock.patch.object(manual.Gio, 'bus_get_sync', return_value=bus), \
             mock.patch.object(manual.ExistingLogin, 'call') as method, \
             mock.patch.object(manual.Secret.Service, 'open_sync') as opened:
            method.return_value.unpack.return_value = ('/',)
            with self.assertRaisesRegex(manual.UnlockBlocked, 'existing_login_required'):
                manual.ExistingLogin()
            opened.assert_not_called()
            self.assertEqual(method.call_args.args[1], 'ReadAlias')

    def test_encryption_fallback_not_admitted(self):
        bus = mock.Mock()
        bus.call_sync.return_value.unpack.side_effect = [(True,), (':1.3',)]
        service = mock.Mock()
        service.get_session_algorithms.return_value = 'plain'
        with mock.patch.object(manual.Gio, 'bus_get_sync', return_value=bus), \
             mock.patch.object(manual.ExistingLogin, 'call') as method, \
             mock.patch.object(manual.Secret.Service, 'open_sync', return_value=service) as opened:
            method.return_value.unpack.return_value = (manual.LOGIN,)
            with self.assertRaisesRegex(manual.UnlockBlocked, 'encrypted_secret_session_required'):
                manual.ExistingLogin()
            self.assertEqual(opened.call_args.args[1], ':1.3')
            service.encode_dbus_secret.assert_not_called()

    def test_unlock_error_does_not_retry_create_or_prompt(self):
        client = self.client()
        from gi.repository import GLib
        encoded = GLib.Variant('(oayays)', ('/synthetic/session', [0] * 16, [0] * 16, 'text/plain'))
        client.service.encode_dbus_secret.return_value = encoded
        client.call.side_effect = OSError('public-synthetic-error')
        with self.assertRaises(OSError):
            client.unlock('public-synthetic-password')
        self.assertEqual(client.call.call_count, 1)
        self.assertEqual(client.call.call_args.args[:2], (manual.INTERNAL, 'UnlockWithMasterPassword'))


class NamespaceTests(unittest.TestCase):
    def test_namespace_omits_personal_home_host_bus_network_and_environment(self):
        binary = launcher.ROOT / 'target/debug/h2-stronghold-fixture'
        with tempfile.TemporaryDirectory(prefix='narys-h2-test-') as temp:
            command = launcher.plan(Path(temp), binary)
        self.assertIn('--unshare-all', command)
        self.assertIn('--clearenv', command)
        self.assertNotIn('--share-net', command)
        self.assertNotIn('/home/sam', command)
        self.assertNotIn('/run/user', command)
        ro = [command[i + 1] for i, word in enumerate(command) if word == '--ro-bind']
        self.assertNotIn('/', ro)
        self.assertNotIn('/etc', ro)
        self.assertNotIn('/home', ro)
        self.assertIn('/usr', ro)  # Trusted software mount, synthetic-only.

    def test_unsafe_job_permissions_block(self):
        with tempfile.TemporaryDirectory(prefix='narys-h2-test-') as temp:
            Path(temp).chmod(0o755)
            with self.assertRaises(ValueError):
                launcher.plan(Path(temp), launcher.ROOT / 'target/debug/h2-stronghold-fixture')

    def test_arbitrary_backend_binary_rejected(self):
        with tempfile.TemporaryDirectory(prefix='narys-h2-test-') as temp:
            with self.assertRaisesRegex(ValueError, 'synthetic_backend_binary_required'):
                launcher.plan(Path(temp), Path('/usr/bin/true'))

    def test_rust_fixture_cannot_open_personal_storage_outside_namespace(self):
        binary = launcher.ROOT / 'target/debug/h2-stronghold-fixture'
        result = subprocess.run([str(binary), 'presence'], env={'HOME': '/synthetic-not-a-namespace'},
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=3)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, b'')

    def test_personal_helper_has_no_daemon_creation_or_credential_retrieval(self):
        source = Path(manual.__file__).read_text()
        for forbidden in ('CreateCollection', 'GetSecret', 'Store', 'Prompt'):
            # They appear only in a comment documenting absence, never call literals.
            self.assertNotIn("'" + forbidden + "'", source)
        self.assertNotIn('subprocess.', source)
        self.assertNotIn('claim_attempt', source)
        self.assertNotIn('password=', source)


if __name__ == '__main__':
    unittest.main()
