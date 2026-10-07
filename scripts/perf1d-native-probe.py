#!/usr/bin/env python3
"""Real Tauri lifecycle + local HTTP Conversation, isolated DBus/data/credentials.
Build: cargo build --release --features perf1d-probe --manifest-path src-tauri/Cargo.toml
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
parser.add_argument('--output', default='/tmp/narys-perf1d-native.json')
parser.add_argument('--seconds', type=int, default=5)
parser.add_argument('--smoke', action='store_true', help='Functional scenario with controlled deadlines and no endurance cycles; not performance evidence')
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
evidence = {'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(), 'realWebViews':True, 'functionalOnly':args.smoke, 'softwareRendering':True, 'provider':'local HTTP Groq SSE fixture; not commercial', 'focus':'native focus handler controlled by probe; physical compositor focus not attested','snapshots':[], 'measurements':[], 'reopens':[], 'lifecycleTrees':[]}
with tempfile.TemporaryDirectory(prefix='narys-perf1d-') as temporary:
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
        evidence['snapshots'].append({'action':action,'windows':result['windows'],'mainPresent':result['mainPresent'],'registryAddress':result['registryAddress'],'providerRuntimeAddress':result['providerRuntimeAddress'],'sessionId':result['sessionId'],'taskId':result.get('task',{}).get('taskId') if result.get('task') else None,'taskState':result.get('task',{}).get('state') if result.get('task') else None,'adaptive':{k:v for k,v in result['adaptive'].items() if k!='history'},'historyCount':len(result['adaptive']['history'])})
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
    def closed(manual=False):
        if manual: command('close')
        def absent():
            s = command('snapshot')
            return s if not s['windows'] and not s['mainPresent'] else None
        result = until(absent,timeout=45)
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
    def safe_economy():
        until(lambda: frontend().get('mode')=='economy')
        return until(lambda: (s if not s['adaptive']['uiGuard'] and not s['adaptive']['transitioning'] else None) if (s:=command('snapshot')) else None)
    def auto_close(controlled=True):
        safe_economy()
        command('focused'); snap=command('background')
        assert snap['adaptive']['pendingToken'] is not None, snap
        started=time.monotonic()
        if controlled: command('expire')
        result=closed()
        evidence.setdefault('autoCloses',[]).append({'controlledClock':controlled,'elapsedMs':round((time.monotonic()-started)*1000,2)})
        assert result['adaptive']['pendingToken'] is None and not result['adaptive']['timerActive'] and not result['adaptive']['transitioning']
        return result
    def attention_reopen(action='attention'):
        started=time.monotonic(); command(action); safe_economy()
        report=frontend(); assert report['canvases']==0 and report['glb']==0
        snap=command('snapshot'); assert snap['adaptive']['attention'] is not None
        assert 'Atenção requerida' in report['body'],report
        evidence.setdefault('attentionReopens',[]).append(round((time.monotonic()-started)*1000,2))
        command('ack_attention'); until(lambda: command('snapshot')['adaptive']['attention'] is None)
        return snap
    def no_3d_since(index):
        assert not any('Luna.glb' in e['data']['path'] or 'AvatarViewport' in e['data']['path'] for e in events()[index:] if e['event']=='asset')
    try:
        initial=until(lambda: command('snapshot'),timeout=90); safe_economy()
        assert initial['adaptive']['policy']=='economy'
        command('background'); assert command('snapshot')['adaptive']['pendingToken'] is None
        command('auto'); safe_economy(); no_3d_since(0)
        # Real React draft: only empty/nonempty state crosses IPC; retained when auto is blocked.
        command('draft'); until(lambda: command('snapshot')['adaptive']['uiGuard'])
        command('focused'); command('background'); command('expire'); time.sleep(.3)
        assert command('snapshot')['mainPresent']
        assert 'Unsent local draft' not in json.dumps(command('snapshot')['adaptive'])
        command('clear_draft'); safe_economy()
        command('settings'); until(lambda: len(command('snapshot')['windows'])==2)
        command('focused'); command('background'); command('expire'); time.sleep(.3)
        assert command('snapshot')['mainPresent']; command('close_settings'); until(lambda: len(command('snapshot')['windows'])==1)
        until(lambda: sum(r['name'].startswith('WebKitWeb') for r in sampler.read_tree(app.pid))==1)
        evidence['afterSettingsCloseTree']=sampler.read_tree(app.pid)
        command('ui_bound_on'); command('focused'); command('background'); command('expire'); time.sleep(.3)
        assert command('snapshot')['mainPresent']; command('ui_bound_off')
        # Focus and preference changes invalidate stale delayed decisions.
        command('focused'); command('background'); token=command('snapshot')['adaptive']['pendingToken']
        command('focused'); assert command('snapshot')['adaptive']['pendingToken'] is None
        command('background'); command('economy'); assert command('snapshot')['adaptive']['pendingToken'] is None
        command('background'); assert command('snapshot')['adaptive']['pendingToken'] is None
        command('auto'); safe_economy()
        command('send'); until(lambda: len(requests)==1)
        active=task_state('running'); task_id=active['task']['taskId']; session_id=active['sessionId']
        # First cycle uses the actual 30-second timer; following cycles control its deadline.
        absent=auto_close(args.smoke); assert absent['task']['taskId']==task_id and absent['task']['state']=='running'
        if not args.smoke: assert 29 <= evidence['autoCloses'][-1]['elapsedMs']/1000 < 38
        restored=attention_reopen('approval_attention')
        assert restored['registryAddress']==initial['registryAddress'] and restored['providerRuntimeAddress']==initial['providerRuntimeAddress']
        until(lambda: any(e['event']=='ui_attach' and e['data']['taskId']==task_id for e in events()))
        assert len(requests)==1
        auto_close(); restored=reopened(); safe_economy(); assert restored['task']['taskId']==task_id
        assert len(requests)==1; no_3d_since(0)
        # Explicit Presence remains stable on background. Attention/reopen in Auto never chooses it.
        command('presence'); until(lambda: (r:=frontend())['mode']=='presence' and r['phase']=='ready' and r['canvases']==1)
        evidence['explicitPresence']={'frontend':{k:v for k,v in frontend().items() if k!='body'},'assets':[e['data'] for e in events() if e['event']=='asset' and ('Luna.glb' in e['data']['path'] or 'AvatarViewport' in e['data']['path'])]}
        command('background'); assert command('snapshot')['adaptive']['pendingToken'] is None
        closed(True); reopened(); until(lambda: (r:=frontend())['mode']=='presence' and r['phase']=='ready')
        command('background'); assert command('snapshot')['adaptive']['pendingToken'] is None
        command('auto'); safe_economy(); assert frontend()['canvases']==0
        absent=auto_close(); gates[0].set(); assert task_state('completed')['task']['taskId']==task_id
        # Cycle RSS compares warmed Economy and Headless separately, same persistent Core.
        baseline_headless=sampler.read_tree(app.pid)
        attention_reopen(); safe_economy(); time.sleep(2)
        baseline_economy=sampler.read_tree(app.pid)
        cycle_asset_start=len(events())
        for index in range(0 if args.smoke else 15):
            absent=auto_close()
            tree=sampler.read_tree(app.pid)
            evidence.setdefault('cycles',[]).append({'cycle':index+1,'headlessTree':tree,'adaptive':absent['adaptive']})
            if index % 2: reopened(); safe_economy()
            else: attention_reopen()
            snap=command('snapshot')
            assert len(snap['windows'])==1 and snap['registryAddress']==initial['registryAddress'] and snap['providerRuntimeAddress']==initial['providerRuntimeAddress']
            assert snap['adaptive']['pendingToken'] is None
            assert len(snap['adaptive']['history'])<=64 and len(requests)==1
            evidence['cycles'][-1]['economyTree']=sampler.read_tree(app.pid)
        no_3d_since(cycle_asset_start)
        time.sleep(2); final_economy=sampler.read_tree(app.pid)
        absent=auto_close(); final_headless=sampler.read_tree(app.pid)
        evidence['cycleMemory']={'beforeEconomy':baseline_economy,'afterEconomy':final_economy,'beforeHeadless':baseline_headless,'afterHeadless':final_headless}
        evidence['transitionRing']=absent['adaptive']['history']; assert len(evidence['transitionRing'])<=64
        # Manual Headless is a persisted preference, explicit activation only opens temporary Economy.
        command('headless'); reopened(); safe_economy(); snap=command('snapshot')
        assert snap['adaptive']['policy']=='headless'; command('background'); assert command('snapshot')['adaptive']['pendingToken'] is None
        assert frontend()['canvases']==0
        closed(True); assert command('snapshot')['adaptive']['policy']=='headless'
        reopened(); safe_economy(); command('auto'); safe_economy()
        command('send'); until(lambda: len(requests)==2); task_state('running'); auto_close()
        before_quit=sampler.read_tree(app.pid); command('quit'); app.wait(timeout=12); assert app.returncode==0
        time.sleep(1)
        live=[]
        for row in before_quit:
            path=Path(f'/proc/{row["pid"]}/stat')
            if path.exists():
                tail=path.read_text().rsplit(')',1)[1].split()
                if int(tail[19])==row['startTicks'] and tail[0]!='Z': live.append(row['pid'])
        assert not live,live
        evidence['orphanHelpers']=live; evidence['quitExitCode']=app.returncode
        evidence['providerCalls']=len(requests); evidence['autoNeverPresence']=True; evidence['pass']=True
        # Isolated cold start Headless: Core initializes without ever constructing a WebView.
        cold=directory/'cold'; cold.mkdir(); shutil.copyfile(directory/'identity.json',cold/'identity.json')
        env={**env,'XDG_DATA_HOME':str(cold/'data'),'NARYS_PERF1C_PROBE':str(cold),'NARYS_PERF1D_INITIAL_POLICY':'headless'}
        directory=cold; sequence=0; app=subprocess.Popen([str(binary)],env=env,stdout=log,stderr=log)
        cold_snap=until(lambda: command('snapshot'),timeout=90)
        assert cold_snap['windows']==[] and cold_snap['adaptive']['policy']=='headless'
        assert not any(r['name'].startswith('WebKitWeb') for r in sampler.read_tree(app.pid))
        evidence['coldHeadless']=cold_snap
        reopened(); safe_economy(); assert command('snapshot')['adaptive']['policy']=='headless'
        command('auto'); safe_economy(); command('focused'); command('background'); assert command('snapshot')['adaptive']['timerActive']
        quitting=command('quit'); assert quitting['adaptive']['pendingToken'] is None and not quitting['adaptive']['timerActive']
        app.wait(timeout=12); assert app.returncode==0
    except BaseException:
        log.flush(); log.seek(0); print(log.read()[-12000:],file=sys.stderr)
        evidence['pass']=False
        evidence['failureTree']=sampler.read_tree(app.pid)
        raise
    finally:
        for gate in gates: gate.set()
        if app.poll() is None: app.terminate(); app.wait(timeout=15)
        Path(args.output).write_text(json.dumps(evidence,indent=2,ensure_ascii=False)+'\n')
        server.shutdown()
