#!/usr/bin/python3
"""Install only the reviewed user units; no reboot/root/service replacement."""
import os, shutil, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
HOME=Path.home()
files={HOME/'.config/systemd/user/narys-core.service':ROOT/'ops/narys-core.service',
       HOME/'.config/systemd/user/gnome-keyring-daemon.service.d/90-narys-headless.conf':ROOT/'ops/keyring-headless.conf'}
binary=HOME/'.local/lib/narys/narys-core'
source=ROOT.parent/'src-tauri/target/debug/narys-core'
if not source.is_file(): raise SystemExit('build_required')
keyring=HOME/'.local/share/keyrings/login.keyring'
m=keyring.lstat()
if not keyring.is_file() or keyring.is_symlink() or m.st_uid!=os.getuid() or m.st_mode&0o777!=0o600: raise SystemExit('EXISTING_LOGIN_KEYRING_REQUIRED')
state=HOME/'.local/state/narys/core'
state.mkdir(parents=True,exist_ok=True,mode=0o700)
if state.is_symlink() or state.stat().st_uid!=os.getuid() or state.stat().st_mode&0o777!=0o700: raise SystemExit('UNSAFE_CORE_STATE')
for dest, src in files.items():
    dest.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    if dest.exists() and dest.read_bytes()!=src.read_bytes(): raise SystemExit('unexpected_existing_unit_preserved')
# Existing keyring daemon must not be replaced/restarted by installation.
for dest,src in files.items():
    if not dest.exists(): dest.write_bytes(src.read_bytes());dest.chmod(0o600)
binary.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
if binary.exists(): raise SystemExit('existing_binary_preserved_review_update_required')
shutil.copyfile(source,binary);binary.chmod(0o700)
subprocess.run(['/usr/bin/strip','--strip-debug',str(binary)],check=True)
for args in [('daemon-reload',),('enable','gnome-keyring-daemon.service','gnome-keyring-daemon.socket','narys-core.service'),('start','narys-core.service')]:
    subprocess.run(['/usr/bin/systemctl','--user',*args],check=True)
import time
for _ in range(50):
    if (Path(os.environ['XDG_RUNTIME_DIR'])/'narys-core/control.sock').exists():break
    time.sleep(0.1)
else:raise SystemExit('CORE_READINESS_NOT_CONFIRMED')
print('USER_CORE_INSTALLED; no keyring restart, no boot change, no inference')
