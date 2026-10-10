"""Synthetic terminal and existing-only credential adapter tests. No personal vault."""
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import sys
import termios
import time
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / 'ops/credential_manager.py'
spec = importlib.util.spec_from_file_location('manager', SOURCE)
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)

class GateTests(unittest.TestCase):
    def test_pipe_and_automation_refused_before_service_or_password(self):
        p = subprocess.run(['/usr/bin/python3', '-I', '-c', SOURCE.read_text(), 'unlock-json'],
            input=b'synthetic-unread-input', capture_output=True, env={}, timeout=5)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(json.loads(p.stdout)['error_code'], 'human_private_ssh_terminal_required')
        self.assertNotIn(b'synthetic-unread-input', p.stdout + p.stderr)

    def test_agent_allocated_pty_is_not_ssh(self):
        master, slave = pty.openpty()
        try:
            p = subprocess.Popen(['/usr/bin/python3', '-I', '-c', SOURCE.read_text(), 'unlock-json'],
                stdin=slave, stdout=slave, stderr=slave, env={}, start_new_session=True)
            p.wait(timeout=5)
            output = os.read(master, 8192)
            self.assertNotEqual(p.returncode, 0)
            self.assertNotIn(b'Senha', output)
        finally:
            os.close(master); os.close(slave)

    def test_missing_service_status_is_sanitized(self):
        p = subprocess.run(['/usr/bin/python3', '-I', '-c', SOURCE.read_text(), 'status'],
            capture_output=True, env={'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/nonexistent-synthetic-bus'}, timeout=5)
        data = json.loads(p.stdout)
        self.assertFalse(data['service_available'])
        self.assertFalse(data['login_unlocked'])
        self.assertNotIn(b'Traceback', p.stdout+p.stderr)

    def terminal_case(self, interrupt=False):
        # Tests only HumanPassword memory/termios, not real SSH authentication.
        master, slave = pty.openpty()
        original = termios.tcgetattr(slave)
        program = '''
import ctypes, importlib.util, signal, sys
s=importlib.util.spec_from_file_location('m',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
m.require_human_tty=lambda: None
signal.signal(signal.SIGTERM,lambda *args: (_ for _ in ()).throw(m.Blocked('interrupted')))
p=m.HumanPassword()
try:
    with p:
        assert ctypes.string_at(p.address,p.length)==b'synthetic-fixture-only'
except m.Blocked:
    pass
assert all(v==0 for v in p.buffer)
print('MEMORY_WIPED')
'''
        p = subprocess.Popen(['/usr/bin/python3', '-I', '-c', program, str(SOURCE)],
            stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        try:
            self.assertTrue(select.select([master], [], [], 5)[0])
            output = os.read(master, 4096)
            self.assertIn(b'sem eco', output)
            self.assertFalse(termios.tcgetattr(slave)[3] & (termios.ECHO|termios.ECHONL))
            if interrupt: p.send_signal(signal.SIGTERM)
            else: os.write(master, b'synthetic-fixture-only\n')
            self.assertEqual(p.wait(timeout=5), 0)
            if select.select([master], [], [], 1)[0]: output += os.read(master, 4096)
            self.assertIn(b'MEMORY_WIPED', output)
            self.assertNotIn(b'synthetic-fixture-only', output)
            self.assertEqual(termios.tcgetattr(slave), original)
        finally:
            if p.poll() is None: p.kill(); p.wait()
            os.close(master); os.close(slave)

    def test_native_password_no_echo_wiped_and_tty_restored(self):
        self.terminal_case()

    def test_signal_wipes_and_restores_tty(self):
        self.terminal_case(True)

    def test_ssh_session_properties_and_interactive_shell_gate(self):
        from unittest.mock import MagicMock
        import stat
        import tempfile
        import grp
        with tempfile.TemporaryDirectory() as root:
            cli=Path(root)/'cli';shell=Path(root)/'shell';cli.mkdir();shell.mkdir()
            (cli/'status').write_text(f'Name: narys\nUid: {os.getuid()} {os.getuid()} {os.getuid()} {os.getuid()}\n')
            (shell/'exe').symlink_to('/usr/bin/bash')
            (shell/'cmdline').write_bytes(b'-bash\0')
            tty='/dev/pts/123'
            props={'Remote':True,'Service':'sshd','Type':'tty','Class':'user','User':(os.getuid(),'/user'),'TTY':'pts/123'}
            Gio=MagicMock();GLib=MagicMock();bus=Gio.bus_get_sync.return_value
            tty_stat=MagicMock(st_mode=stat.S_IFCHR|0o620,st_uid=os.getuid(),st_gid=grp.getgrnam('tty').gr_gid)
            with patch.object(manager.os,'isatty',return_value=True), patch.object(manager.os,'ttyname',return_value=tty), patch.object(manager.os,'stat',return_value=tty_stat), patch.object(manager.os,'tcgetpgrp',return_value=os.getpgrp()), patch.object(manager,'proc_fields',side_effect=lambda pid:(cli,99) if pid==os.getppid() else (shell,1)), patch.object(manager,'libraries',return_value=(Gio,GLib)):
                def responses(p):
                    a=MagicMock();a.unpack.return_value=('/session',)
                    b=MagicMock();b.unpack.return_value=(p,)
                    bus.call_sync.side_effect=[a,b]
                responses(props);manager.require_human_tty()
                for key,value in [('Remote',False),('Service','agent'),('User',(os.getuid()+1,'/user')),('TTY','pts/124')]:
                    bad=dict(props);bad[key]=value;responses(bad)
                    with self.assertRaises(manager.Blocked):manager.require_human_tty()
                (shell/'cmdline').write_bytes(b'bash\0-c\0synthetic-command\0')
                with self.assertRaises(manager.Blocked):manager.require_human_tty()

    def test_existing_collection_only_no_create_prompt_or_plain_session(self):
        text = SOURCE.read_text()
        for call in ("'CreateCollection'", "'GetSecret'", "'SetSecret'", "'Prompt'", "'Unlock'"):
            self.assertNotIn(call, text)
        self.assertIn('dh-ietf1024-sha256-aes128-cbc-pkcs7', text)

if __name__ == '__main__': unittest.main()
