import datetime, hashlib, json, os, pathlib, subprocess, sys, time
sys.path.insert(0, str(pathlib.Path('narys-core/tests').resolve()))
from test_boundary import Boundary, CORE, CLI, peer, write, command
out = pathlib.Path('docs/evidence/lr10c/fix1/operational-chain.json')
b = Boundary('runTest')
start = time.monotonic(); b.setUp(); boot = time.monotonic()-start
report = {'generated_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'classification':'offline local operational proof only; candidate for independent re-audit', 'layers':['real release Core','real release CLI/operator socket','scripted operator confirmation in trusted host domain','synthetic Python peer','real isolated subprocess tools','authoritative instance SQLite'], 'physical_human_tested':False,'ssh_transport_tested':False,'copilot_authenticated_tested':False,'provider_requests':0,'cost_authorized':False,'temporary_root':str(b.root),'binaries':{},'measurements':{'startup_seconds':round(boot,6)}}
for label, file in [('core', CORE), ('cli', CLI)]:
    report['binaries'][label]={'path':str(file), 'sha256':hashlib.sha256(file.read_bytes()).hexdigest()}
def confirm(receipt):
    shown = b.good('show', approval_id=receipt['approval_id']); d=shown['exact_preview']['digest']
    r=subprocess.run([str(CLI),'boundary',str(b.socket),'approve',receipt['approval_id']],input=f"approve {receipt['approval_id']} {d}\n",text=True,capture_output=True,timeout=8)
    assert r.returncode==0, r.stderr+r.stdout
    assert 'Confirme a operação exata' in r.stdout
    return {'approval_id':receipt['approval_id'],'operator_client_exit':r.returncode,'confirmation':'scripted exact approve ID digest, after CLI showed preview','preview':shown['exact_preview']}
try:
    started=time.monotonic()
    t=b.start(peer(write(), command('/usr/bin/sha256sum',['/workspace/artifact.txt'])))
    confirmations=[confirm(b.pending(t['task_id'])),confirm(b.pending(t['task_id']))]
    done=b.terminal(t['task_id']); assert done['state']=='completed',done
    data=(pathlib.Path(t['workspace'])/'artifact.txt').read_bytes(); assert data==b'NARYS_OFFLINE_TEST:approved\n'
    report['positive_chain']={'request':t,'confirmations':confirmations,'durable_result':done,'independent_artifact':{'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'exact_bytes':'NARYS_OFFLINE_TEST:approved\n'},'duration_seconds':round(time.monotonic()-started,6)}
    assert done['executions'][0]['result']['result']['evidence']['sha256']==hashlib.sha256(data).hexdigest()
    replay=b.req('approve',approval_id=confirmations[0]['approval_id'],digest=confirmations[0]['preview']['digest']); assert not replay['ok']
    report['receipt_replay_attempt']=replay
    long=b.start(peer(command())); confirmation=confirm(b.pending(long['task_id']))
    b.wait(lambda:any(e['phase']=='started' for e in b.good('task',task_id=long['task_id'])['executions']))
    other=b.start(peer(write('parallel.txt'))); confirm(b.pending(other['task_id'])); other_done=b.terminal(other['task_id']); assert other_done['state']=='completed'
    assert b.good('task',task_id=long['task_id'])['executions'][0]['phase']=='started'
    status=pathlib.Path(f'/proc/{b.proc.pid}/status').read_text()
    report['measurements']['core_memory_kib']={line.split(':')[0]:int(line.split()[1]) for line in status.splitlines() if line.startswith(('VmRSS:','VmHWM:'))}
    now=time.monotonic(); ack=b.good('cancel',task_id=long['task_id']); ack_time=time.monotonic()-now
    terminal=b.terminal(long['task_id']); clean_time=time.monotonic()-now
    assert terminal['executions'][0]['phase']=='cancelled' and terminal['executions'][0]['cleanup_verified'] and terminal['peer_cleanup_verified']
    assert clean_time<2,clean_time
    report['cancellation']={'request':long,'confirmation':confirmation,'ack':ack,'durable_result':terminal,'ack_seconds':round(ack_time,6),'cleanup_seconds':round(clean_time,6),'other_workspace_completed_before_cancel':other_done}
    with b.db() as db:
        db.row_factory=__import__('sqlite3').Row
        report['sqlite']={'user_version':db.execute('pragma user_version').fetchone()[0], 'events':[dict(r) for r in db.execute('select * from agent_execution_events order by sequence')], 'receipts':[dict(r) for r in db.execute('select approval_id,task_id,state,policy_version from agent_approvals order by rowid')]}
    report['status']=b.good('status'); assert not report['status']['cost_authorized']
    b.good('shutdown'); code=b.proc.wait(timeout=8); assert code==0,code
    b.proc.stdout.close();b.err.close();b.boot()
    history=b.good('task',task_id=t['task_id']); assert history['executions']==done['executions']
    old=b.req('approve',approval_id=confirmations[0]['approval_id'],digest=confirmations[0]['preview']['digest']);assert not old['ok']
    report['clean_restart']={'history_preserved':True,'effects_replayed':0,'old_receipt_approval':old,'status':b.good('status')}
    report['elapsed_seconds']=round(time.monotonic()-start,6)
    out.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'output':str(out),'write_sha256':report['positive_chain']['independent_artifact']['sha256'],'startup_seconds':report['measurements']['startup_seconds'],'cancel_ack_seconds':report['cancellation']['ack_seconds'],'cancel_cleanup_seconds':report['cancellation']['cleanup_seconds'],'core_memory_kib':report['measurements']['core_memory_kib'],'elapsed_seconds':report['elapsed_seconds']}))
finally:
    b.tearDown()
