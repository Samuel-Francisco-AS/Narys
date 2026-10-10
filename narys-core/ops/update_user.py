#!/usr/bin/python3
"""Reviewed update of only our installed Core. Existing hashes are mandatory."""
import hashlib, json, os, shutil, subprocess, sys, time, tempfile
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
HOME = Path.home()
binary = HOME / '.local/lib/narys/narys-core'
unit = HOME / '.config/systemd/user/narys-core.service'
source = ROOT.parent / 'src-tauri/target/debug/narys-core'
def digest(path):
    m = path.lstat()
    if not path.is_file() or path.is_symlink() or m.st_uid != os.getuid() or m.st_nlink != 1:
        raise SystemExit('UNSAFE_UPDATE_PATH')
    return hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
if len(sys.argv) != 3 or digest(binary) != sys.argv[1] or digest(unit) != sys.argv[2]:
    raise SystemExit('EXISTING_INSTALLATION_NOT_REVIEWED')
dropin = HOME / '.config/systemd/user/gnome-keyring-daemon.service.d/90-narys-headless.conf'
if dropin.is_symlink() or dropin.read_bytes() != (ROOT/'ops/keyring-headless.conf').read_bytes():
    raise SystemExit('KEYRING_DROPIN_PRESERVED_UPDATE_BLOCKED')
if f'ExecStart=%h/.local/lib/narys/narys-core serve' not in unit.read_text():
    raise SystemExit('UNKNOWN_SERVICE_PRESERVED')
# Preserve reviewed binary/unit before the only service stop. Database migration
# creates its own SQLite backups/receipt; never automatically roll back new data.
updates = HOME / '.local/state/narys/core/updates'
updates.mkdir(mode=0o700, exist_ok=True)
if updates.is_symlink() or updates.stat().st_uid != os.getuid() or updates.stat().st_mode & 0o077:
    raise SystemExit('UNSAFE_UPDATE_BACKUP_DIRECTORY')
for proc in Path('/proc').iterdir():
    if not proc.name.isdigit(): continue
    try:
        if proc.stat().st_uid == os.getuid() and (proc/'exe').resolve().name in {'assistente-3d', 'assistente_3d'}:
            raise SystemExit('LEGACY_DESKTOP_ACTIVE_UPDATE_BLOCKED')
    except (FileNotFoundError, PermissionError): pass
backup = Path(tempfile.mkdtemp(prefix='server-1a-', dir=updates))
for old, name, mode in [(binary, 'narys-core', 0o700), (unit, 'narys-core.service', 0o600)]:
    shutil.copyfile(old, backup/name)
    (backup/name).chmod(mode)
    with (backup/name).open('rb') as saved: os.fsync(saved.fileno())
(backup/'reviewed-hashes.json').write_text(json.dumps({'binary':sys.argv[1], 'unit':sys.argv[2]}, indent=2)+'\n')
(backup/'reviewed-hashes.json').chmod(0o600)
with (backup/'reviewed-hashes.json').open('rb') as saved: os.fsync(saved.fileno())
fd = os.open(backup, os.O_RDONLY | os.O_DIRECTORY)
os.fsync(fd); os.close(fd)
new = binary.with_name('narys-core.new')
fd = os.open(new, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o700)
with os.fdopen(fd, 'wb') as target, source.open('rb') as src:
    shutil.copyfileobj(src, target)
    target.flush(); os.fsync(target.fileno())
subprocess.run(['/usr/bin/strip', '--strip-debug', str(new)], check=True)
subprocess.run(['/usr/bin/systemctl', '--user', 'stop', 'narys-core.service'], check=True)
if digest(binary) != sys.argv[1] or digest(unit) != sys.argv[2]:
    raise SystemExit('INSTALLATION_CHANGED_PRESERVED')
os.replace(new, binary)
unit.write_bytes((ROOT/'ops/narys-core.service').read_bytes())
subprocess.run(['/usr/bin/systemctl', '--user', 'daemon-reload'], check=True)
started = subprocess.run(['/usr/bin/systemctl', '--user', 'start', 'narys-core.service'])
if started.returncode != 0:
    raise SystemExit('CORE_UPDATE_FAILED; reviewed binary/unit and SQLite backups preserved; no automatic data rollback')
for _ in range(50):
    if (Path(os.environ['XDG_RUNTIME_DIR'])/'narys-core/control.sock').exists(): break
    time.sleep(0.1)
else: raise SystemExit('CORE_READINESS_NOT_CONFIRMED')
status = subprocess.run([str(binary), 'status'], capture_output=True, timeout=10)
try: healthy = status.returncode == 0 and json.loads(status.stdout)['data']['protocol_version'] == 1
except (ValueError, KeyError): healthy = False
if not healthy: raise SystemExit('CORE_PROTOCOL_READINESS_NOT_CONFIRMED; backups preserved')
print('CORE_UPDATED; protocol_v1_ready; reviewed binary/unit backup=' + str(backup))
