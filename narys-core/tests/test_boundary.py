"""FIX-1: real offline Core/operator IPC + hostile synthetic peers + real tools.
Scripted confirmations exercise the operator protocol; no physical human or
actual SSH transport is claimed. No provider/runtime/authentication is used.
"""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import unittest

CORE = Path(os.environ.get('NARYS_TEST_CORE', str(Path(__file__).parents[1] / 'target/debug/narys-core')))
CLI = Path(os.environ.get('NARYS_TEST_CLI', str(CORE.with_name('narys'))))

def peer(*intents):
    return '\n'.join("print(" + repr(json.dumps(i)) + ",flush=True)\nreply=json.loads(sys.stdin.buffer.readline())\nassert reply['ok']" for i in intents)

def write(path='artifact.txt', content='NARYS_OFFLINE_TEST:approved\n'):
    return dict(kind='write', path=path, content=content)

def command(program='/usr/bin/sleep', arguments=None):
    return dict(kind='command', program=program, arguments=arguments or ['30'])

class FixtureDB(sqlite3.Connection):
    def __exit__(self, *exc):
        try:
            return super().__exit__(*exc)
        finally:
            self.close()

class Boundary(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='narys-boundary-'))
        self.socket = self.root / 'operator.sock'
        self.proc = None
        self.boot()

    def boot(self):
        self.err = tempfile.TemporaryFile()
        self.proc = subprocess.Popen([str(CORE), 'boundary-serve', str(self.root)], stdout=subprocess.PIPE, stderr=self.err, env={'PATH':'/usr/bin','LANG':'C.UTF-8','NARYS_PRIVATE_MARKER':'synthetic-private-env'})
        ready, _, _ = select.select([self.proc.stdout], [], [], 8)
        if not ready:
            self.fail('Core startup timeout')
        line = self.proc.stdout.readline()
        if not line:
            self.err.seek(0); self.fail('Core failed: ' + self.err.read().decode())
        self.assertEqual(json.loads(line)['mode'], 'offline_local_boundary')

    def tearDown(self):
        if self.proc and self.proc.poll() is None:
            try:
                self.req('shutdown')
            except Exception:
                self.proc.terminate()
            try:
                self.proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.proc.kill(); self.proc.wait(timeout=3)
        if self.proc:
            self.proc.stdout.close()
        self.err.close()
        shutil.rmtree(self.root)

    def req(self, op, **kwargs):
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(8); s.connect(str(self.socket)); s.sendall(json.dumps(dict(operation=op, **kwargs)).encode()); s.shutdown(socket.SHUT_WR)
            b = bytearray()
            while True:
                part = s.recv(8192)
                if not part: break
                b.extend(part)
            return json.loads(b)

    def good(self, op, **kw):
        r = self.req(op, **kw)
        self.assertTrue(r['ok'], r)
        return r['result']

    def start(self, source, ttl=60):
        return self.good('start_synthetic_peer', source=source, ttl_seconds=ttl)

    def wait(self, fn, seconds=8):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            v = fn()
            if v: return v
            time.sleep(.01)
        self.fail('condition timeout')

    def pending(self, task):
        return self.wait(lambda: next((r for r in self.good('approvals')['approvals'] if r['task_id']==task and r['state']=='pending'), None))

    def approve(self, receipt):
        d = self.good('show', approval_id=receipt['approval_id'])['exact_preview']['digest']
        return self.good('approve', approval_id=receipt['approval_id'], digest=d)

    def terminal(self, task):
        return self.wait(lambda: (r if r['peer_cleanup_verified'] or r['state']=='cleanup_uncertain' else None) if (r:=self.good('task', task_id=task)) else None)

    def db(self):
        return sqlite3.connect(self.root / 'db/luna.sqlite3', timeout=3, factory=FixtureDB)

    def task_count(self):
        with self.db() as db:
            return db.execute('select count(*) from agent_local_tasks').fetchone()[0]

    def test_positive_cli_confirmation_write_verify_durable_and_replay(self):
        t = self.start(peer(write(), command('/usr/bin/sha256sum', ['/workspace/artifact.txt'])))
        first = self.pending(t['task_id'])
        shown = self.good('show', approval_id=first['approval_id'])
        self.assertEqual(shown['exact_preview']['effect']['content'], 'NARYS_OFFLINE_TEST:approved\n')
        self.assertIn('runner_arguments', shown['exact_preview'])
        d = shown['exact_preview']['digest']
        result = subprocess.run([str(CLI), 'boundary', str(self.socket), 'approve', first['approval_id']], input=f"approve {first['approval_id']} {d}\n", text=True, capture_output=True, timeout=8)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        second = self.pending(t['task_id']); self.approve(second)
        done = self.terminal(t['task_id'])
        self.assertEqual(done['state'], 'completed')
        self.assertEqual(len(done['executions']), 2)
        self.assertTrue(all(r['phase']=='completed' and r['cleanup_verified'] for r in done['executions']))
        data = (Path(t['workspace']) / 'artifact.txt').read_bytes()
        self.assertEqual(data, b'NARYS_OFFLINE_TEST:approved\n')
        self.assertEqual(done['executions'][0]['result']['result']['evidence']['sha256'], hashlib.sha256(data).hexdigest())
        self.assertFalse(self.req('approve', approval_id=first['approval_id'], digest=d)['ok'])
        with self.db() as db:
            self.assertEqual(db.execute('select count(*) from agent_tool_executions').fetchone()[0], 2)
            self.assertEqual(db.execute('select group_concat(phase) from agent_execution_events').fetchone()[0], 'claimed,started,completed,claimed,started,completed')
            self.assertNotIn('NARYS_OFFLINE_TEST:', db.execute('select group_concat(summary_json) from agent_approvals').fetchone()[0])
        self.assertFalse(self.good('status')['cost_authorized'])

    def test_host_operator_disconnect_reconnect_and_wrong_digest(self):
        t = self.start(peer(write())); r = self.pending(t['task_id'])
        with socket.socket(socket.AF_UNIX) as disconnected:
            disconnected.connect(str(self.socket)); disconnected.sendall(b'{'); disconnected.close()
        self.assertFalse(self.req('approve', approval_id=r['approval_id'], digest='changed')['ok'])
        self.assertEqual(self.good('show', approval_id=r['approval_id'])['receipt']['state'], 'pending')
        self.approve(r); self.assertEqual(self.terminal(t['task_id'])['state'], 'completed')

    def test_no_operator_timeout_and_deny(self):
        t = self.start(peer(write()), ttl=1); done = self.terminal(t['task_id'])
        self.assertEqual(done['executions'], []); self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())
        t = self.start(peer(write())); r=self.pending(t['task_id']); self.good('deny', approval_id=r['approval_id'])
        self.assertEqual(self.terminal(t['task_id'])['executions'], [])

    def test_cancel_pending_no_claim_no_effect(self):
        t = self.start(peer(write())); self.pending(t['task_id']); r=self.good('cancel', task_id=t['task_id'])
        self.assertTrue(r['cancel_requested']); self.assertFalse(r['cleanup_verified'])
        done=self.terminal(t['task_id']);self.assertEqual(done['state'],'cancelled');self.assertEqual(done['executions'],[])
        self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())

    def test_shutdown_sqlite_revocation_failure_still_cancels_all_live_tools(self):
        pending = self.start(peer(write()))
        self.pending(pending['task_id'])
        live = []
        for _ in range(2):
            t = self.start(peer(command()))
            self.approve(self.pending(t['task_id']))
            self.wait(lambda: any(e['phase']=='started' for e in self.good('task', task_id=t['task_id'])['executions']))
            live.append(t['task_id'])
        with self.db() as db:
            db.execute("CREATE TRIGGER fail_revoke BEFORE UPDATE ON agent_approvals WHEN OLD.state='pending' AND NEW.state='cancelled' BEGIN SELECT RAISE(ABORT,'synthetic revocation failure');END")
        started = time.monotonic()
        self.assertFalse(self.req('shutdown')['ok'])
        self.assertEqual(self.proc.wait(timeout=2), 1)
        self.assertLess(time.monotonic()-started, 2)
        with self.db() as db:
            rows = db.execute('SELECT phase,cleanup_verified FROM agent_tool_executions WHERE task_id IN (?,?)', live).fetchall()
            self.assertEqual(rows, [('cancelled',1),('cancelled',1)])
            self.assertEqual(db.execute('SELECT count(*) FROM agent_local_tasks WHERE peer_cleanup_verified=0').fetchone()[0], 0)

    def test_long_tool_cancel_is_responsive_and_other_task_progresses(self):
        t=self.start(peer(command()));r=self.pending(t['task_id']);self.approve(r)
        self.wait(lambda:any(e['phase']=='started' for e in self.good('task',task_id=t['task_id'])['executions']))
        other=self.start(peer(write('other.txt')));self.approve(self.pending(other['task_id']))
        self.assertEqual(self.terminal(other['task_id'])['state'],'completed')
        now=time.monotonic();self.good('cancel',task_id=t['task_id']);done=self.terminal(t['task_id'])
        self.assertLess(time.monotonic()-now,2)
        self.assertEqual(done['executions'][0]['phase'],'cancelled');self.assertTrue(done['executions'][0]['cleanup_verified'])
        self.assertTrue(done['peer_cleanup_verified'])

    def test_cancel_vs_completion_never_late_success_after_cancel_wins(self):
        for i in range(8):
            t=self.start(peer(command(arguments=['0'])));r=self.pending(t['task_id']);self.approve(r)
            cancel=self.req('cancel',task_id=t['task_id']);done=self.terminal(t['task_id'])
            for e in done['executions']:
                self.assertIn(e['phase'],['completed','cancelled'])
                if e['phase']=='completed':self.assertFalse(e['cancel_requested'])
                self.assertTrue(e['cleanup_verified'])
            if cancel['ok'] and not done['executions']:
                self.assertEqual(done['state'],'cancelled')

    def test_concurrent_operator_approval_single_claim(self):
        t=self.start(peer(write()));r=self.pending(t['task_id']);d=self.good('show',approval_id=r['approval_id'])['exact_preview']['digest']
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            values=list(pool.map(lambda _:self.req('approve',approval_id=r['approval_id'],digest=d),range(8)))
        self.assertEqual(sum(v['ok'] for v in values),1)
        self.assertEqual(len(self.terminal(t['task_id'])['executions']),1)

    def test_concurrent_admission_reserves_only_four_peer_slots(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            replies = list(pool.map(lambda _: self.req('start_synthetic_peer', source=peer(write()), ttl_seconds=60), range(8)))
        accepted = [r['result'] for r in replies if r['ok']]
        self.assertEqual(len(accepted), 4, replies)
        self.assertTrue(all(r['ok'] or r['error_code']=='local_concurrency_limit' for r in replies), replies)
        self.assertEqual(self.task_count(), 4)
        for t in accepted:
            self.good('cancel', task_id=t['task_id'])
            self.assertEqual(self.terminal(t['task_id'])['state'], 'cancelled')
        again = self.start(peer(write()))
        self.good('cancel', task_id=again['task_id'])
        self.assertEqual(self.terminal(again['task_id'])['state'], 'cancelled')

    def test_instance_task_limit_is_transactional_under_concurrent_admission(self):
        # Operator-side durable-state fixture avoids launching 127 processes.
        # The last admission and its actual sandboxed peer are operational.
        with self.db() as db:
            db.executemany("INSERT INTO agent_local_tasks(task_id,session_id,workspace,epoch,state,peer_cleanup_verified) VALUES(?,?,?,'fixture','completed',1)",[(i, f'fixture-{i}', str(self.root/'workspaces'/str(i))) for i in range(1000,1127)])
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            replies = list(pool.map(lambda _: self.req('start_synthetic_peer', source=peer(write()), ttl_seconds=60), range(4)))
        accepted = [r['result'] for r in replies if r['ok']]
        self.assertEqual(len(accepted), 1, replies)
        self.assertTrue(all(r['ok'] or r['error_code']=='local_instance_task_limit' for r in replies), replies)
        self.assertEqual(self.task_count(), 128)
        self.good('cancel', task_id=accepted[0]['task_id'])
        self.terminal(accepted[0]['task_id'])
        self.assertEqual(self.req('start_synthetic_peer',source=peer(write()),ttl_seconds=60)['error_code'],'local_instance_task_limit')

    def test_same_uid_malicious_peer_cannot_approve_or_escape_or_alter_workspace(self):
        outside=self.root/'private-marker'
        source=f'''import socket,os,subprocess,errno
from pathlib import Path
assert os.getuid()=={os.getuid()}
for domain in [socket.AF_INET,socket.AF_UNIX,socket.AF_VSOCK]:
 try:socket.socket(domain,socket.SOCK_STREAM);raise AssertionError('unfiltered network')
 except OSError as e:assert e.errno==errno.EPERM
assert 'Seccomp:\t2' in Path('/proc/self/status').read_text()
for action in [lambda:socket.socket(socket.AF_UNIX).connect({str(self.socket)!r}), lambda:Path({str(outside)!r}).read_bytes(),lambda:Path({str(outside)!r}).write_text('escaped'),lambda:Path('/workspace/unapproved').write_text('escaped'),lambda:Path('/proc/{self.proc.pid}/environ').read_bytes(),lambda:socket.create_connection(('127.0.0.1',9),timeout=.2)]:
 try: action(); raise AssertionError('escaped')
 except OSError: pass
assert not os.environ.get('NARYS_PRIVATE_MARKER')
assert len(os.listdir('/proc/self/fd'))<=4
assert subprocess.run(['/usr/bin/unshare','-Ur','/usr/bin/true'],capture_output=True).returncode!=0
try: Path('/workspace/alias').symlink_to({str(outside)!r});raise AssertionError('unapproved symlink')
except OSError:pass
'''+peer(write(content='NARYS_OFFLINE_TEST:negative attempts really failed\n'))
        t=self.start(source);r=self.pending(t['task_id']);self.approve(r)
        self.assertEqual(self.terminal(t['task_id'])['state'],'completed')
        self.assertEqual(outside.read_text(),'SYNTHETIC_OFFLINE_PRIVATE_MARKER')
        self.assertFalse((Path(t['workspace'])/'unapproved').exists())

    def test_unmediated_native_tools_profile_changes_and_sensitive_contents_denied(self):
        for intent in [dict(kind='bash',arguments=['touch outside']),dict(operation='approve',trusted=True),dict(kind='write',path='../escape',content='NARYS_OFFLINE_TEST:x'),dict(kind='write',path='artifact.txt',content='token=synthetic-secret'),dict(kind='command',program='/bin/sh',arguments=['-c','touch x']),dict(kind='write',path='x',content='NARYS_OFFLINE_TEST:x',profile='explicit_yolo')]:
            source="print("+repr(json.dumps(intent))+",flush=True)\nreply=json.loads(sys.stdin.buffer.readline())\nassert not reply['ok']"
            t=self.start(source);done=self.terminal(t['task_id']);self.assertEqual(done['executions'],[])
            self.assertFalse((Path(t['workspace'])/'x').exists())
        self.assertEqual(self.good('status')['native_copilot'],'BLOCKED')

    def test_symlinks_hardlinks_and_workspace_identity_changed_before_approval(self):
        for mode in ['symlink','hardlink','replacement']:
            t=self.start(peer(write()));r=self.pending(t['task_id']);shown=self.good('show',approval_id=r['approval_id']);w=Path(t['workspace'])
            if mode=='symlink':(w/'artifact.txt').symlink_to(self.root/'private-marker')
            elif mode=='hardlink':os.link(self.root/'private-marker',w/'artifact.txt')
            else:w.rename(w.with_name(w.name+'old'));w.mkdir(mode=0o700)
            self.assertFalse(self.req('approve',approval_id=r['approval_id'],digest=shown['exact_preview']['digest'])['ok'])
            self.good('cancel',task_id=t['task_id']);self.terminal(t['task_id'])
        self.assertEqual((self.root/'private-marker').read_text(),'SYNTHETIC_OFFLINE_PRIVATE_MARKER')

    def test_sqlite_failure_before_claim_is_atomic_no_effect(self):
        t=self.start(peer(write()));r=self.pending(t['task_id'])
        with self.db() as db:
            db.execute("CREATE TRIGGER fail_claim BEFORE INSERT ON agent_tool_executions BEGIN SELECT RAISE(ABORT,'synthetic before claim');END")
        self.approve(r);done=self.terminal(t['task_id']);self.assertEqual(done['executions'],[])
        self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())
        self.assertNotEqual(self.good('show',approval_id=r['approval_id'])['receipt']['state'],'consumed')

    def test_sqlite_failure_after_effect_blocks_admission_no_replay(self):
        t=self.start(peer(write()));r=self.pending(t['task_id'])
        with self.db() as db:
            db.execute("CREATE TRIGGER fail_result BEFORE UPDATE ON agent_tool_executions WHEN NEW.phase='completed' BEGIN SELECT RAISE(ABORT,'synthetic result failure');END")
        self.approve(r);done=self.terminal(t['task_id']);self.assertTrue((Path(t['workspace'])/'artifact.txt').exists())
        self.assertTrue(self.good('status')['admission_closed'])
        self.assertEqual(done['executions'][0]['phase'],'started')
        self.assertFalse(self.req('approve',approval_id=r['approval_id'],digest=r['binding_digest'])['ok'])

    def test_sqlite_failure_before_start_records_not_started(self):
        t=self.start(peer(write()));r=self.pending(t['task_id'])
        with self.db() as db:
            db.execute("CREATE TRIGGER fail_start BEFORE UPDATE ON agent_tool_executions WHEN NEW.phase='started' BEGIN SELECT RAISE(ABORT,'synthetic start failure');END")
        self.approve(r);done=self.terminal(t['task_id']);self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())
        self.assertEqual(done['executions'][0]['phase'],'failed')
        self.assertFalse(done['executions'][0]['effect_started'])
        self.assertTrue(done['executions'][0]['cleanup_verified'])

    def restart(self):
        self.proc.kill();self.proc.wait(timeout=3);self.proc.stdout.close();self.err.close();self.boot()

    def test_restart_pending_no_grants_no_auto_replay(self):
        t=self.start(peer(write()));r=self.pending(t['task_id']);self.restart()
        self.assertEqual(self.good('show',approval_id=r['approval_id'])['receipt']['state'],'interrupted')
        self.assertTrue(self.good('status')['admission_closed'])
        self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())
        self.assertFalse(self.req('approve',approval_id=r['approval_id'],digest=r['binding_digest'])['ok'])
        self.assertFalse(self.req('start_synthetic_peer',source=peer(write()),ttl_seconds=60)['ok'])

    def test_restart_approved_before_claim_has_no_replay(self):
        # SIGSTOP creates a deterministic crash window without a production
        # bypass/sleep knob. SQLite is fixture-controlled, never agent-visible.
        t=self.start(peer(write()));r=self.pending(t['task_id']);os.kill(self.proc.pid,19)
        with self.db() as db:db.execute("UPDATE agent_approvals SET state='approved' WHERE approval_id=?",[r['approval_id']])
        self.restart();self.assertEqual(self.good('show',approval_id=r['approval_id'])['receipt']['state'],'interrupted')
        self.assertFalse((Path(t['workspace'])/'artifact.txt').exists());self.assertTrue(self.good('status')['admission_closed'])

    def test_restart_claimed_before_effect_fixture_is_uncertain_no_replay(self):
        t=self.start(peer(write()));r=self.pending(t['task_id']);os.kill(self.proc.pid,19)
        # Durable crash image fixture: grant cannot be reconstructed from it.
        with self.db() as db:
            db.execute("UPDATE agent_approvals SET state='consumed' WHERE approval_id=?",[r['approval_id']])
            db.execute("INSERT INTO agent_tool_executions(approval_id,task_id,phase) VALUES(?,?,'claimed')",[r['approval_id'],t['task_id']])
        self.restart();done=self.good('task',task_id=t['task_id'])
        self.assertEqual(done['executions'][0]['phase'],'uncertain')
        self.assertFalse(done['executions'][0]['effect_started']);self.assertFalse(done['executions'][0]['cleanup_verified'])
        self.assertTrue(self.good('status')['admission_closed']);self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())

    def test_restart_after_claim_and_running_has_uncertain_result(self):
        t=self.start(peer(command()));r=self.pending(t['task_id']);self.approve(r)
        self.wait(lambda:any(e['phase']=='started' for e in self.good('task',task_id=t['task_id'])['executions']))
        self.restart();done=self.good('task',task_id=t['task_id'])
        self.assertEqual(done['executions'][0]['phase'],'uncertain');self.assertFalse(done['executions'][0]['cleanup_verified'])
        self.assertTrue(self.good('status')['admission_closed'])

    def test_pid_namespace_kills_daemonized_peer_descendants(self):
        source='''import os,time
from pathlib import Path
if os.fork()==0:
 os.setsid(); time.sleep(.8)
 try:Path('/workspace/orphan').write_text('escaped')
 except OSError:pass
 time.sleep(30)
else:
 print(json.dumps({'kind':'command','program':'/usr/bin/sleep','arguments':['30']}),flush=True)
 json.loads(sys.stdin.buffer.readline())
'''
        t=self.start(source);r=self.pending(t['task_id']);self.approve(r)
        self.wait(lambda:any(e['phase']=='started' for e in self.good('task',task_id=t['task_id'])['executions']))
        self.good('cancel',task_id=t['task_id']);done=self.terminal(t['task_id']);self.assertTrue(done['peer_cleanup_verified'])
        time.sleep(1);self.assertFalse((Path(t['workspace'])/'orphan').exists())

    def test_operator_cannot_change_arguments_or_supply_authority_fields(self):
        t=self.start(peer(write()));r=self.pending(t['task_id'])
        self.assertFalse(self.req('approve',approval_id=r['approval_id'],digest=r['binding_digest'],arguments=['changed'])['ok'])
        self.assertFalse(self.req('approve',approval_id=r['approval_id'],digest=r['binding_digest'],origin='HumanLocal')['ok'])
        self.good('deny',approval_id=r['approval_id']);self.terminal(t['task_id'])

    def test_clean_shutdown_and_restart_does_not_restore_grants(self):
        t=self.start(peer(write()));r=self.pending(t['task_id']);self.approve(r);self.terminal(t['task_id'])
        self.good('shutdown');self.assertEqual(self.proc.wait(timeout=8),0)
        self.proc.stdout.close();self.err.close();self.boot()
        self.assertFalse(self.good('status')['admission_closed'])
        self.assertEqual(self.good('show',approval_id=r['approval_id'])['receipt']['state'],'consumed')
        self.assertFalse(self.req('approve',approval_id=r['approval_id'],digest=r['binding_digest'])['ok'])

    def test_boundary_operationally_incapable_does_not_issue_approval(self):
        # A real outer sandbox disables nested user namespaces. Binary presence
        # is insufficient: the inner Core preflight must fail, not fall back.
        code=r"""
import json,socket,subprocess,time
p=subprocess.Popen(['/core','boundary-serve','/tmp/narys-boundary-nested'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
assert json.loads(p.stdout.readline())['mode']=='offline_local_boundary'
def call(operation,**kw):
 with socket.socket(socket.AF_UNIX) as s:
  s.settimeout(8);s.connect('/tmp/narys-boundary-nested/operator.sock');s.sendall(json.dumps(dict(operation=operation,**kw)).encode());s.shutdown(socket.SHUT_WR)
  data=bytearray()
  while True:
   b=s.recv(8192)
   if not b:break
   data.extend(b)
  return json.loads(data)
try:
 r=call('start_synthetic_peer',source='pass',ttl_seconds=60)
 assert not r['ok'],r
 assert call('status')['result']['admission_closed']
 assert call('approvals')['result']['approvals']==[]
 print(json.dumps({'inner_sandbox_failed':True,'positive_authority_issued':False}))
finally:
 call('shutdown');p.wait(timeout=8)
"""
        args=['/usr/bin/bwrap','--unshare-all','--unshare-user','--die-with-parent','--new-session','--cap-drop','ALL','--disable-userns','--assert-userns-disabled','--ro-bind','/usr','/usr','--symlink','usr/bin','/bin','--symlink','usr/lib64','/lib64','--symlink','usr/lib','/lib','--proc','/proc','--dev','/dev','--tmpfs','/tmp','--tmpfs','/home','--tmpfs','/run','--ro-bind',str(CORE),'/core','--clearenv','--setenv','PATH','/usr/bin','--remount-ro','/','--','/usr/bin/python3','-I','-c',code]
        r=subprocess.run(args,env={},capture_output=True,text=True,timeout=15)
        self.assertEqual(r.returncode,0,r.stderr+r.stdout)
        self.assertFalse(json.loads(r.stdout)['positive_authority_issued'])

    def test_peer_output_without_newline_is_bounded_and_exits_without_approval(self):
        t=self.start("sys.stdout.write('X'*20000);sys.stdout.flush()")
        done=self.terminal(t['task_id']);self.assertEqual(done['executions'],[])
        self.assertFalse(self.good('status')['admission_closed'])

    def test_shutdown_racing_preflight_certifies_cleanup_and_no_start_after_stop(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            job=pool.submit(self.req,'start_synthetic_peer',source='import time;time.sleep(30)',ttl_seconds=60)
            self.wait(lambda:self.task_count()>0)
            self.good('shutdown');job.result(timeout=12)
        self.assertEqual(self.proc.wait(timeout=12),0)
        with self.db() as db:
            rows=db.execute('select state,peer_cleanup_verified from agent_local_tasks').fetchall()
            self.assertTrue(rows);self.assertTrue(all(state=='cancelled' and clean for state,clean in rows),rows)
            self.assertEqual(db.execute('select count(*) from agent_tool_executions').fetchone()[0],0)

    def test_explicit_approval_cli_wrong_confirmation_has_no_effect(self):
        t=self.start(peer(write()));r=self.pending(t['task_id'])
        result=subprocess.run([str(CLI),'boundary',str(self.socket),'approve',r['approval_id']],input='yes\n',capture_output=True,text=True,timeout=8)
        self.assertNotEqual(result.returncode,0);self.assertIn('explicit_confirmation_required',result.stderr)
        self.good('deny',approval_id=r['approval_id']);self.terminal(t['task_id']);self.assertFalse((Path(t['workspace'])/'artifact.txt').exists())

if __name__ == '__main__':unittest.main(verbosity=2)
