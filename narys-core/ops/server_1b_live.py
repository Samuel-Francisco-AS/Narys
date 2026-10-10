#!/usr/bin/python3
"""Explicit live gate. Requires operator-confirmed Groq Free; never changes billing.
The operator must unlock GNOME Keyring in a separate private SSH terminal.
Only new validation-session content is exported. No historical messages/secrets.
"""
import copy, datetime, hashlib, json, os, secrets, subprocess, sys, time
from pathlib import Path

BINARY=Path.home()/'.local/lib/narys/narys-core'
SERVICE=['/usr/bin/systemctl','--user']
def call(command):
    run=subprocess.run([str(BINARY),'ipc'],input=json.dumps(command),capture_output=True,text=True,timeout=30)
    response=json.loads(run.stdout)
    if run.returncode or response.get('ok') is not True:
        raise RuntimeError('live_gate_blocked:'+response.get('error_code','ipc_failed'))
    return response['data']
def terminal(task):
    deadline=time.monotonic()+90
    while time.monotonic()<deadline:
        value=call({'operation':'task-get','task':{'namespace':'product','id':task}})
        if value['state'] not in ('pending','running'):return value
        time.sleep(.1)
    raise RuntimeError('live_gate_result_timeout_no_retry')
def pid():
    return int(subprocess.check_output(SERVICE+['show','narys-core.service','-p','MainPID','--value'],text=True).strip())
def metrics(process):
    proc=Path('/proc')/str(process)
    fields=(proc/'stat').read_text().rsplit(')',1)[1].split()
    rss=next(int(line.split()[1]) for line in (proc/'status').read_text().splitlines() if line.startswith('VmRSS:'))
    return {'pid':process,'cpu_seconds':(int(fields[11])+int(fields[12]))/os.sysconf('SC_CLK_TCK'),'rss_kib':rss}
def save(path,data):
    fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,'w') as stream:
        json.dump(data,stream,ensure_ascii=False,indent=2);stream.write('\n');stream.flush();os.fsync(stream.fileno())
def main():
    if len(sys.argv)!=3 or sys.argv[1]!='--groq-free-confirmed-no-overage':
        raise SystemExit('Explicit operator confirmation is required; no credentials accepted.')
    output=Path(sys.argv[2]);started=datetime.datetime.now().astimezone().isoformat()
    status=call({'operation':'status'})
    if status['graphical_environment_present'] or status['agent_execution_authority'] or status['tools']!=0:
        raise RuntimeError('live_gate_runtime_boundary_failed')
    providers=call({'operation':'providers'})
    groq=next(p for p in providers['providers'] if p['id']=='groq')
    if not providers['credential_store_available'] or not groq['configured']:
        raise RuntimeError('live_gate_existing_credentials_required')
    original=providers['conversation_policy']
    if original['routingMode']!='fixed' or [t['providerId'] for t in original['targets']]!=['groq']:
        raise RuntimeError('live_gate_existing_groq_route_required')
    permission=call({'operation':'provider-configure','provider_id':'groq','enabled':True,'free_tier_confirmed':True})
    bounded=copy.deepcopy(original);bounded.update(maxOutputTokens=512,maxProviderCalls=1,retryEnabled=False,maxRetries=0)
    changed=False
    try:
        call({'operation':'conversation-policy','policy':bounded});changed=True
        session=call({'operation':'session-create'})['session_id']
        marker='SERVER1B-'+secrets.token_hex(4)
        text=f'Guarde nesta conversa o marcador {marker}. Responda brevemente com o marcador e com o resultado de 17 + 26.'
        receipt=call({'operation':'conversation','session_id':session,'text':text})
        # Each client subprocess has exited; the provider worker belongs to the service.
        first=terminal(receipt['task_id'])
        if first['state']!='completed' or first['result']['providerId']!='groq':
            save(output,{'started_at':started,'session_id':session,'first':first,'pass':False});raise RuntimeError('live_gate_provider_failed_no_retry')
        if marker not in first['result']['text'] or '43' not in first['result']['text']:
            save(output,{'started_at':started,'session_id':session,'first':first,'pass':False});raise RuntimeError('live_gate_answer_validation_failed_no_retry')
        messages=call({'operation':'session-get','session_id':session})
        before_pid=pid()
        subprocess.run(SERVICE+['restart','narys-core.service'],check=True,timeout=45)
        after_pid=pid()
        recovered_task=call({'operation':'task-get','task':{'namespace':'product','id':receipt['task_id']}})
        recovered_session=call({'operation':'session-get','session_id':session})
        if recovered_task!=first or recovered_session!=messages or before_pid==after_pid:
            raise RuntimeError('live_gate_restart_recovery_failed')
        followup='Qual marcador de continuidade eu registrei nesta sessão e qual foi o resultado da soma? Responda brevemente.'
        second_receipt=call({'operation':'conversation','session_id':session,'text':followup})
        second=terminal(second_receipt['task_id'])
        if second['state']!='completed' or second['result']['providerId']!='groq' or marker not in second['result']['text'] or '43' not in second['result']['text']:
            save(output,{'started_at':started,'session_id':session,'first':first,'second':second,'pass':False});raise RuntimeError('live_gate_followup_failed_no_retry')
        cancel_receipt=call({'operation':'conversation','session_id':session,'text':'Mensagem de validação de cancelamento SERVER-1B. Responda em uma frase.'})
        cancel=call({'operation':'task-cancel','task':{'namespace':'product','id':cancel_receipt['task_id']}})
        cancelled=terminal(cancel_receipt['task_id'])
        final_messages=call({'operation':'session-get','session_id':session})
        events=call({'operation':'events'})
        sample_start=time.monotonic();m1=metrics(after_pid);time.sleep(10);m2=metrics(pid())
        idle={'start':m1,'end':m2,'elapsed_seconds':time.monotonic()-sample_start,'cpu_seconds_delta':m2['cpu_seconds']-m1['cpu_seconds']}
        evidence={'started_at':started,'finished_at':datetime.datetime.now().astimezone().isoformat(),'account_confirmation':'operator confirmed Groq Free, quota available, no overage','permission':permission,'session_id':session,'marker':marker,'first_request':text,'first_receipt':receipt,'first_result':first,'restart':{'pid_before':before_pid,'pid_after':after_pid,'task_equal':recovered_task==first,'session_equal':recovered_session==messages,'no_replay':True},'followup_request':followup,'second_receipt':second_receipt,'second_result':second,'cancel_receipt':cancel_receipt,'cancel_response':cancel,'cancel_result':cancelled,'final_session':final_messages,'events':events,'idle':idle,'pass':cancelled['state']=='cancelled'}
        save(output,evidence)
        print(json.dumps({'pass':evidence['pass'],'session_id':session,'tasks':[receipt['task_id'],second_receipt['task_id'],cancel_receipt['task_id']],'restart_equal':True,'cancel_state':cancelled['state'],'rss_kib':m2['rss_kib'],'idle_cpu_seconds':idle['cpu_seconds_delta'],'evidence':str(output)}))
    finally:
        if changed:call({'operation':'conversation-policy','policy':original})
if __name__=='__main__':main()
