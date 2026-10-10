"""CLI behavior with an isolated same-UID IPC fixture; no remote inference."""
import json
import os
from pathlib import Path
import pty
import select
import socket
import subprocess
import tempfile
import threading
import time
import unittest
CLI = Path(__file__).resolve().parents[2] / 'src-tauri/target/debug/narys'
class Fixture:
    def __init__(self, mode='normal'):
        self.temp=tempfile.TemporaryDirectory(prefix='narys-cli-fixture-')
        self.runtime=Path(self.temp.name); (self.runtime/'narys-core').mkdir(mode=0o700)
        self.sock=socket.socket(socket.AF_UNIX); self.sock.bind(str(self.runtime/'narys-core/control.sock'))
        self.sock.listen(); self.sock.settimeout(.2); self.closed=False; self.calls=[]; self.mode=mode
        self.thread=threading.Thread(target=self.run);self.thread.start()
    def run(self):
        while not self.closed:
            try: conn,_=self.sock.accept()
            except socket.timeout: continue
            with conn:
                raw=b''
                while True:
                    part=conn.recv(16384)
                    if not part:break
                    raw+=part
                r=json.loads(raw); c=r['command'];self.calls.append(c)
                op=c['operation']
                data={'credentials':{'login_unlocked':True},'sessions':{'sessions':[], 'has_more':False},
                    'session-create':{'session_id':42},'session-resume':{'session_id':42},
                    'conversation':{'task_id':73,'namespace':'product','durable':True},
                    'task-get':{'task_id':73,'state':'completed','result':{'text':'fixture-response\x1b[31m'}}}.get(op,{})
                reply={'version':1,'request_id':r['request_id'],'ok':True,'data':data}
                if self.mode=='correlation': reply['request_id']='wrong'
                if self.mode=='version': reply['version']=2
                payload=json.dumps(reply).encode()
                if self.mode=='oversize':payload=b'x'*(256*1024+1)
                if self.mode=='uncertain':payload=b''
                try:conn.sendall(payload)
                except (BrokenPipeError,ConnectionResetError):pass
    def env(self):return {'XDG_RUNTIME_DIR':str(self.runtime),'PATH':'/usr/bin:/bin'}
    def close(self):
        self.closed=True;self.thread.join(2);self.sock.close();self.temp.cleanup()
class CliTests(unittest.TestCase):
    def test_chat_select_create_send_follow_and_exit(self):
        fixture=Fixture(); master,slave=pty.openpty()
        p=subprocess.Popen([str(CLI),'chat'],stdin=slave,stdout=slave,stderr=slave,env=fixture.env())
        output=b''
        def until(marker):
            nonlocal output
            end=time.monotonic()+5
            while marker not in output:
                if time.monotonic()>end:raise AssertionError('fixture interactive deadline')
                if select.select([master],[],[],.1)[0]:output+=os.read(master,65536)
        try:
            until(b'pr\xc3\xb3xima p\xc3\xa1gina):');os.write(master,b'new\n');until(b'Voc');os.write(master,b'synthetic-chat-input\n')
            until(b'fixture-response');os.write(master,b'/exit\n');self.assertEqual(p.wait(timeout=5),0)
            self.assertNotIn(b'\x1b',output)
            self.assertEqual(len([c for c in fixture.calls if c['operation']=='conversation']),1)
            self.assertTrue(any(c['operation']=='task-get' for c in fixture.calls))
        finally:
            if p.poll() is None:p.kill();p.wait()
            os.close(master);os.close(slave);fixture.close()
    def test_version_correlation_response_limits_and_uncertain_send_never_replay(self):
        for mode,code in [('version','response_correlation_mismatch'),('correlation','response_correlation_mismatch'),('oversize','response_limit'),('uncertain','response_invalid')]:
            fixture=Fixture(mode)
            try:
                p=subprocess.run([str(CLI),'session','new','--json'],env=fixture.env(),capture_output=True,timeout=5)
                self.assertNotEqual(p.returncode,0)
                self.assertEqual(json.loads(p.stdout)['error_code'],code)
                self.assertEqual(len(fixture.calls),1)
            finally:fixture.close()
    def test_send_stdin_bound_and_private_runtime(self):
        fixture=Fixture()
        try:
            p=subprocess.run([str(CLI),'send','42','--json'],input=b'x'*4097,env=fixture.env(),capture_output=True,timeout=5)
            self.assertEqual(json.loads(p.stdout)['error_code'],'input_limit')
            self.assertEqual(fixture.calls,[])
            os.chmod(fixture.runtime,0o755)
            p=subprocess.run([str(CLI),'status','--json'],env=fixture.env(),capture_output=True,timeout=5)
            self.assertFalse(json.loads(p.stdout)['ok']);self.assertEqual(fixture.calls,[])
        finally:fixture.close()
if __name__=='__main__':unittest.main()
