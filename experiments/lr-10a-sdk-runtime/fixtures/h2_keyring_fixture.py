#!/usr/bin/python3
"""Real GNOME50/libsecret behavior, SYNTHETIC namespace only, no personal mode."""
import hashlib
import json
import os
from pathlib import Path
import signal
import resource
import subprocess
import time
import tempfile

import dbus
from measure import owned_handle, identity
from h2_manual_unlock import ExistingLogin

PASSWORD = b'public-h2-synthetic-password'
LOGIN = '/org/freedesktop/secrets/collection/login'
SERVICE = '/org/freedesktop/secrets'
SETUP_STAGE = 'namespace_validation'
DIAGNOSTIC = None


def check(condition, code):
    if not condition:
        raise ValueError(code)


def stop(child):
    if child.poll() is None:
        fd, _ = owned_handle(child.pid)
        try:
            signal.pidfd_send_signal(fd, signal.SIGTERM)
        finally:
            os.close(fd)
    child.wait(timeout=4)


def call(bus, path, interface, method, signature='', arguments=()):
    return bus.call_blocking('org.freedesktop.secrets', path, interface, method,
                             signature, arguments, timeout=3)


def present(bus):
    return str(call(bus, SERVICE, 'org.freedesktop.Secret.Service',
                    'ReadAlias', 's', ('login',))) == LOGIN


def locked(bus):
    return bool(call(bus, LOGIN, 'org.freedesktop.DBus.Properties', 'Get',
                     'ss', ('org.freedesktop.Secret.Collection', 'Locked')))


def start_keyring(bus, password=None):
    started = time.monotonic()
    argv = ['/usr/bin/gnome-keyring-daemon', '--foreground',
        '--components=secrets', '--control-directory=/run/h2/keyring']
    if password is not None:
        argv.append('--unlock')
    process = subprocess.Popen(argv,
        stdin=subprocess.PIPE if password is not None else subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=DIAGNOSTIC)
    if password is not None:
        process.stdin.write(password)
        process.stdin.close()
    deadline = time.monotonic() + 4
    while time.monotonic() < deadline:
        # Bounded readiness condition, not a sleep used to hide a lifecycle error.
        if bus.name_has_owner('org.freedesktop.secrets'):
            owner = int(bus.call_blocking('org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', 'GetConnectionUnixProcessID', 's',
                ('org.freedesktop.secrets',), timeout=3))
            check(owner == process.pid, 'synthetic_owner_mismatch')
            process.h2_startup_ms = round((time.monotonic() - started) * 1000, 2)
            return process
        check(process.poll() is None, 'synthetic_daemon_early_exit')
        time.sleep(0.01)
    raise ValueError('synthetic_daemon_readiness_timeout')


def unlock(bus, password, existing_only=True):
    check(existing_only and present(bus), 'existing_collection_required')
    # Same libsecret encrypted transport as the pending human terminal helper.
    client = ExistingLogin()
    try:
        client.unlock(password.decode())
    finally:
        client.close()


def backend(operation):
    p = subprocess.run(['/h2/stronghold-fixture', operation], stdin=subprocess.DEVNULL,
                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    check(p.returncode == 0, 'synthetic_rust_backend_failed')
    r = json.loads(p.stdout)
    check(r.get('synthetic_backend_operation') == 'PASS', 'synthetic_backend_report_invalid')
    check(r.get('secret_values_returned') is False, 'synthetic_backend_value_exposed')
    return r


def main():
    global SETUP_STAGE, DIAGNOSTIC
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    DIAGNOSTIC = tempfile.TemporaryFile()
    check(os.environ.get('HOME') == '/home/fixture'
          and Path('/h2/synthetic-only').read_bytes() == b'H2_SYNTHETIC_ONLY',
          'synthetic_namespace_required')
    check(not Path('/home/sam').exists() and not Path('/run/user').exists(), 'host_visible')
    check('DISPLAY' not in os.environ and 'WAYLAND_DISPLAY' not in os.environ, 'display_inherited')
    # No service activation directories: no prompts, GUI, systemd or host sockets.
    SETUP_STAGE = 'private_bus_start'
    daemon = subprocess.Popen(['/usr/bin/dbus-daemon', '--nofork',
        '--config-file=/h2/dbus.conf', '--print-address=1'],
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    address = daemon.stdout.readline().strip().decode()
    check(address.startswith('unix:path=/run/h2/bus'), 'unexpected_private_bus')
    os.environ['DBUS_SESSION_BUS_ADDRESS'] = address
    SETUP_STAGE = 'private_bus_connect'
    bus = dbus.bus.BusConnection(address)
    server = None
    result = {'scope': 'SYNTHETIC_PRIVATE_FS_DBUS_PID_NET_NAMESPACES',
              'host_credentials_accessed': False, 'copilot_started': False,
              'personal_stronghold_accessed': False, 'sdk_invocations': 0,
              'inference_calls': 0, 'gui_shell_in_namespace': False, 'cases': [],
              'daemon_resource_samples': []}
    def sample(label):
        result['daemon_resource_samples'].append({'phase': label,
            'readiness_ms': server.h2_startup_ms,
            'rss_bytes': identity(server.pid)['rss_bytes']})
    try:
        SETUP_STAGE = 'private_keyring_start'
        server = start_keyring(bus)
        sample('empty_locked_service')
        check(not present(bus), 'unexpected_new_fixture_collection')
        try:
            unlock(bus, PASSWORD)
            raise AssertionError('missing_collection_was_not_rejected')
        except ValueError as error:
            check(str(error) == 'existing_collection_required', 'unexpected_rejection')
        check(not present(bus), 'missing_guard_created_collection')
        result['cases'].append({'case': 'guard_missing_collection', 'state': 'PASS'})
        # Explicit initial synthetic creation proves why personal --unlock needs care.
        stop(server)
        server = start_keyring(bus, PASSWORD)
        sample('explicit_synthetic_creation_on_start')
        check(present(bus) and not locked(bus), 'synthetic_create_or_unlock_failed')
        result['cases'].append({'case': 'official_cli_creates_when_missing', 'state': 'PASS_OBSERVED_CREATION'})
        backend('initialize')
        snapshot = Path('/state/vault/luna-lr3.stronghold')
        initial = hashlib.sha256(snapshot.read_bytes()).hexdigest()
        keyring_file = Path('/state/data/keyrings/login.keyring')
        initial_keyring = hashlib.sha256(keyring_file.read_bytes()).hexdigest()
        check(not Path('/state/vault/luna-lr3.unlock').exists(), 'plaintext_unlock_created')
        call(bus, SERVICE, 'org.freedesktop.Secret.Service', 'Lock', 'ao', ([dbus.ObjectPath(LOGIN)],))
        check(locked(bus), 'synthetic_lock_failed')
        try:
            unlock(bus, b'public-h2-incorrect-password')
            raise ValueError('wrong_password_accepted')
        except Exception as error:
            check(not isinstance(error, ValueError), 'unexpected_password_guard_failure')
            pass
        check(locked(bus), 'wrong_password_unlocked')
        result['cases'].append({'case': 'wrong_password_stays_locked', 'state': 'PASS'})
        unlock(bus, PASSWORD)
        check(not locked(bus), 'existing_unlock_failed')
        result['cases'].append({'case': 'gnome_private_existing_only_unlock_without_gui', 'state': 'PASS_SYNTHETIC_REAL_DAEMON'})
        backend('presence')
        sample('existing_unlock_and_backend_presence')
        check(hashlib.sha256(snapshot.read_bytes()).hexdigest() == initial, 'snapshot_changed_on_presence')
        check(hashlib.sha256(keyring_file.read_bytes()).hexdigest() == initial_keyring,
              'existing_keyring_changed_on_unlock')
        result['cases'].append({'case': 'production_rust_backend_presence_private_fixture', 'state': 'PASS_SYNTHETIC_REAL_BACKEND'})
        # Quit the exact owned foreground daemon; keep encrypted fixture data in place.
        stop(server)
        server = start_keyring(bus)
        sample('restart_existing_encrypted_store_locked')
        check(present(bus) and locked(bus), 'restart_did_not_recover_locked_existing_collection')
        unlock(bus, PASSWORD)
        check(not locked(bus), 'unlock_after_restart_failed')
        backend('presence')
        sample('restart_existing_store_unlocked_and_backend_presence')
        check(hashlib.sha256(snapshot.read_bytes()).hexdigest() == initial, 'snapshot_changed_after_restart')
        check(hashlib.sha256(keyring_file.read_bytes()).hexdigest() == initial_keyring,
              'existing_keyring_changed_after_restart_unlock')
        result['cases'].append({'case': 'daemon_restart_existing_store_and_rust_reopen', 'state': 'PASS_SYNTHETIC_REAL_BACKEND'})
        check(all(p.name in ('login.keyring', 'user.keystore', 'default')
                  for p in Path('/state/data/keyrings').iterdir()), 'unexpected_fixture_store')
        result['fixture_store_file_modes'] = {
            p.name: oct(p.stat().st_mode & 0o777) for p in Path('/state/data/keyrings').iterdir()}
        check(all(mode == '0o600' for mode in result['fixture_store_file_modes'].values()),
              'fixture_store_permissions_unsafe')
        result['snapshot_bytes_unchanged_after_presence_and_restart'] = True
        result['synthetic_login_keyring_bytes_unchanged_after_unlock_and_restart'] = True
        result['official_cli_unlock_is_atomic_existing_only'] = False
        result['existing_only_extension_is_supported_public_api'] = False
        result['manual_unlock_transport'] = 'LIBSECRET_DH_AES_NO_PLAIN_FALLBACK'
        result['personal_unlock_authorized_or_executed'] = False
        result['state'] = 'PASS_SYNTHETIC_REAL_COMPONENTS'
    except Exception as error:
        result['state'] = 'BLOCKED_SYNTHETIC_EXPERIMENT'
        result['error_code'] = (str(error) if isinstance(error, ValueError)
                                else 'synthetic_fixture_operation_failed')
    finally:
        if server is not None:
            stop(server)
        bus.close()
        stop(daemon)
    DIAGNOSTIC.seek(0)
    private_errors = DIAGNOSTIC.read()
    result['daemon_error_indicators'] = {name: fragment in private_errors for name, fragment in (
        ('store_module_missing', b"couldn't find secret store module"),
        ('credential_creation_failed', b"couldn't create login credential"),
        ('store_creation_failed', b"couldn't create login keyring"),
        ('permission_denied', b'Permission denied'),
        ('system_bus_unavailable', b'system bus'),
        ('collection_creation_notification', b'collection_Created'))}
    DIAGNOSTIC.close()
    print(json.dumps(result))
    return 0 if result['state'] == 'PASS_SYNTHETIC_REAL_COMPONENTS' else 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except Exception as error:
        # Even failure before the inner lifecycle block emits no traceback/payload.
        name = error.get_dbus_name() if isinstance(error, dbus.DBusException) else None
        code = next((n for n in ('NoReply', 'AccessDenied', 'AuthFailed', 'NoServer',
            'NotSupported', 'BadAddress', 'IOError', 'FileNotFound')
            if name == 'org.freedesktop.DBus.Error.' + n), 'setup_or_cleanup_failed')
        print(json.dumps({'state': 'BLOCKED_SYNTHETIC_EXPERIMENT',
                          'error_code': code, 'stage': SETUP_STAGE}))
        raise SystemExit(1)
