#!/usr/bin/python3
"""GNOME50 experimental existing-only unlock. Human terminal only; no service start.

Uses installed libsecret encryption, but GNOME's unlock method is explicitly an
unsupported extension. Personal use requires separate reviewed human setup.
Never invoke this dispatcher from Codex or an automated password pipeline.
"""
import hashlib
import os
from pathlib import Path
import resource
import signal
import sys
import termios

import gi
gi.require_version('Secret', '1')
from gi.repository import Gio, GLib, Secret

NAME = 'org.freedesktop.secrets'
SERVICE = '/org/freedesktop/secrets'
LOGIN = SERVICE + '/collection/login'
INTERNAL = 'org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface'
ALGORITHM = 'dh-ietf1024-sha256-aes128-cbc-pkcs7'
DAEMON_SHA = 'c7c5ad270c98fc0c9466031a08a037d650a918786036579003d9bcca4844481e'


class UnlockBlocked(ValueError):
    """Only audited safe codes may be printed by the human dispatcher."""


def require(condition, code):
    if not condition:
        raise UnlockBlocked(code)


class ExistingLogin:
    """Pinned unique bus owner. Never CreateCollection, GetSecret, Store or Prompt."""
    def __init__(self):
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        owned = self.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', (NAME,)),
            GLib.VariantType.new('(b)'), Gio.DBusCallFlags.NO_AUTO_START, 3000, None).unpack()[0]
        require(owned is True, 'existing_secret_service_required')
        self.owner = self.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetNameOwner', GLib.Variant('(s)', (NAME,)),
            GLib.VariantType.new('(s)'), Gio.DBusCallFlags.NO_AUTO_START, 3000, None).unpack()[0]
        require(self.owner.startswith(':'), 'unique_owner_required')
        require(self.call('org.freedesktop.Secret.Service', 'ReadAlias',
            GLib.Variant('(s)', ('login',))).unpack()[0] == LOGIN, 'existing_login_required')
        # libsecret's supported constructor against a UNIQUE name. A unique name
        # cannot be activated, unlike get_sync on the well-known service name.
        self.service = Secret.Service.open_sync(Secret.Service.__gtype__, self.owner,
            Secret.ServiceFlags.OPEN_SESSION, None)
        self.service.set_default_timeout(3000)
        self.service.ensure_session_sync(None)
        require(self.service.get_session_algorithms() == ALGORITHM,
                'encrypted_secret_session_required')

    def call(self, interface, method, parameters=None, path=SERVICE):
        return self.bus.call_sync(self.owner, path, interface, method, parameters,
            None, Gio.DBusCallFlags.NO_AUTO_START, 3000, None)

    def locked(self):
        return self.call('org.freedesktop.DBus.Properties', 'Get',
            GLib.Variant('(ss)', ('org.freedesktop.Secret.Collection', 'Locked')),
            LOGIN).unpack()[0]

    def unlock(self, password):
        require(self.locked() is True, 'locked_existing_login_required')
        require(bool(password) and '\x00' not in password and len(password.encode()) <= 8192,
                'invalid_password_input')
        value = Secret.Value.new(password, -1, 'text/plain')
        encoded = self.service.encode_dbus_secret(value)
        require(encoded.get_type_string() == '(oayays)' and
                len(encoded.get_child_value(1).unpack()) == 16,
                'encrypted_payload_required')
        parameters = GLib.Variant.new_tuple(GLib.Variant('o', LOGIN), encoded)
        self.call(INTERNAL, 'UnlockWithMasterPassword', parameters)
        require(self.locked() is False, 'unlock_not_confirmed')

    def close(self):
        path = self.service.get_session_dbus_path()
        if path:
            self.call('org.freedesktop.Secret.Session', 'Close', path=path)


def human_password():
    require(sys.stdin.isatty() and sys.stdout.isatty(), 'separate_private_tty_required')
    # Separate streams: BufferedRandom (r+) requires a seekable file, unlike a TTY.
    with open('/dev/tty', 'r', encoding='utf-8', buffering=1) as tty, \
         open('/dev/tty', 'w', encoding='utf-8', buffering=1) as output:
        original = termios.tcgetattr(tty.fileno())
        hidden = original.copy()
        hidden[3] &= ~termios.ECHO
        try:
            termios.tcsetattr(tty.fileno(), termios.TCSAFLUSH, hidden)
            output.write('Senha do cofre login (terminal privado, sem eco): ')
            output.flush()
            password = tty.readline(8193)
        finally:
            termios.tcsetattr(tty.fileno(), termios.TCSAFLUSH, original)
            output.write('\n')
        require(password.endswith('\n'), 'password_input_incomplete')
        return password[:-1]


def require_headless_service(services, rows, role, target, gui_ps_exit):
    # ps exit1 means no match. Errors (including missing/unreadable proc) are not
    # normalized to GUI absent just because the historical helper returns False.
    require(gui_ps_exit == 1, 'gui_process_absence_not_verified')
    require(services.get('gnome-shell_running') is False and target == 'inactive'
        and bool(rows) and all(r['values'].get('Type') in ('tty', 'unspecified') for r in rows),
        'verified_gui_absence_required')
    require(role.get('state') == 'OBSERVED' and role.get('in_user_manager') is True
        and not role.get('in_graphical_login_scope'), 'credential_owner_in_user_manager_required')


def main():
    require(sys.argv == [sys.argv[0], 'unlock-existing-login'], 'fixed_human_operation_required')
    require(sys.stdin.isatty() and sys.stdout.isatty() and
            'NARYS_LR10A_OWNED_HARNESS' not in os.environ, 'human_private_terminal_required')
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    import ctypes
    libc = ctypes.CDLL(None, use_errno=True)
    require(libc.prctl(4, 0, 0, 0, 0) == 0, 'disable_process_dump_required')
    def interrupted(signum, frame):
        raise KeyboardInterrupt
    for signum in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, interrupted)
    # Metadata only for daemon identity; never inspect process argv/environ or store.
    from diagnose_a9_auth import CONTEXT
    from diagnose_a9_gui_auth import gui_services
    from h1_context import sessions, owner, properties
    context = {k: os.environ[k] for k in CONTEXT if k in os.environ}
    services = gui_services(context)
    _, rows = sessions(context)
    role = owner(context, services, rows)
    target = properties(['/usr/bin/systemctl', '--user', 'show',
        'graphical-session.target', '-p', 'ActiveState'], context,
        {'ActiveState': 'active|inactive'})['values'].get('ActiveState')
    gui_ps_exit = properties(['/usr/bin/ps', '-C', 'gnome-shell', '-o', 'pid='],
                             context, {})['exit_code']
    require_headless_service(services, rows, role, target, gui_ps_exit)
    entry = Path('/proc') / str(role['pid'])
    require(entry.stat().st_uid == os.getuid() and
        (entry / 'exe').resolve() == Path('/usr/bin/gnome-keyring-daemon') and
        hashlib.sha256((entry / 'exe').read_bytes()).hexdigest() == DAEMON_SHA,
        'gnome50_daemon_pin_required')
    client = ExistingLogin()
    try:
        # Bind the inspected PID to the unique name actually receiving the password.
        pid = client.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
            GLib.Variant('(s)', (client.owner,)), GLib.VariantType.new('(u)'),
            Gio.DBusCallFlags.NO_AUTO_START, 3000, None).unpack()[0]
        require(pid == role['pid'], 'credential_owner_changed')
        from confirm_a9_metadata import proc_identity
        require(proc_identity(entry)['start_ticks'] == role['start_ticks']
                and entry.stat().st_uid == os.getuid(), 'credential_identity_changed')
        if client.locked() is False:
            print('LOGIN_ALREADY_UNLOCKED; nenhum segredo solicitado')
            return
        password = human_password()
        try:
            client.unlock(password)
        finally:
            del password  # Python is not a secure-memory allocator; no erasure claim.
        print('LOGIN_UNLOCKED; SDK não executado')
    finally:
        client.close()


if __name__ == '__main__':
    try:
        main()
    except BaseException as error:
        code = str(error) if isinstance(error, UnlockBlocked) else 'unlock_interrupted_or_failed'
        print('BLOCKED: ' + code)
        raise SystemExit(1)
