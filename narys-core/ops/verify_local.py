#!/usr/bin/python3
"""Sanitized operational evidence; no private config/keyring/item contents."""
import json, os, subprocess, time
from pathlib import Path
root=Path(__file__).resolve().parents[1]
home=Path.home()
def command(args):
    r=subprocess.run(args,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=10)
    return {'exit_code':r.returncode,'lines':r.stdout.decode(errors='replace').strip().splitlines()}
def metadata(p):
    try:
        m=p.lstat();return {'exists':True,'uid':m.st_uid,'mode':oct(m.st_mode&0o777),'inode':m.st_ino,'size':m.st_size,'mtime_ns':m.st_mtime_ns,'ctime_ns':m.st_ctime_ns}
    except FileNotFoundError:return {'exists':False}
def collect():
    binary=home/'.local/lib/narys/narys-core'
    probes={}
    # These control calls cannot start Copilot or return key values.
    for op in ('status','credentials'):
        r=command([str(binary),op]);
        try: probes[op]=json.loads('\n'.join(r['lines']))
        except ValueError:probes[op]={'exit_code':r['exit_code']}
    return {'observed_at_unix':time.time(),'core':probes,
        'core_service':command(['/usr/bin/systemctl','--user','show','narys-core.service','-p','ActiveState','-p','SubState','-p','MainPID','-p','MemoryCurrent','-p','TasksCurrent','-p','UnitFileState']),
        'keyring_service':command(['/usr/bin/systemctl','--user','show','gnome-keyring-daemon.service','-p','ActiveState','-p','MainPID','-p','UnitFileState']),
        'gdm':command(['/usr/bin/systemctl','is-active','gdm.service']),
        'gnome_shell':command(['/usr/bin/pgrep','-x','gnome-shell']),
        'boot_target':command(['/usr/bin/systemctl','get-default']),
        'linger':command(['/usr/bin/loginctl','show-user',str(os.getuid()),'-p','Linger']),
        'login_keyring_metadata':metadata(home/'.local/share/keyrings/login.keyring'),
        'stronghold_metadata':metadata(home/'.local/share/br.com.assistente3d.app/luna-lr3.stronghold'),
        'a9_marker_present':os.path.lexists(home/'.local/state/narys/lr10a-a9-host-attempt.json')}
if __name__=='__main__':print(json.dumps(collect(),indent=2))
