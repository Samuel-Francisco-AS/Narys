#!/usr/bin/python3
"""Run only under a fresh dbus-run-session + synthetic HOME/runtime.

Fixture explicitly creates its own login collection. Production never creates.
The native encrypted extension is exercised against real GNOME50 with test-only
subclasses providing synthetic password and bypassing host identity admission.
"""
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time

source = Path(__file__).resolve().parents[1] / 'ops/credential_manager.py'
spec = importlib.util.spec_from_file_location('m', source)
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
require = m.require
require(os.environ.get('NARYS_SYNTHETIC_KEYRING') == '1', 'synthetic_fixture_only')
require(Path.home().name.startswith('narys-synthetic-'), 'synthetic_home_required')
password = b'narys-synthetic-keyring-only'
class FixturePassword(m.HumanPassword):
    def __enter__(self):
        ctypes.memmove(self.address, password, len(password))
        self.length = len(password)
        return self
class FixtureLogin(m.ExistingLogin):
    def verify_backend(self):
        # Owner is still a unique bus name from this isolated bus. No real user service.
        pass
m.HumanPassword = FixturePassword
# Existing daemon never inherited. This fixture creates the disposable login only.
daemon = subprocess.Popen(['/usr/bin/gnome-keyring-daemon', '--foreground', '--unlock', '--components=secrets'],
    stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    daemon.stdin.write(password); daemon.stdin.close()
    client = None
    for _ in range(100):
        try: client = FixtureLogin(); break
        except Exception: time.sleep(.05)
    require(client is not None, 'synthetic_service_unavailable')
    require(client.locked() is False, 'synthetic_initial_unlock_failed')
    status_unlocked = subprocess.run(['/usr/bin/python3','-I','-c',source.read_text(),'status'],capture_output=True,check=True)
    require(json.loads(status_unlocked.stdout)['state'] == 'unlocked', 'synthetic_unlocked_status_failed')
    # The production identity gate rejects this disposable non-systemd daemon.
    try:
        m.ExistingLogin().verify_backend()
        raise AssertionError('unowned_synthetic_daemon_accepted')
    except m.Blocked as e:
        require(str(e) == 'credential_owner_not_user_service', 'synthetic_identity_error_unexpected')
    saved_pin = m.DAEMON_SHA
    try:
        m.DAEMON_SHA = 'synthetic-incompatible-backend'
        try:
            m.ExistingLogin().verify_backend()
            raise AssertionError('incompatible_backend_accepted')
        except m.Blocked as e:
            require(str(e) == 'credential_backend_incompatible', 'synthetic_pin_error_unexpected')
    finally:
        m.DAEMON_SHA = saved_pin
    before = {p.name: p.read_bytes() for p in (Path.home()/'.local/share/keyrings').glob('*.keyring')}
    def lock():
        client.call('org.freedesktop.Secret.Service', 'Lock', m.libraries()[1].Variant('(ao)', ([m.LOGIN],)))
        require(client.locked() is True, 'synthetic_lock_failed')
    lock()
    status_locked = subprocess.run(['/usr/bin/python3','-I','-c',source.read_text(),'status'],capture_output=True,check=True)
    require(json.loads(status_locked.stdout)['state'] == 'locked', 'synthetic_locked_status_failed')
    password = b'synthetic-wrong-password'
    try:
        client.unlock()
        raise AssertionError('wrong_password_accepted')
    except m.Blocked as e:
        require(str(e).startswith('credential_password_rejected'), 'wrong_password_error_unsanitized')
    require(client.locked() is True, 'wrong_password_unlocked')
    password = b'narys-synthetic-keyring-only'
    require(client.unlock() == 'unlocked', 'synthetic_unlock_failed')
    require(client.locked() is False, 'synthetic_not_unlocked')
    require(client.unlock() == 'already_unlocked', 'synthetic_reprompt_failed')
    # No collection/snapshot rewritten by unlock; keyring native access-only behavior.
    after = {p.name: p.read_bytes() for p in (Path.home()/'.local/share/keyrings').glob('*.keyring')}
    require(before == after, 'synthetic_keyring_bytes_changed')
    print(json.dumps({'synthetic_existing_keyring':True,'encrypted_native_session':True,
        'wrong_password_rejected':True,'correct_password_unlock':True,'keyring_bytes_preserved':True,
        'locked_unlocked_metadata_diagnostics':True,'already_unlocked_no_reprompt':True,
        'production_rejects_non_service_backend':True,'incompatible_backend_pin_rejected':True,'personal_vault_accessed':False,'fixture_gate_bypass_only':True}))
finally:
    daemon.terminate()
    try: daemon.wait(timeout=5)
    except subprocess.TimeoutExpired: daemon.kill(); daemon.wait()
