#!/usr/bin/env python3
"""LR-9C real Tauri/production React+xterm/Channels, Close/Reopen/Quit.
Fixed native fixture; isolated DBus/vault/HOME, no providers/LLM/network.
CPU percent of one core, summed tree RSS; short diagnostic samples, not PSS.
"""
import argparse, hashlib, importlib.util, json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
sys.dont_write_bytecode = True
if not os.getenv('NARYS_LR9C_PROBE_BUS'):
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,__file__,*sys.argv[1:]],{**os.environ,'NARYS_LR9C_PROBE_BUS':'1'})
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',default='src-tauri/target/debug/assistente-3d')
parser.add_argument('--output',default='/tmp/narys-lr9c-native.json')
parser.add_argument('--sample-seconds',type=int,default=10)
args=parser.parse_args();binary=Path(args.binary).resolve()
spec=importlib.util.spec_from_file_location('sampler',Path(__file__).with_name('perf1a-process-baseline.py'));sampler=importlib.util.module_from_spec(spec);spec.loader.exec_module(sampler)
evidence={'pass':False,'realTauri':True,'realProductionReactXterm':True,'realChannels':True,'providerInvocationRequestedByHarness':False,'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'snapshots':[],'frontend':[],'performance':[]}
with tempfile.TemporaryDirectory(prefix='narys-lr9c-native-') as temporary:
    directory=Path(temporary);shutil.copyfile(Path(__file__).with_name('fixtures')/'perf1c-identity.json',directory/'identity.json')
    env={**os.environ,'HOME':str(directory),'SHELL':'/bin/sh','XDG_DATA_HOME':str(directory/'data'),'NARYS_PERF1C_PROBE':str(directory),'NARYS_PERF1C_ENDPOINT':'http://127.0.0.1:9/not-used','LIBGL_ALWAYS_SOFTWARE':'1'}
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
    def measure(scenario,trigger=None):
        if trigger is None:time.sleep(10)
        samples=[(time.monotonic(),sampler.read_tree(app.pid))]
        if trigger:trigger()
        for _ in range(args.sample_seconds):time.sleep(1);samples.append((time.monotonic(),sampler.read_tree(app.pid)))
        ticks=0;churn=set()
        for prev,curr in zip(samples,samples[1:]):
            old={(r['pid'],r['startTicks']):r for r in prev[1]};new={(r['pid'],r['startTicks']):r for r in curr[1]};churn|=old.keys()^new.keys();ticks+=sum(max(0,r['cpuTicks']-old[k]['cpuTicks']) for k,r in new.items() if k in old)
        duration=samples[-1][0]-samples[0][0]
        m={'scenario':scenario,'seconds':round(duration,3),'cpuPercentOneCore':round(100*ticks/os.sysconf('SC_CLK_TCK')/duration,3),'rssMinMiB':round(min(sum(r['rssKiB'] for r in rows) for _,rows in samples)/1024,2),'rssMaxMiB':round(max(sum(r['rssKiB'] for r in rows) for _,rows in samples)/1024,2),'processChurn':sorted(churn)};evidence['performance'].append(m);print(json.dumps(m),flush=True)
    try:
        until(lambda:command('snapshot')['mainPresent'],90);until(lambda:frontend().get('view')=='conversation');boot=frontend();assert not any('TerminalWorkspace' in r for r in boot['resources']);assert state()['status'] is None
        measure('Economy Conversation idle, Terminal not loaded')
        command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');assert 'Iniciar terminal local' in frontend()['body'];assert state()['status'] is None
        command('lr9c_start');until(lambda:state()['status'] and state()['status']['state']=='running');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');session=state();sid=session['status']['sessionId'];pid=session['session']['pid'];assert isinstance(sid,str)
        measure('Terminal idle')
        measure('Terminal visual PTY burst >6 MiB',lambda:command('lr9c_burst'));until(lambda:'VISUAL_ALIVE' in state()['session']['tail']);until(lambda:'VISUAL_ALIVE' in (frontend().get('ptyText') or '').replace('\n',''));assert state()['session']['totalBytes']>6*1024*1024;assert state()['session']['gap'];assert state()['status']['state']=='running'
        command('lr9c_input');until(lambda:'INPUT_AFTER_1' in (frontend().get('ptyText') or '').replace('\n',''))
        before=state()['status'];command('lr9c_resize');until(lambda:state()['status']['rows']!=before['rows'] or state()['status']['cols']!=before['cols']);command('lr9c_input');size=state()['status'];until(lambda:f"{size['rows']} {size['cols']}" in state()['session']['tail'])
        measure('Operational Trace burst 6000 facts',lambda:command('lr9c_trace'));trace=frontend();assert trace['rows']<=40;assert 'entregas live perdidas' in trace['body'];assert int(trace['traceUpdates'])<1000
        command('lr9c_narrow');until(lambda:frontend()['width']==640);assert 'Shell' in frontend()['body'] and 'Activity' in frontend()['body']
        command('lr9c_conversation');until(lambda:frontend().get('view')=='conversation');until(lambda:state()['bridgeWorkers']==0);assert state()['session']['pid']==pid;before_burst=state()['session']['totalBytes'];command('lr9c_headless_burst');until(lambda:state()['session']['totalBytes']>=before_burst+6*1024*1024 and 'HEADLESS_ALIVE' in state()['session']['tail']);command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');assert state()['status']['sessionId']==sid;assert 'chunks de PTY anteriores indisponíveis' in frontend()['body']
        command('close');until(lambda:not command('lr9c_snapshot')['windows'] and not webprocesses());until(lambda:state()['bridgeWorkers']==0);before_burst=state()['session']['totalBytes'];command('lr9c_headless_burst');until(lambda:state()['session']['totalBytes']>=before_burst+6*1024*1024 and 'HEADLESS_ALIVE' in state()['session']['tail']);assert state()['session']['pid']==pid;evidence['headlessWebKitWebProcesses']=webprocesses();evidence['headlessBridgeWorkers']=state()['bridgeWorkers']
        measure('Headless real PTY alive')
        second=subprocess.run([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20);assert second.returncode==0;until(lambda:command('snapshot')['mainPresent']);until(lambda:frontend().get('view')=='conversation');assert not any('TerminalWorkspace' in r for r in frontend()['resources']);command('lr9c_terminal');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');assert state()['session']['pid']==pid and state()['status']['sessionId']==sid
        command('lr9c_input');until(lambda:'INPUT_AFTER_3' in (frontend().get('ptyText') or '').replace('\n',''));assert 'chunks de PTY anteriores indisponíveis' in frontend()['body'];evidence['reattachSameSessionPid']=True
        command('lr9c_exit');until(lambda:state()['status']['reaped']);assert not Path(f'/proc/{pid}').exists();until(lambda:state()['brokerActive']==0 and state()['brokerWorkers']==0);until(lambda:frontend().get('terminal',{}).get('terminalState') in ['completed','failed','cancelled'] and 'Iniciar terminal local' in frontend()['body']);command('lr9c_start');until(lambda:state()['status']['state']=='running');until(lambda:frontend().get('terminal',{}).get('terminalConnection')=='conectado');second_pid=state()['session']['pid'];command('lr9c_end_session');until(lambda:state()['status']['reaped']);assert not Path(f'/proc/{second_pid}').exists();until(lambda:state()['brokerActive']==0 and state()['brokerWorkers']==0);until(lambda:frontend().get('terminal',{}).get('terminalState') in ['completed','failed','cancelled'] and 'Iniciar terminal local' in frontend()['body']);command('lr9c_start');until(lambda:state()['status']['state']=='running');third_pid=state()['session']['pid'];started=time.monotonic();command('quit');app.wait(timeout=12);assert app.returncode==0;assert not Path(f'/proc/{third_pid}').exists();evidence['quitElapsedSeconds']=round(time.monotonic()-started,3);evidence['quitExitCode']=app.returncode;evidence['remainingExecutionPids']=[];evidence['pass']=True
        print(json.dumps({k:v for k,v in evidence.items() if k not in ['snapshots','frontend','performance']}),flush=True)
    except BaseException:
        log.flush();print((directory/'native.log').read_text()[-10000:],file=sys.stderr);raise
    finally:
        if app.poll() is None:
            try:command('quit');app.wait(timeout=12)
            except BaseException:app.terminate();app.wait(timeout=15)
        Path(args.output).write_text(json.dumps(evidence,indent=2,ensure_ascii=False)+'\n')
        shutil.copyfile(directory/'native.log',str(args.output)+'.log')
