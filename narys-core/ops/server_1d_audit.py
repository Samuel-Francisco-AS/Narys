#!/usr/bin/python3
"""Read-only SERVER-1D audit; hashes private content, never unlocks or infers.

Use --backup once before the human reboot; the online backup stays outside Git.
An optional --baseline private SQLite backup proves original rows survived additions.
"""
import argparse
import datetime
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import socket
import sqlite3
import stat
import struct
import subprocess
import tempfile
from zoneinfo import ZoneInfo

HOME = Path.home()
STATE = HOME / '.local/state/narys/core'
DB = STATE / 'db/luna.sqlite3'
CLI = HOME / '.local/bin/narys'
ROOT = Path(__file__).resolve().parents[2]
DEADLINE = datetime.datetime.fromisoformat('2026-10-12T15:03:12-03:00')

def run(*args):
    p = subprocess.run(args, capture_output=True, text=True, timeout=30)
    return {'exit_code': p.returncode, 'output': p.stdout.strip()}

def call(*args):
    p = subprocess.run([str(CLI), *args, '--json'], capture_output=True,
                       timeout=30, cwd='/tmp', env={'PATH': '/usr/bin:/bin'})
    value = json.loads(p.stdout)
    if p.returncode or value.get('ok') is not True:
        raise RuntimeError(value.get('error_code', 'audit_cli_failed'))
    return value['data']

def digest(value):
    encoded = json.dumps(value, sort_keys=True, ensure_ascii=False,
                         default=lambda b: b.hex(), separators=(',', ':')).encode()
    return hashlib.sha256(encoded).hexdigest()

def metadata(path):
    m = path.lstat()
    value = dict(inode=m.st_ino, size=m.st_size, mtime_ns=m.st_mtime_ns,
                 ctime_ns=m.st_ctime_ns, mode=m.st_mode, uid=m.st_uid,
                 links=m.st_nlink)
    if stat.S_ISREG(m.st_mode):
        with path.open('rb') as stream:
            value['sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
    return value

def connect(path):
    return sqlite3.connect(f'file:{path}?mode=ro', uri=True)

def page_all(command, key, cursor_key, *args):
    rows, cursor = [], 0
    while True:
        page = call(command, *args, '--after', str(cursor), '--limit', '7')
        rows.extend(page[key])
        if not page['has_more']:
            return rows
        next_cursor = page[cursor_key]
        if next_cursor <= cursor:
            raise RuntimeError('audit_cursor_not_advancing')
        cursor = next_cursor

def database_report(baseline=None):
    with connect(DB) as db:
        db.execute('BEGIN')
        tables = [r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
        counts = {t: db.execute('SELECT count(*) FROM "'+t+'"').fetchone()[0] for t in tables}
        sessions = page_all('sessions', 'sessions', 'next_session')
        assert len(sessions) == counts['conversation_sessions']
        total = 0
        for session in sessions:
            sid = session['session_id']
            messages = page_all('session', 'messages', 'next_message', str(sid))
            rows = [dict(zip(('id', 'role', 'content', 'created_at'), r)) for r in db.execute('SELECT id,role,content,created_at FROM conversation_messages WHERE session_id=? ORDER BY id', [sid])]
            assert digest(rows) == digest(messages), 'history_changed_during_audit'
            total += len(messages)
        tasks = page_all('tasks', 'tasks', 'next_task')
        assert len(tasks) == db.execute('SELECT count(*) FROM (SELECT task_id FROM conversation_runs UNION SELECT task_id FROM task_records)').fetchone()[0]
        runs = []
        for tid, sid, state, result in db.execute('SELECT task_id,session_id,state,result_json FROM conversation_runs ORDER BY task_id'):
            task = call('task', str(tid))
            assert task['state'] == state
            assert digest(task.get('result')) == digest(json.loads(result) if result else None)
            runs.append(dict(task_id=tid, session_id=sid, state=state,
                             result_sha256=hashlib.sha256((result or '').encode()).hexdigest(),
                             result_equals_authority=True))
        events = []
        # Events have a separate 1..128 contract, unlike other pages.
        cursor, complete = 0, True
        while True:
            page = call('events', '--after', str(cursor), '--limit', '7')
            events.extend(page['events']); complete &= page['complete']
            if not page['has_more']:
                break
            assert page['next_sequence'] > cursor
            cursor = page['next_sequence']
        event_rows = [dict(sequence=r[0], namespace=r[1], task_id=r[2], code=r[3],
                           created_at=r[4], details=json.loads(r[5]) if r[5] else None)
                      for r in db.execute('SELECT sequence,namespace,task_id,code,created_at,details_json FROM server_events ORDER BY sequence')]
        assert digest(events) == digest(event_rows), 'events_changed_during_audit'
        preservation = {}
        if baseline:
            with connect(baseline) as old:
                for table in ('conversation_sessions', 'conversation_messages', 'task_records', 'task_subtask_records', 'conversation_runs', 'headless_tasks', 'identity_snapshots', 'memory_records'):
                    if table not in tables:
                        continue
                    columns = [r[1] for r in old.execute('PRAGMA table_info("'+table+'")')]
                    names = ','.join('"'+c+'"' for c in columns)
                    prior = old.execute('SELECT '+names+' FROM "'+table+'"').fetchall()
                    current = set(db.execute('SELECT '+names+' FROM "'+table+'"').fetchall())
                    preservation[table] = {'prior_count': len(prior), 'all_original_rows_equal': all(r in current for r in prior)}
        return dict(schema=db.execute('PRAGMA user_version').fetchone()[0],
                    integrity=db.execute('PRAGMA quick_check').fetchone()[0],
                    foreign_key_errors=len(db.execute('PRAGMA foreign_key_check').fetchall()),
                    counts=counts, sessions_verified=len(sessions), messages_verified_exact=total,
                    product_tasks_listed=len(tasks), runs=runs, baseline_rows=preservation,
                    events=dict(count=len(events), complete=complete, next_sequence=page['next_sequence'],
                                ordered_unique=all(a['sequence'] < b['sequence'] for a,b in zip(events,events[1:])),
                                all_pages_equal_authority=True))

def process_report():
    selected = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if proc.stat().st_uid != os.getuid():
                continue
            name = (proc / 'comm').read_text().strip()
            if name in ('narys-core', 'gnome-shell', 'copilot', 'gnome-keyring-d', 'Xorg', 'Xwayland'):
                fields = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
                selected.append(dict(pid=int(proc.name), name=name, state=fields[0], ppid=int(fields[1]), cgroup=(proc/'cgroup').read_text().strip()))
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            pass
    return selected

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backup', action='store_true')
    parser.add_argument('--baseline', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    backup = None
    if args.backup:
        directory = Path(tempfile.mkdtemp(prefix='server-1d-', dir=STATE/'backups'))
        backup = directory / 'authority.sqlite3'
        with connect(DB) as source, sqlite3.connect(backup) as destination:
            source.backup(destination)
            assert destination.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
        with backup.open('rb') as stream:
            os.fsync(stream.fileno())
        fd = os.open(directory, os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    now = datetime.datetime.now(ZoneInfo('America/Recife'))
    pid = int(run('systemctl', '--user', 'show', 'narys-core.service', '-p', 'MainPID', '--value')['output'])
    runtime = Path('/run/user') / str(os.getuid())
    sock = runtime / 'narys-core/control.sock'
    with socket.socket(socket.AF_UNIX) as peer:
        peer.connect(str(sock))
        peer_pid, peer_uid, peer_gid = struct.unpack('3i', peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
    proc = Path('/proc')/str(pid)
    inodes = set()
    fd_visibility = True
    try:
        descriptors = list((proc/'fd').iterdir())
    except PermissionError:
        descriptors = []
        fd_visibility = False  # Core deliberately sets PR_SET_DUMPABLE=0.
    for fd in descriptors:
        try:
            target = os.readlink(fd)
            if target.startswith('socket:['):
                inodes.add(target[8:-1])
        except FileNotFoundError:
            pass
    tcp_listeners, host_tcp_listeners = [], []
    for family in ('tcp', 'tcp6'):
        for line in (Path('/proc/net')/family).read_text().splitlines()[1:]:
            columns = line.split()
            if columns[3] == '0A':
                address, port = columns[1].split(':')
                words = [int(address[i:i+8],16) for i in range(0,len(address),8)]
                packed = struct.pack('='+str(len(words))+'I',*words)
                decoded = socket.inet_ntop(socket.AF_INET if family=='tcp' else socket.AF_INET6,packed)
                host_tcp_listeners.append(dict(family=family, local_address_hex=columns[1],
                                               local_address=decoded, port=int(port,16), uid=int(columns[7])))
            if columns[9] in inodes and columns[3] == '0A':
                tcp_listeners.append(family)
    cg = Path('/sys/fs/cgroup')/(proc/'cgroup').read_text().strip().split('::',1)[1].lstrip('/')
    memory = {name: (cg/name).read_text().strip() for name in ('memory.events', 'memory.current', 'memory.peak', 'memory.pressure') if (cg/name).exists()}
    old = json.loads((ROOT/'docs/evidence/server-1c/host-before.json').read_text())
    snapshot = metadata(HOME/'.local/share/br.com.assistente3d.app/luna-lr3.stronghold')
    snapshot_equal = all(snapshot[k] == v for k,v in old['snapshot'].items())
    installed = json.loads((ROOT/'docs/evidence/server-1c/installed-artifacts.json').read_text())
    paths = {'cli': CLI, 'core': HOME/'.local/lib/narys/narys-core', 'unit': HOME/'.config/systemd/user/narys-core.service'}
    artifacts = {k: dict(path=str(p), **metadata(p)) for k,p in paths.items()}
    journal = subprocess.run(['journalctl','--user','-u','narys-core.service','-b','--output=cat','--no-pager'],capture_output=True,text=True,timeout=30)
    lines = journal.stdout.splitlines()
    fixed = ('narys_core_ready protocol_v1 local_same_uid zero_tools', 'narys_core_stopped')
    report = dict(observed_at=now.isoformat(), deadline=DEADLINE.isoformat(), remaining_seconds=int((DEADLINE-now).total_seconds()),
                  boot_id=Path('/proc/sys/kernel/random/boot_id').read_text().strip(), uptime_seconds=float(Path('/proc/uptime').read_text().split()[0]),
                  fedora=Path('/etc/os-release').read_text(), kernel=run('uname','-r')['output'],
                  default_target=run('systemctl','get-default'), multi_user_active=run('systemctl','is-active','multi-user.target'), gdm=run('systemctl','is-active','gdm'), sshd=run('systemctl','is-active','sshd'),
                  linger=run('loginctl','show-user',str(os.getuid()),'-p','Linger'),
                  service=run('systemctl','--user','show','narys-core.service','-p','ActiveState','-p','SubState','-p','MainPID','-p','Result','-p','NRestarts','-p','Type','-p','ExecMainStartTimestamp','-p','CPUUsageNSec'),
                  enabled=run('systemctl','--user','is-enabled','narys-core.service'),
                  keyring_service=run('systemctl','--user','show','gnome-keyring-daemon.service','-p','ActiveState','-p','MainPID'),
                  status=call('status'), doctor=call('doctor'), credentials=call('credentials','status'),
                  socket=dict(mode=oct(stat.S_IMODE(sock.stat().st_mode)), directory_mode=oct(stat.S_IMODE(sock.parent.stat().st_mode)),peer_pid=peer_pid,peer_uid=peer_uid,uid_matches=peer_uid==os.getuid(),pid_matches=peer_pid==pid),
                  core_tcp_listeners=tcp_listeners if fd_visibility else None,
                  proc_fd_visible=fd_visibility, host_tcp_listeners=host_tcp_listeners,
                  same_uid_nonloopback_tcp_listeners=[s for s in host_tcp_listeners if s['uid']==os.getuid() and not ipaddress.ip_address(s['local_address']).is_loopback],
                  tcp_attribution_limit=None if fd_visibility else 'core_nondumpable_fd_access_denied; compare_host_listeners_and_review_unix_only_bind',
                  processes=process_report(),
                  memory=memory, host_memory={line.split(':')[0]:line.split(':')[1].strip() for line in Path('/proc/meminfo').read_text().splitlines() if line.startswith(('MemTotal:', 'MemAvailable:', 'SwapTotal:', 'SwapFree:'))},
                  host_memory_pressure=Path('/proc/pressure/memory').read_text().strip(),
                  database=database_report(args.baseline), stronghold_snapshot=snapshot,
                  stronghold_unchanged_since_1c=snapshot_equal,
                  closed_lr10a_authorization=metadata(STATE/'lr10a-final-authorization/closed.json'),
                  artifacts=artifacts, installed_hashes_equal_1c=all(artifacts[k]['sha256']==installed[k+'_sha256'] for k in paths),
                  journal=dict(exit_code=journal.returncode, total_lines=len(lines),ready_count=lines.count(fixed[0]),stopped_count=lines.count(fixed[1]),unexported_lines=sum(line not in fixed for line in lines)),
                  backup=dict(path=str(backup), **metadata(backup)) if backup else None,
                  remote_inference=False, unlock_invoked=False,
                  ssh_termux_human_gate='requires_separate_operator_evidence')
    print(json.dumps(report, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    main()
