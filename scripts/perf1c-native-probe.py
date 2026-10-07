#!/usr/bin/env python3
"""Real Tauri lifecycle + local HTTP Conversation, isolated DBus/data/credentials.
Build: cargo build --release --features perf1c-probe --manifest-path src-tauri/Cargo.toml
No fake WebView; this fixture is not a commercial-provider gate.
"""
import argparse
import hashlib
import http.server
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time

if not os.getenv('NARYS_PROBE_BUS'):
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, __file__, *sys.argv[1:]], {**os.environ, 'NARYS_PROBE_BUS': '1'})
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', default='src-tauri/target/release/assistente-3d')
parser.add_argument('--output', default='/tmp/narys-perf1c-native.json')
parser.add_argument('--seconds', type=int, default=30)
args = parser.parse_args()
binary = Path(args.binary).resolve()
requests = []
gates = []
class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *_): pass
    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        is_summary = 'session_summary' in payload['messages'][-1]['content']
        requests.append({'model':payload['model'], 'messageCount':len(payload['messages']), 'summary':is_summary})
        gate = threading.Event(); gates.append(gate)
        self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Connection','close'); self.end_headers()
        try:
            answer = json.dumps({'title':'Local summary','summary':'Summary completed without WebView.'}) if is_summary else 'Local fixture answer'
            self.wfile.write(('data: '+json.dumps({'choices':[{'delta':{'content':answer}}]})+'\n\n').encode()); self.wfile.flush()
            while not gate.wait(1):
                self.wfile.write(b': heartbeat\n\n'); self.wfile.flush()
            self.wfile.write(b'data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":4,"total_tokens":12}}\n\ndata: [DONE]\n\n'); self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError): pass
server = http.server.ThreadingHTTPServer(('127.0.0.1',0), Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
spec = importlib.util.spec_from_file_location('sampler', Path(__file__).with_name('perf1a-process-baseline.py'))
sampler = importlib.util.module_from_spec(spec); spec.loader.exec_module(sampler)
evidence = {'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(), 'realWebViews':True, 'softwareRendering':True, 'provider':'local HTTP Groq SSE fixture; not commercial', 'focus':'not attested','snapshots':[], 'measurements':[], 'reopens':[], 'lifecycleTrees':[]}
with tempfile.TemporaryDirectory(prefix='narys-perf1c-') as temporary:
    directory = Path(temporary)
    shutil.copyfile(Path(__file__).with_name('fixtures')/'perf1c-identity.json', directory/'identity.json')
    env = {**os.environ,'XDG_DATA_HOME':str(directory/'data'),'NARYS_PERF1C_PROBE':str(directory),'NARYS_PERF1C_ENDPOINT':f'http://127.0.0.1:{server.server_port}/chat','LIBGL_ALWAYS_SOFTWARE':'1'}
    log = open(directory/'native.log','w+')
    app = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    sequence = 0
    def until(check, timeout=30):
        end = time.monotonic()+timeout
        while time.monotonic()<end:
            result = check()
            if result: return result
            if app.poll() is not None: raise RuntimeError('native exited unexpectedly')
            time.sleep(.1)
        raise TimeoutError('condition not reached')
    def command(action):
        global sequence
        sequence += 1
        pending = directory/'request.tmp'; pending.write_text(json.dumps({'sequence':sequence,'action':action})); pending.replace(directory/'request.json')
        def read():
            try:
                value = json.loads((directory/'response.json').read_text())
                if value['sequence']==sequence: return value
            except (OSError,json.JSONDecodeError): pass
        result = until(read)
        assert result['error'] is None, result
        evidence['snapshots'].append({'action':action,**result})
        return result
    def events():
        try: return [json.loads(line) for line in (directory/'events.jsonl').read_text().splitlines(keepends=True) if line.endswith('\n')]
        except OSError: return []
    def frontend():
        before = len([e for e in events() if e['event']=='ui_report'])
        command('frontend')
        until(lambda: len([e for e in events() if e['event']=='ui_report'])>before)
        return [e['data'] for e in events() if e['event']=='ui_report'][-1]
    def reopened():
        started = time.monotonic()
        before = len([e for e in events() if e['event']=='ui_get_current_interaction'])
        assets_before = len([e for e in events() if e['event']=='asset'])
        second = subprocess.run([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
        assert second.returncode==0
        snapshot = until(lambda: (s if s['mainPresent'] and len(s['windows'])==1 else None) if (s:=command('snapshot')) else None)
        until(lambda: len([e for e in events() if e['event']=='ui_get_current_interaction'])>before)
        until(lambda: frontend().get('mode') in ('economy','presence'))
        evidence['reopens'].append({'elapsedMs':round((time.monotonic()-started)*1000,2), 'snapshot':snapshot,'assetIndex':assets_before,'assets': [e['data'] for e in events() if e['event']=='asset'][assets_before:]})
        tree = sampler.read_tree(app.pid)
        assert sum(row['name'].startswith('WebKitWeb') for row in tree)==1, tree
        assert sum(row['name'].startswith('WebKitNetwork') for row in tree)<=1, tree
        evidence['lifecycleTrees'].append({'phase':'reopened','tree':tree})
        return snapshot
    def closed():
        command('close')
        def absent():
            s = command('snapshot')
            return s if not s['windows'] and not s['mainPresent'] else None
        result = until(absent)
        until(lambda: not any(row['name'].startswith('WebKitWeb') for row in sampler.read_tree(app.pid)))
        tree = sampler.read_tree(app.pid)
        assert sum(row['name'].startswith('WebKitNetwork') for row in tree)<=1, tree
        evidence['lifecycleTrees'].append({'phase':'closed','tree':tree})
        return result
    def task_state(state):
        def found():
            s = command('snapshot')
            return s if s.get('task') and s['task']['state']==state else None
        return until(found)
    def measure(mode, round):
        time.sleep(10)
        output = subprocess.check_output([sys.executable,str(Path(__file__).with_name('perf1a-process-baseline.py')),'--pid',str(app.pid),'--scenario',f'perf1c-{mode}-{round}','--seconds',str(args.seconds)],text=True)
        rows = [json.loads(line) for line in output.splitlines()]
        snap = command('snapshot')
        if mode=='headless':
            assert not snap['mainPresent'] and not snap['windows']
            assert not any(row['name'].startswith('WebKitWeb') for row in sampler.read_tree(app.pid))
        evidence['measurements'].append({'mode':mode,'round':round,'data':rows,'native':snap})
        print(json.dumps({'mode':mode,'round':round,'summary':rows[-1]}),flush=True)
    try:
        initial = until(lambda: command('snapshot'),timeout=90)
        until(lambda: any(e['event']=='ui_get_current_interaction' for e in events()))
        until(lambda: frontend().get('mode')=='economy')
        report = frontend(); assert report['canvases']==0 and report['glb']==0,report
        measure('economy',1)
        command('send'); until(lambda: len(requests)==1)
        active = task_state('running'); task_id=active['task']['taskId']; session_id=active['sessionId']
        absent = closed(); assert absent['task']['taskId']==task_id and absent['task']['state']=='running'
        time.sleep(3); assert task_state('running')['task']['taskId']==task_id
        restored = reopened(); assert restored['registryAddress']==initial['registryAddress'] and restored['providerRuntimeAddress']==initial['providerRuntimeAddress']
        until(lambda: any(e['event']=='ui_attach' and e['data']['taskId']==task_id and e['data']['sessionId']==session_id for e in events()))
        report=frontend(); assert 'Resposta em andamento' in report['body'] and report['canvases']==0,report
        assert len(requests)==1
        closed(); gates[0].set(); completed=task_state('completed'); assert completed['task']['taskId']==task_id
        measure('headless',1)
        # Alternate idle measurement order in the same persistent native runtime.
        measure('headless',2)
        reopened(); report=frontend(); assert 'Local fixture answer' in report['body'],report
        measure('economy',2)
        for mode in ['presence','economy','economy']:
            command(mode); closed(); restored=reopened()
            until(lambda: frontend().get('mode')==mode)
            report=frontend()
            if mode=='presence':
                until(lambda: (r:=frontend())['canvases']==1 and r.get('phase')=='ready')
                evidence['reopens'][-1]['assets'] = [e['data'] for e in events() if e['event']=='asset'][evidence['reopens'][-1]['assetIndex']:]
                assert any('Luna.glb' in a['path'] and a['status']==200 for a in evidence['reopens'][-1]['assets'])
            else:
                assert report['canvases']==0 and report['glb']==0,report
                assert not any('Luna.glb' in a['path'] or 'AvatarViewport' in a['path'] for a in evidence['reopens'][-1]['assets'])
            assert len(restored['windows'])==1 and restored['registryAddress']==initial['registryAddress']
        command('send'); until(lambda: len(requests)==2); active=task_state('running'); second_id=active['task']['taskId']
        closed(); command('cancel'); assert task_state('cancelled')['task']['taskId']==second_id
        reopened(); command('new'); until(lambda: len(requests)==3 and requests[2]['summary']); closed()
        database = next((directory/'data').rglob('luna.sqlite3'))
        def summary_done():
            with sqlite3.connect(database) as conn:
                return conn.execute('SELECT summary_status,summary FROM conversation_sessions WHERE id=?',(session_id,)).fetchone()
        assert summary_done()[0]=='running'
        gates[2].set(); until(lambda: summary_done()[0]=='completed')
        evidence['summaryWithoutWebView']=summary_done(); assert not command('snapshot')['mainPresent']
        reopened(); command('send'); until(lambda: len(requests)==4); quit_task=task_state('running')['task']['taskId']; closed()
        before_quit = sampler.read_tree(app.pid)
        command('quit'); app.wait(timeout=12); assert app.returncode==0
        time.sleep(2)
        with sqlite3.connect(database) as conn:
            quit_state = conn.execute('SELECT state FROM task_records WHERE task_id=?',(quit_task,)).fetchone()
        assert quit_state == ('cancelled',), quit_state
        evidence['quitTaskState']=quit_state[0]
        def live_helpers():
            live=[]
            for row in before_quit:
                file=Path(f'/proc/{row["pid"]}/stat')
                if file.exists():
                    try:
                        tail=file.read_text().rsplit(')',1)[1].split()
                        if int(tail[19])==row['startTicks'] and tail[0]!='Z': live.append(row['pid'])
                    except OSError: pass
            return live
        deadline=time.monotonic()+10
        while live_helpers() and time.monotonic()<deadline: time.sleep(.1)
        assert not live_helpers(), live_helpers()
        evidence['orphanHelpers']=[]
        evidence['quitExitCode']=app.returncode; evidence['providerCalls']=len(requests); evidence['events']=events(); evidence['pass']=True
    except BaseException:
        log.flush(); print((directory/'native.log').read_text()[-12000:],file=sys.stderr)
        evidence['events']=events(); evidence['pass']=False
        raise
    finally:
        for gate in gates: gate.set()
        if app.poll() is None: app.terminate(); app.wait(timeout=15)
        Path(args.output).write_text(json.dumps(evidence,indent=2,ensure_ascii=False)+'\n')
        server.shutdown()
