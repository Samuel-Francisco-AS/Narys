#!/usr/bin/python3
"""User-only official CLI install/update + reviewed Core update. Never sudo.

Build first. Requires exact hashes of the existing Core and unit. The existing
updater preserves backups and touches only our Core service. CLI is self-contained.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
ROOT = Path(__file__).resolve().parents[1]
HOME = Path.home()
CORE = HOME / '.local/lib/narys/narys-core'
BIN = HOME / '.local/bin'
CLI = BIN / 'narys'
MANIFEST = HOME / '.local/lib/narys/cli-installation.json'
SOURCE = ROOT.parent / 'src-tauri/target/debug/narys'
def digest(p, built=False):
    m = p.lstat()
    if not p.is_file() or p.is_symlink() or m.st_uid != os.getuid() or (not built and m.st_nlink != 1):
        raise SystemExit('unsafe_installation_file')
    return hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()
def atomic(p, content, mode):
    fd, name = tempfile.mkstemp(prefix='.narys-install-', dir=p.parent)
    try:
        with os.fdopen(fd, 'wb') as out:
            out.write(content); out.flush(); os.fsync(out.fileno())
        os.chmod(name, mode); os.replace(name, p)
        fd = os.open(p.parent, os.O_RDONLY | os.O_DIRECTORY)
        try: os.fsync(fd)
        finally: os.close(fd)
    finally:
        if os.path.exists(name): os.unlink(name)
if len(sys.argv) != 3:
    raise SystemExit('usage: install_cli.py EXISTING_CORE_SHA256 EXISTING_UNIT_SHA256')
if CLI.exists() or CLI.is_symlink():
    if not MANIFEST.exists() or digest(CLI) != json.loads(MANIFEST.read_text())['cli_sha256']:
        raise SystemExit('unknown_cli_preserved')
if MANIFEST.exists(): digest(MANIFEST)
if BIN.is_symlink(): raise SystemExit('unsafe_user_bin')
BIN.mkdir(mode=0o700, exist_ok=True)
m = BIN.stat()
if m.st_uid != os.getuid() or m.st_mode & 0o022 or BIN.resolve() != BIN:
    raise SystemExit('unsafe_user_bin')
status = subprocess.run([str(CORE), 'status'], capture_output=True, check=True, timeout=10)
data = json.loads(status.stdout)['data']
if data.get('active_task') is not None or data.get('product_active_tasks') or data.get('execution_workers') or data.get('copilot_runtime',{}).get('active_tasks') or data.get('copilot_runtime',{}).get('runtime',{}).get('leases'):
    raise SystemExit('active_work_update_blocked')
# Strip a staged CLI before any service update; preserve prior known CLI on update.
digest(SOURCE, built=True)
fd, staged = tempfile.mkstemp(prefix='.narys-build-', dir=BIN); os.close(fd)
try:
    shutil.copyfile(SOURCE, staged);os.chmod(staged,0o700)
    subprocess.run(['/usr/bin/strip','--strip-debug',staged],check=True)
    cli_content = Path(staged).read_bytes()
finally: os.unlink(staged)
subprocess.run(['/usr/bin/python3',str(ROOT/'ops/update_user.py'),*sys.argv[1:]],check=True)
if CLI.exists():
    backup=HOME/'.local/state/narys/core/updates'/('narys-cli-'+digest(CLI))
    if not backup.exists(): atomic(backup,CLI.read_bytes(),0o700)
atomic(CLI,cli_content,0o700)
atomic(MANIFEST,(json.dumps({'cli_sha256':digest(CLI),'core_sha256':digest(CORE),
    'source_revision':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
    'source_worktree_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),
    'checkout_required_for_cli':False},indent=2)+'\n').encode(),0o600)
check=subprocess.run([str(CLI),'status','--json'],capture_output=True,check=True,timeout=10)
if json.loads(check.stdout)['data']['protocol_version']!=1:raise SystemExit('cli_protocol_not_ready')
print('CLI_INSTALLED '+str(CLI)+'; Core active; no checkout required for CLI/credentials')
