#!/usr/bin/python3
"""Reviewed update of only our installed Core. Existing hashes are mandatory."""
import hashlib, os, shutil, subprocess, sys, time
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
subprocess.run(['/usr/bin/systemctl', '--user', 'start', 'narys-core.service'], check=True)
for _ in range(50):
    if (Path(os.environ['XDG_RUNTIME_DIR'])/'narys-core/control.sock').exists(): break
    time.sleep(0.1)
else: raise SystemExit('CORE_READINESS_NOT_CONFIRMED')
print('CORE_UPDATED; Keyring/GDM/boot/credentials unchanged')
