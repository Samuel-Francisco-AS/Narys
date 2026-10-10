"""Embedded existing-only GNOME50 adapter. Password never enters Python strings.

No imports from checkout, no autoactivation, no standard Unlock/Prompt/Create.
The unsupported method is isolated behind unique-owner, UID/executable/hash pins.
TTY/logind/interactive-shell checks are admission checks, not same-UID isolation.
"""
import ctypes
import hashlib
import grp
import json
import os
from pathlib import Path
import resource
import signal
import stat
import sys
import termios

NAME = 'org.freedesktop.secrets'
SERVICE = '/org/freedesktop/secrets'
LOGIN = SERVICE + '/collection/login'
INTERNAL = 'org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface'
DAEMON_SHA = 'c7c5ad270c98fc0c9466031a08a037d650a918786036579003d9bcca4844481e'
ALGORITHM = b'dh-ietf1024-sha256-aes128-cbc-pkcs7'

class Blocked(ValueError):
    pass

def require(condition, code):
    if not condition:
        raise Blocked(code)

def libraries():
    import gi
    gi.require_version('Secret', '1')
    from gi.repository import Gio, GLib
    return Gio, GLib

class ExistingLogin:
    def __init__(self):
        self.Gio, self.GLib = libraries()
        self.bus = self.Gio.bus_get_sync(self.Gio.BusType.SESSION, None)
        owner = self.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetNameOwner', self.GLib.Variant('(s)', (NAME,)),
            None, self.Gio.DBusCallFlags.NO_AUTO_START, 3000, None).unpack()[0]
        require(owner.startswith(':'), 'credential_service_unavailable')
        self.owner = owner
        require(self.call('org.freedesktop.Secret.Service', 'ReadAlias',
            self.GLib.Variant('(s)', ('login',))).unpack()[0] == LOGIN,
            'existing_login_required')

    def call(self, interface, method, params=None, path=SERVICE):
        return self.bus.call_sync(self.owner, path, interface, method, params,
            None, self.Gio.DBusCallFlags.NO_AUTO_START, 3000, None)

    def locked(self):
        return self.call('org.freedesktop.DBus.Properties', 'Get',
            self.GLib.Variant('(ss)', ('org.freedesktop.Secret.Collection', 'Locked')),
            LOGIN).unpack()[0]

    def verify_backend(self):
        def identity(method):
            return self.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', method, self.GLib.Variant('(s)', (self.owner,)),
                None, self.Gio.DBusCallFlags.NO_AUTO_START, 3000, None).unpack()[0]
        require(identity('GetConnectionUnixUser') == os.getuid(), 'credential_owner_invalid')
        pid = identity('GetConnectionUnixProcessID')
        entry = Path('/proc') / str(pid)
        require(entry.stat().st_uid == os.getuid() and
            (entry / 'exe').resolve() == Path('/usr/bin/gnome-keyring-daemon') and
            hashlib.sha256((entry / 'exe').read_bytes()).hexdigest() == DAEMON_SHA,
            'credential_backend_incompatible')
        groups = [row for row in (entry / 'cgroup').read_text().splitlines() if row.startswith('0:')]
        require(len(groups) == 1 and
            '/app.slice/gnome-keyring-daemon.service' in groups[0] and
            f'/user@{os.getuid()}.service/' in groups[0], 'credential_owner_not_user_service')

    def unlock(self):
        # libsecret ABI uses native secure SecretValue and encrypted D-Bus Secret.
        # ctypes handles only native pointers; Python sees ciphertext, never password.
        lib = ctypes.CDLL('libsecret-1.so.0')
        glib = ctypes.CDLL('libglib-2.0.so.0')
        obj = ctypes.CDLL('libgobject-2.0.so.0')
        lib.secret_service_get_type.restype = ctypes.c_size_t
        lib.secret_service_open_sync.argtypes = [ctypes.c_size_t, ctypes.c_char_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p]
        lib.secret_service_open_sync.restype = ctypes.c_void_p
        lib.secret_service_get_session_algorithms.argtypes = [ctypes.c_void_p]
        lib.secret_service_get_session_algorithms.restype = ctypes.c_char_p
        lib.secret_service_encode_dbus_secret.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        lib.secret_service_encode_dbus_secret.restype = ctypes.c_void_p
        lib.secret_service_get_session_dbus_path.argtypes = [ctypes.c_void_p]
        lib.secret_service_get_session_dbus_path.restype = ctypes.c_char_p
        lib.secret_value_new.argtypes = [ctypes.c_void_p, ctypes.c_ssize_t, ctypes.c_char_p]
        lib.secret_value_new.restype = ctypes.c_void_p
        lib.secret_value_unref.argtypes = [ctypes.c_void_p]
        glib.g_variant_print.argtypes = [ctypes.c_void_p, ctypes.c_int]
        glib.g_variant_print.restype = ctypes.c_void_p
        glib.g_free.argtypes = [ctypes.c_void_p]
        glib.g_variant_unref.argtypes = [ctypes.c_void_p]
        obj.g_object_unref.argtypes = [ctypes.c_void_p]
        service = lib.secret_service_open_sync(lib.secret_service_get_type(), self.owner.encode(), 2, None, None)
        require(service, 'credential_session_unavailable')
        try:
            require(lib.secret_service_get_session_algorithms(service) == ALGORITHM,
                'encrypted_credential_session_required')
            self.verify_backend()
            if self.locked() is False:
                return 'already_unlocked'
            with HumanPassword() as password:
                value = lib.secret_value_new(password.address, password.length, b'text/plain')
                require(value, 'credential_encoding_failed')
                try:
                    encoded = lib.secret_service_encode_dbus_secret(service, value)
                finally:
                    lib.secret_value_unref(value)
                require(encoded, 'credential_encoding_failed')
                try:
                    printed = glib.g_variant_print(encoded, True)
                    require(printed, 'credential_encoding_failed')
                    try:
                        ciphertext = ctypes.string_at(printed).decode('ascii')
                    finally:
                        glib.g_free(printed)
                    variant = self.GLib.Variant.parse(None, ciphertext, None, None)
                    require(variant.get_type_string() == '(oayays)' and
                        len(variant.get_child_value(1).unpack()) == 16,
                        'encrypted_credential_payload_required')
                    # Revalidate owner, collection and service immediately before effect.
                    self.verify_backend()
                    if self.locked() is False:
                        return 'already_unlocked'
                    try:
                        self.call(INTERNAL, 'UnlockWithMasterPassword',
                            self.GLib.Variant.new_tuple(self.GLib.Variant('o', LOGIN), variant))
                    except Exception:
                        raise Blocked('credential_password_rejected_or_service_changed') from None
                finally:
                    glib.g_variant_unref(encoded)
            require(self.locked() is False, 'credential_password_rejected')
            return 'unlocked'
        finally:
            path = lib.secret_service_get_session_dbus_path(service)
            try:
                if path:
                    self.call('org.freedesktop.Secret.Session', 'Close', path=path.decode())
            finally:
                obj.g_object_unref(service)

def proc_fields(pid):
    p = Path('/proc') / str(pid)
    fields = (p / 'stat').read_text().rsplit(')', 1)[1].split()
    return p, int(fields[1])  # parent PID

def require_human_tty():
    require(all(os.isatty(fd) for fd in (0, 1, 2)), 'human_private_ssh_terminal_required')
    tty = os.ttyname(0)
    require(tty.startswith('/dev/pts/') and all(os.ttyname(fd) == tty for fd in (1, 2)),
        'human_private_ssh_terminal_required')
    m = os.stat(tty)
    require(stat.S_ISCHR(m.st_mode) and m.st_uid == os.getuid() and
        m.st_mode & 0o047 == 0 and
        (not m.st_mode & 0o020 or grp.getgrgid(m.st_gid).gr_name == 'tty'),
        'terminal_permissions_invalid')  # Unix tty group may write; cannot read password
    require(os.tcgetpgrp(0) == os.getpgrp(), 'terminal_foreground_required')
    # Direct official CLI -> interactive login shell. Reject agents, Python harness,
    # shell -c scripts, tmux/screen, and allocated PTYs without a logind SSH session.
    cli, parent = proc_fields(os.getppid())
    # Official CLI disables dumpability; Linux intentionally hides its exe link.
    # Check readable kernel identity metadata; same UID remains the trust boundary.
    metadata = dict(row.split(':', 1) for row in (cli / 'status').read_text().splitlines() if ':' in row)
    require(metadata.get('Name', '').strip() in ('narys', 'narys-core') and
        [int(v) for v in metadata.get('Uid', '').split()] == [os.getuid()] * 4,
        'official_human_cli_required')
    shell, _ = proc_fields(parent)
    require((shell / 'exe').resolve() in [Path('/usr/bin/bash'), Path('/usr/bin/zsh'), Path('/usr/bin/fish'), Path('/usr/bin/dash')],
        'interactive_ssh_shell_required')
    # Shell invocation metadata only; never inspect arbitrary process argv/env.
    argv = (shell / 'cmdline').read_bytes().split(b'\0')
    require(len([v for v in argv if v]) == 1 or argv[1:] == [b'-l', b''],
        'interactive_ssh_shell_required')
    Gio, GLib = libraries()
    system = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    flags = Gio.DBusCallFlags.NO_AUTO_START
    path = system.call_sync('org.freedesktop.login1', '/org/freedesktop/login1',
        'org.freedesktop.login1.Manager', 'GetSessionByPID', GLib.Variant('(u)', (os.getpid(),)),
        None, flags, 3000, None).unpack()[0]
    props = system.call_sync('org.freedesktop.login1', path, 'org.freedesktop.DBus.Properties',
        'GetAll', GLib.Variant('(s)', ('org.freedesktop.login1.Session',)),
        None, flags, 3000, None).unpack()[0]
    require(props.get('Remote') is True and props.get('Service') == 'sshd' and
        props.get('Type') == 'tty' and props.get('Class') == 'user' and
        props.get('User', (None,))[0] == os.getuid() and
        props.get('TTY') == tty.removeprefix('/dev/'), 'authenticated_ssh_session_required')

class HumanPassword:
    def __init__(self):
        self.buffer = bytearray(8193)
        self.native = (ctypes.c_ubyte * len(self.buffer)).from_buffer(self.buffer)
        self.address = ctypes.addressof(self.native)
        self.length = 0
        self.libc = ctypes.CDLL(None, use_errno=True)
        self.libc.mlock.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        self.libc.munlock.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        self.libc.read.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_size_t]
        self.libc.read.restype = ctypes.c_ssize_t

    def __enter__(self):
        require_human_tty()
        require(self.libc.mlock(self.address, len(self.buffer)) == 0, 'credential_secure_memory_unavailable')
        original = termios.tcgetattr(0)
        hidden = original.copy()
        hidden[3] &= ~(termios.ECHO | termios.ECHONL)
        hidden[3] |= termios.ICANON
        try:
            termios.tcsetattr(0, termios.TCSAFLUSH, hidden)
            os.write(2, b'Senha do login keyring (SSH privado, sem eco): ')
            # SIGINT/HUP/TERM raise; finally restores termios and wipes native bytes.
            for i in range(8193):
                n = self.libc.read(0, self.address + i, 1)
                require(n == 1, 'credential_input_interrupted')
                c = self.buffer[i]
                if c == 10:
                    self.buffer[i] = 0
                    self.length = i
                    require(i > 0, 'credential_input_invalid')
                    return self
                require(c != 0 and i < 8192, 'credential_input_invalid')
            raise Blocked('credential_input_invalid')
        except BaseException:
            self.__exit__(None, None, None)
            raise
        finally:
            termios.tcsetattr(0, termios.TCSAFLUSH, original)
            os.write(2, b'\n')

    def __exit__(self, *unused):
        ctypes.memset(self.address, 0, len(self.buffer))
        self.libc.munlock(self.address, len(self.buffer))

def main():
    operation = sys.argv[1] if len(sys.argv) == 2 else ''
    require(operation in ('status', 'unlock', 'unlock-json'), 'fixed_credential_operation_required')
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    require(ctypes.CDLL(None).prctl(4, 0, 0, 0, 0) == 0, 'credential_dump_protection_required')
    def interrupted(signum, frame):
        raise Blocked('credential_input_interrupted')
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, interrupted)
    if operation != 'status':
        require_human_tty()  # before bus access, even if already unlocked
    client = ExistingLogin()
    locked = client.locked()
    if operation == 'status':
        try:
            client.verify_backend()
            compatible, code = True, None
        except Blocked as e:
            compatible, code = False, str(e)
        print(json.dumps({'service_available': True, 'login_unlocked': not locked,
            'state': 'locked' if locked else 'unlocked',
            'unlock_backend_compatible': compatible, 'unlock_error_code': code}))
        return
    client.verify_backend()
    result = 'already_unlocked' if not locked else client.unlock()
    data = {'state': result, 'login_unlocked': True, 'password_requested': result == 'unlocked'}
    print(json.dumps({'version': 1, 'ok': True, 'data': data}) if operation == 'unlock-json'
        else ('Cofre já desbloqueado; nenhuma senha solicitada.' if result == 'already_unlocked' else 'Cofre desbloqueado; servidor independente da conexão SSH.'))

if __name__ == '__main__':
    try:
        main()
    except BaseException as error:
        code = str(error) if isinstance(error, Blocked) else 'credential_service_or_session_unavailable'
        if len(sys.argv) == 2 and sys.argv[1] == 'status':
            print(json.dumps({'service_available': False, 'login_unlocked': False, 'state': 'unavailable', 'code': code}))
        else:
            print(json.dumps({'version': 1, 'ok': False, 'error_code': code, 'category': 'credentials'}))
        raise SystemExit(1)
