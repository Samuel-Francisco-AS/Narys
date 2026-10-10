#!/usr/bin/python3
"""Read-only, sanitized SERVER-1A evidence. No credential/inference APIs."""
import datetime, hashlib, json, os, sqlite3, stat, subprocess
from pathlib import Path
from zoneinfo import ZoneInfo
HOME = Path.home()
STATE = HOME / '.local/state/narys/core'
DB = STATE / 'db/luna.sqlite3'
DESKTOP = HOME / '.local/share/br.com.assistente3d.app/luna.sqlite3'
START = datetime.datetime.fromisoformat('2026-10-10T15:03:12-03:00')
def connect(path):
    return sqlite3.connect(f'file:{path}?mode=ro', uri=True)
def table_data(db, table):
    # Hash only. Neither business data nor authorization/secret payloads are printed.
    rows = db.execute('SELECT * FROM "'+table.replace('"','""')+'"').fetchall()
    encoded = [json.dumps(row, ensure_ascii=False, default=lambda b: b.hex(), separators=(',', ':')) for row in rows]
    return {'count': len(rows), 'sha256': hashlib.sha256('\n'.join(sorted(encoded)).encode()).hexdigest()}
def inventory(path):
    with connect(path) as db:
        tables = [r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
        return {'schema':db.execute('PRAGMA user_version').fetchone()[0], 'integrity':db.execute('PRAGMA quick_check').fetchone()[0], 'tables':{t:table_data(db,t) for t in tables}}
def metadata(path):
    try:
        m = path.lstat()
        return {'inode':m.st_ino, 'size':m.st_size, 'mode':oct(stat.S_IMODE(m.st_mode)), 'mtime_ns':m.st_mtime_ns, 'ctime_ns':m.st_ctime_ns}
    except FileNotFoundError: return {'exists':False}
def command(*args):
    p=subprocess.run(args,capture_output=True,text=True,timeout=20,check=True)
    return p.stdout.strip()
def main():
    now=datetime.datetime.now(ZoneInfo('America/Recife'))
    authority=inventory(DB)
    source=inventory(DESKTOP)
    db=connect(DB)
    receipt=db.execute("SELECT backup_directory FROM server_migrations WHERE name='server-1a-desktop-takeover'").fetchone()
    migration=None
    if receipt:
        backup=Path(receipt[0]); desktop_backup=inventory(backup/'desktop.sqlite3')
        comparison={t:authority['tables'].get(t)==data for t,data in desktop_backup['tables'].items()}
        legacy_backup=inventory(backup/'core.sqlite3') if (backup/'core.sqlite3').exists() else None
        legacy_preserved=legacy_backup is None or authority['tables']['headless_tasks']==legacy_backup['tables']['headless_tasks']
        migration={'backup_directory':str(backup), 'desktop_tables_equal':comparison, 'all_desktop_tables_equal':all(comparison.values()), 'legacy_task_ids_results_equal':legacy_preserved, 'desktop_source_equal_backup':source==desktop_backup, 'fence_present':DESKTOP.with_suffix('.sqlite3.core-owned').exists()}
    pid=int(command('/usr/bin/systemctl','--user','show','narys-core.service','-p','MainPID','--value'))
    proc=Path('/proc')/str(pid)
    rss=next(line for line in (proc/'status').read_text().splitlines() if line.startswith('VmRSS:'))
    socket=Path(os.environ['XDG_RUNTIME_DIR'])/'narys-core/control.sock'
    statuses={op:json.loads(command(str(HOME/'.local/lib/narys/narys-core'),op)) for op in ['status','capabilities','events']}
    executables=[]
    for item in Path('/proc').iterdir():
        if not item.name.isdigit():continue
        try:
            if item.stat().st_uid==os.getuid():executables.append((item/'exe').resolve().name)
        except (PermissionError,FileNotFoundError):pass
    return {'observed_at':now.isoformat(), 'start':START.isoformat(), 'deadline':(START+datetime.timedelta(hours=48)).isoformat(), 'remaining_seconds':int((START+datetime.timedelta(hours=48)-now).total_seconds()), 'authority':authority, 'desktop':source, 'migration':migration, 'service':{'pid':pid,'state':command('/usr/bin/systemctl','--user','is-active','narys-core.service'),'linger':command('/usr/bin/loginctl','show-user',str(os.getuid()),'-p','Linger','--value'),'default_target':command('/usr/bin/systemctl','get-default'),'rss':rss,'gui_environment_present':statuses['status']['data']['graphical_environment_present'],'gnome_shell_count':executables.count('gnome-shell'),'copilot_count':executables.count('copilot'),'socket_mode':metadata(socket)['mode'],'linked_gui_libraries':[l for l in command('/usr/bin/ldd',str(HOME/'.local/lib/narys/narys-core')).splitlines() if any(x in l.lower() for x in ['gtk','webkit','x11','wayland'])]}, 'protocol':statuses,'vault_metadata':metadata(HOME/'.local/share/br.com.assistente3d.app/luna-lr3.stronghold'),'closed_authorization_metadata':metadata(STATE/'lr10a-final-authorization/closed.json')}
if __name__=='__main__':print(json.dumps(main(),ensure_ascii=False,indent=2))
