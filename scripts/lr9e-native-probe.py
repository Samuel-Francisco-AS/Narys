#!/usr/bin/env python3
"""LR-9E real Tauri/production React+xterm/Channels, Close/Reopen/Quit.
Fixed native fixture; isolated DBus/vault/HOME, no commercial provider or external network.
CPU percent of one core, summed tree RSS and available PSS; short local samples.
"""
import argparse, hashlib, importlib.util, json, os, shutil, subprocess, sys, tempfile, time, http.server, threading
from pathlib import Path
sys.dont_write_bytecode = True
if not os.getenv('NARYS_LR9E_PROBE_BUS'):
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,__file__,*sys.argv[1:]],{**os.environ,'NARYS_LR9E_PROBE_BUS':'1'})
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',default='src-tauri/target/debug/assistente-3d')
parser.add_argument('--output',default='/tmp/narys-lr9e-native.json')
parser.add_argument('--sample-seconds',type=int,default=10)
parser.add_argument('--warmup-seconds',type=int,default=10)
parser.add_argument('--profile',choices=['debug','release'],default='debug')
args=parser.parse_args();binary=Path(args.binary).resolve()
spec=importlib.util.spec_from_file_location('sampler',Path(__file__).with_name('perf1a-process-baseline.py'));sampler=importlib.util.module_from_spec(spec);spec.loader.exec_module(sampler)
evidence={'pass':False,'realTauri':True,'realProductionReactXterm':True,'realChannels':True,'commercialProviderInvocationRequested':False,'localFakeProviderInvocationRequested':True,'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'snapshots':[],'frontend':[],'performance':[]}
requests=[];gates=[];hold_summaries=False
class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*_):pass
    def do_POST(self):
        payload=json.loads(self.rfile.read(int(self.headers['Content-Length'])));summary='session_summary' in payload['messages'][-1]['content'];gate=threading.Event()
        private_absent=all(m not in json.dumps(payload) for m in ['PTY-ONLY-PRIVATE-MARKER','ENVIRONMENT-PRIVATE-MARKER','WORKER-INTERNAL-PRIVATE-MARKER']);assert private_absent
        record={'summary':summary,'model':payload['model'],'ptyEnvironmentWorkerMarkersAbsent':private_absent,'responseCompleted':False,'clientDisconnected':False};requests.append(record);gates.append(gate)
        if summary and not hold_summaries:gate.set()
        self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers()
        try:
            text=json.dumps({'title':'Local summary','summary':'Metadata fixture'}) if summary else 'NATIVE-ALLOWED-CONVERSATION'
            self.wfile.write(('data: '+json.dumps({'choices':[{'delta':{'content':text}}]})+'\n\n').encode());self.wfile.flush()
            while not gate.wait(.1):self.wfile.write(b': heartbeat\n\n');self.wfile.flush()
            self.wfile.write(b'data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":4,"total_tokens":12}}\n\ndata: [DONE]\n\n');self.wfile.flush();record['responseCompleted']=True
        except (BrokenPipeError,ConnectionResetError):record['clientDisconnected']=True
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
evidence['provider']='local HTTP SSE + in-memory Scheduler fakes; no commercial provider'
evidence['profile']=args.profile+'/probe + embedded production frontend'
evidence['physicalSampling']={'sampleSeconds':args.sample_seconds,'idleWarmupSeconds':args.warmup_seconds,'precision':'short diagnostic local samples; one-core CPU; tree RSS/PSS; not scientific peak measurements'}
with tempfile.TemporaryDirectory(prefix='narys-lr9e-native-') as temporary:
    directory=Path(temporary);shutil.copyfile(Path(__file__).with_name('fixtures')/'perf1c-identity.json',directory/'identity.json')
    env={**os.environ,'HOME':str(directory),'SHELL':'/bin/sh','XDG_DATA_HOME':str(directory/'data'),'NARYS_PERF1C_PROBE':str(directory),'NARYS_PERF1C_ENDPOINT':f'http://127.0.0.1:{server.server_port}/chat','LIBGL_ALWAYS_SOFTWARE':'1','LR9E_PRIVATE_ENV':'ENVIRONMENT-PRIVATE-MARKER'}
    log=open(directory/'native.log','w+');app=subprocess.Popen([str(binary)],env=env,stdout=log,stderr=log);sequence=0
    def until(check,timeout=30):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            result=check()
            if result:return result
            if app.poll() is not None:raise RuntimeError('app exited')
            time.sleep(.1)
        raise TimeoutError('condition not reached')
    def command(action):
        global sequence
        sequence+=1;(directory/'request.tmp').write_text(json.dumps({'sequence':sequence,'action':action}));(directory/'request.tmp').replace(directory/'request.json')
        def read():
            try:
                v=json.loads((directory/'response.json').read_text());return v if v['sequence']==sequence else None
            except (OSError,json.JSONDecodeError):return None
        v=until(read);assert v['error'] is None,v;evidence['snapshots'].append({'action':action,**v});return v
    def state():return command('lr9c_snapshot')['terminal']
    def frontend():
        def reports():
            try:return [json.loads(line)['data'] for line in (directory/'events.jsonl').read_text().splitlines() if json.loads(line)['event']=='ui_report']
            except OSError:return []
        previous=len(reports());command('lr9c_frontend');report=until(lambda:reports()[-1] if len(reports())>previous else None);evidence['frontend'].append(report);return report
    def webprocesses():return [r['pid'] for r in sampler.read_tree(app.pid) if r['name'].startswith('WebKitWeb')]
    def pss():
        total=0;observed=0
        for row in sampler.read_tree(app.pid):
            try:
                values=(Path('/proc')/str(row['pid'])/'smaps_rollup').read_text().splitlines();total+=int(next(v.split()[1] for v in values if v.startswith('Pss:')));observed+=1
            except (OSError,StopIteration):pass
        return {'pssMiB':round(total/1024,2),'pssObservedProcesses':observed}
    def measure(scenario,trigger=None):
        if trigger is None:time.sleep(args.warmup_seconds)
        samples=[(time.monotonic(),sampler.read_tree(app.pid))];physical=[pss()];runtime=[final()]
        if trigger:trigger()
        for _ in range(args.sample_seconds):
            time.sleep(1);samples.append((time.monotonic(),sampler.read_tree(app.pid)));physical.append(pss());runtime.append(final())
        ticks=0;churn=set()
        for prev,curr in zip(samples,samples[1:]):
            old={(r['pid'],r['startTicks']):r for r in prev[1]};new={(r['pid'],r['startTicks']):r for r in curr[1]};churn|=old.keys()^new.keys();ticks+=sum(max(0,r['cpuTicks']-old[k]['cpuTicks']) for k,r in new.items() if k in old)
        duration=samples[-1][0]-samples[0][0]
        m={'scenario':scenario,'seconds':round(duration,3),'cpuPercentOneCore':round(100*ticks/os.sysconf('SC_CLK_TCK')/duration,3),'rssMinMiB':round(min(sum(r['rssKiB'] for r in rows) for _,rows in samples)/1024,2),'rssMaxMiB':round(max(sum(r['rssKiB'] for r in rows) for _,rows in samples)/1024,2),'processChurn':sorted(churn)};m['pssSamples']=physical;m['runtimeSamples']=runtime;evidence['performance'].append(m);print(json.dumps({k:v for k,v in m.items() if k not in ['runtimeSamples','pssSamples']}),flush=True)
    def final():return command('lr9e_snapshot')['finalGate']
    def new_conversation():
        # Summary enqueue precedes the React controller's async close completion.
        # Waiting only for HTTP incorrectly races send against busy.current.
        until(lambda:'Luna está escrevendo' not in frontend()['body'])
        command('new')
        until(lambda:'Native headless fixture' not in frontend()['body'] and 'NATIVE-ALLOWED-CONVERSATION' not in frontend()['body'])
    def work():command('lr9e_workload')
    def wait_work():until(lambda:not final()['workloadActive'])
    def cycle():
        command('lr9c_conversation');until(lambda:frontend().get('view')=='conversation');until(lambda:state()['bridgeWorkers']==0)
        command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado')
    try:
        until(lambda:command('snapshot')['mainPresent'],90);until(lambda:frontend().get('view')=='conversation');assert not any('TerminalWorkspace' in r for r in frontend()['resources']);assert state()['status'] is None
        if args.profile=='release':
            command('lr9e_release_security')
            def denied_report():
                reports=[json.loads(l)['data'] for l in (directory/'events.jsonl').read_text().splitlines() if json.loads(l)['event']=='ui_report'];return next((r for r in reports if r.get('gate')=='LR9E_RELEASE_IPC'),None)
            report=until(denied_report);assert len(report['results'])==9 and all(r['denied'] for r in report['results']),report;evidence['releaseInvokeDenied']=report
        measure('Conversation idle; Terminal absent')
        command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');assert state()['status'] is None
        measure('Terminal + Activity idle; no eager shell')
        command('lr9c_start');until(lambda:state()['status'] and state()['status']['state']=='running');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado')
        sid=state()['status']['sessionId'];pid=state()['session']['pid'];command('lr9c_input');until(lambda:'INPUT_AFTER_1' in state()['session']['tail'])
        command('lr9e_pty_marker');until(lambda:'PTY-ONLY-PRIVATE-MARKER' in state()['session']['tail']);assert final()['hygienePrivateMarkerAbsent']
        measure('Terminal human PTY idle')
        # Start a real product Conversation through its UI; keep SSE in flight.
        command('lr9c_conversation');until(lambda:frontend().get('view')=='conversation');assert 'PTY-ONLY-PRIVATE-MARKER' not in frontend()['body'];command('send');until(lambda:len(requests)==1);command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado')
        measure('Representative cognitive streaming; Activity expanded',work);wait_work()
        sources=final()['sources'];assert all(x in sources for x in ['Core','Scheduler','CognitiveProvider','Worker','SpecialistAgent','TaskGraph']),sources
        visible_sources=frontend()['traceSources'];assert all(any(s.startswith(x+':') for s in visible_sources) for x in ['core','scheduler','cognitive_provider','worker','specialist_agent','task_graph']),visible_sources;evidence['activitySourcesReceived']=visible_sources
        gates[0].set();until(lambda:command('snapshot')['activeCount']==0)
        measure('Trace burst; Activity expanded',lambda:command('lr9c_trace'))
        command('lr9e_collapse');until(lambda:frontend()['rows']==0 and final()['trace']['activeSubscribers']==0);assert state()['session']['pid']==pid;evidence['collapsedTraceSubscribers']=0
        measure('Representative cognitive streaming; Activity collapsed',work);wait_work()
        measure('Trace burst; Activity collapsed',lambda:command('lr9c_trace'))
        command('lr9c_conversation');until(lambda:state()['bridgeWorkers']==0)
        measure('Representative cognitive streaming; Terminal absent',work);wait_work()
        measure('Trace burst; Terminal absent',lambda:command('lr9c_trace'))
        command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado')
        measure('PTY burst >6MiB',lambda:command('lr9c_burst'));until(lambda:'VISUAL_ALIVE' in state()['session']['tail']);assert state()['session']['totalBytes']>6*1024*1024
        command('lr9c_input');until(lambda:'INPUT_AFTER_2' in state()['session']['tail']);command('lr9c_resize');command('lr9c_input');until(lambda:'INPUT_AFTER_3' in state()['session']['tail'])
        # Start a fresh product stream: calibration must not outlive provider deadlines.
        command('lr9c_conversation');until(lambda:frontend().get('view')=='conversation');prior=len(requests);new_conversation();until(lambda:len(requests)>prior and requests[-1]['summary']);prior=len(requests);command('send');until(lambda:len(requests)>prior and not requests[-1]['summary']);headless_gate=gates[-1];command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado')
        # Fixed native Structured Exec fixture: no generic WebView command.
        command('lr9b_start');command('lr9b_burst');until(lambda:command('lr9b_snapshot')['execution']['runs']['floodState']=='Completed')
        execution=command('lr9b_snapshot')['execution'];assert execution['runs']['floodResult']['reaped'];assert execution['runs']['floodResult']['stdoutTotal']==6*1024*1024
        def combined():work();command('lr9c_burst');command('lr9c_trace')
        measure('Combined cognitive + trace + PTY + Structured Exec',combined);wait_work()
        work();command('lr9c_burst');command('lr9c_trace');retired_web=webprocesses();command('close');until(lambda:not command('snapshot')['windows'] and not webprocesses() and all(not Path(f'/proc/{p}').exists() for p in retired_web));until(lambda:state()['bridgeWorkers']==0)
        evidence['retiredWebKitWebProcessPids']=retired_web
        assert state()['session']['pid']==pid;before=final()['chunks'];until(lambda:final()['chunks']>before)
        command('lr9c_headless_burst');until(lambda:'HEADLESS_ALIVE' in state()['session']['tail']);assert final()['trace']['retainedEvents']<=1024 and final()['trace']['retainedBytes']<=2097152
        measure('Headless combined: PTY, product Conversation, trace and Exec alive')
        evidence['headlessWebKitWebProcesses']=webprocesses();evidence['headlessBridgeWorkers']=state()['bridgeWorkers'];evidence['headlessCoreContinued']=True
        second=subprocess.run([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20);assert second.returncode==0;until(lambda:command('snapshot')['mainPresent']);until(lambda:frontend().get('view')=='conversation');assert not any('TerminalWorkspace' in r for r in frontend()['resources'])
        command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');assert state()['session']['pid']==pid and state()['status']['sessionId']==sid
        command('lr9c_input');until(lambda:'INPUT_AFTER_4' in state()['session']['tail']);gap_report=until(lambda:(lambda r:r if r.get('ptyGap') and r.get('traceGap') else None)(frontend()));assert 'chunks de PTY anteriores indisponíveis' in gap_report['ptyGap'];assert 'eventos anteriores indisponíveis no replay' in gap_report['traceGap'];evidence['replayGapNotices']={k:gap_report[k] for k in ['ptyGap','traceGap']};evidence['reattachSameSessionPid']=True
        command('lr9c_resize');command('lr9c_input');until(lambda:'INPUT_AFTER_5' in state()['session']['tail'])
        wait_work();before_cancel=final()['chunks'];work();until(lambda:final()['chunks']>before_cancel);command('lr9e_cancel');until(lambda:not final()['workloadActive']);assert command('snapshot')['activeCount']>=1;evidence['cancelDuringTrace']=True;evidence['unrelatedConversationSurvivedCancel']=True
        headless_gate.set();until(lambda:command('snapshot')['activeCount']==0)
        command('lr9c_conversation');until(lambda:frontend().get('view')=='conversation');prior=len(requests);new_conversation();until(lambda:len(requests)>prior and requests[-1]['summary'])
        # Finish Summary before repetition; Quit will claim another below.
        for gate in gates:gate.set()
        until(lambda:command('snapshot')['activeCount']==0)
        evidence['repetitionMemory']=[]
        for _ in range(10):
            cycle();evidence['repetitionMemory'].append({'rssMiB':round(sum(r['rssKiB'] for r in sampler.read_tree(app.pid))/1024,2),**pss(),'runtime':final()})
        evidence['attachDetachCycles']=10;assert state()['session']['pid']==pid
        command('lr9c_exit');until(lambda:state()['status']['reaped']);assert not Path(f'/proc/{pid}').exists()
        assert command('lr9b_snapshot')['execution']['runs']['sleeperState']=='Running';evidence['ptyCleanupPreservedExec']=True
        until(lambda:frontend().get('terminal',{}).get('terminalState') in ['completed','failed','cancelled']);command('lr9c_start');until(lambda:state()['status']['state']=='running');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado')
        quit_pid=state()['session']['pid']
        command('lr9c_conversation');until(lambda:frontend().get('view')=='conversation');prior=len(requests);command('send');until(lambda:len(requests)>prior and not requests[-1]['summary']);gates[-1].set();until(lambda:command('snapshot')['activeCount']==0);hold_summaries=True;prior=len(requests);new_conversation();until(lambda:len(requests)>prior and requests[-1]['summary']);prior=len(requests);command('send');until(lambda:len(requests)>prior and not requests[-1]['summary'])
        command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');work();command('lr9c_burst');command('lr9c_trace')
        before_quit=command('lr9e_snapshot');evidence['quitRaceBefore']=before_quit;assert before_quit['activeCount']>=2;assert state()['status']['state']=='running'
        execution=command('lr9b_snapshot')['execution'];assert execution['runs']['sleeperState']=='Running',execution
        evidence['quitLocalHttpInFlight']=[{'index':i,'summary':r['summary']} for i,(r,g) in enumerate(zip(requests,gates)) if not g.is_set() and not r['clientDisconnected'] and not r['responseCompleted']];assert {r['summary'] for r in evidence['quitLocalHttpInFlight']}=={True,False}
        evidence['quitStructuredExecRunning']=True;managed=[quit_pid,execution['runs']['ptyPid'],execution['runs']['sleeperPid']]
        evidence['hygienePrivateMarkerAbsent']=final()['hygienePrivateMarkerAbsent'];assert evidence['hygienePrivateMarkerAbsent']
        started=time.monotonic();command('quit');app.wait(timeout=12);assert app.returncode==0;assert all(not Path(f'/proc/{p}').exists() for p in managed)
        evidence['quitElapsedSeconds']=round(time.monotonic()-started,3);evidence['quitExitCode']=app.returncode;evidence['remainingExecutionPids']=[];evidence['localHttpRequests']=requests
        shutdown=[json.loads(line)['data'] for line in (directory/'events.jsonl').read_text().splitlines() if json.loads(line)['event']=='lr9e_shutdown'];assert len(shutdown)==1;shutdown=shutdown[0]
        assert shutdown['taskActive']==0 and shutdown['taskWorkers']==0 and shutdown['summaryStopped'];assert shutdown['brokerActive']==0 and shutdown['brokerWorkers']==0 and shutdown['surfaceWorkers']==0 and shutdown['trace']['activeSubscribers']==0,shutdown
        evidence['shutdown']=shutdown;evidence['pass']=True
        print(json.dumps({k:v for k,v in evidence.items() if k not in ['snapshots','frontend','performance','quitRaceBefore']}),flush=True)
    except BaseException as error:
        evidence['failure']=str(error)
        log.flush();print((directory/'native.log').read_text()[-10000:],file=sys.stderr);raise
    finally:
        if app.poll() is None:
            try:command('quit');app.wait(timeout=12)
            except BaseException:app.terminate();app.wait(timeout=15)
        evidence['localHttpRequests']=requests
        Path(args.output).write_text(json.dumps(evidence,indent=2,ensure_ascii=False)+'\n')
        shutil.copyfile(directory/'native.log',str(args.output)+'.log')
