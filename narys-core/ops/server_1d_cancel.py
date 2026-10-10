#!/usr/bin/python3
"""Installed CLI cancellation gate with all remote providers disabled.

One synthetic conversation input is durably admitted into a new validation session.
Groq's existing permission is restored in finally; no remote inference is possible.
Do not run while a human is using Conversation or modifying provider permissions.
"""
import datetime
import json
from pathlib import Path
import subprocess
import time

CLI = Path.home()/'.local/bin/narys'

def call(*args, text=None):
    p = subprocess.run([str(CLI), *args, '--json'],input=text,capture_output=True,text=True,timeout=30)
    value = json.loads(p.stdout)
    if p.returncode or value.get('ok') is not True:
        raise RuntimeError(value.get('error_code','cancel_gate_cli_failed'))
    return value['data']

def main():
    status = call('status')
    assert status['product_active_tasks'] == 0 and status['active_task'] is None
    providers = call('providers')['providers']
    assert all(p['id']=='groq' or not p['enabled'] for p in providers)
    groq = next(p for p in providers if p['id']=='groq')
    assert groq['enabled'] and groq['local_state'] == 'ready_local_quota_unverified'
    report = dict(observed_at=datetime.datetime.now().astimezone().isoformat(),remote_inference=False,
                  method='all provider permissions disabled before admission; official installed CLI')
    call('provider','groq','disable')
    try:
        assert not any(p['enabled'] for p in call('providers')['providers'])
        session = call('session','new')['session_id']
        receipt = call('send',str(session),text='SERVER1D: entrada técnica para cancelamento sem provider habilitado.')
        tid = receipt['task_id']
        cancelled = call('cancel',str(tid))
        end = time.monotonic()+10
        while True:
            task = call('task',str(tid))
            if task['state'] not in ('pending','running'):
                break
            assert time.monotonic()<end, 'cancel_gate_timeout'
            time.sleep(.05)
        events = call('events','--after','0','--limit','128')['events']
        codes = [e['code'] for e in events if e.get('task_id') == tid and e['namespace']=='product']
        report.update(session_id=session,receipt=receipt,cancel_response=cancelled,
                      task_id=tid,state=task['state'],error_code=task.get('error_code'),
                      result_present=task.get('result') is not None,event_codes=codes,
                      eligible_cancel_pass=task['state']=='cancelled',
                      repeated_cancel=call('cancel',str(tid)))
    finally:
        call('provider','groq','enable','--confirm-free')
    report['provider_permission_restored'] = True
    print(json.dumps(report,indent=2))

if __name__ == '__main__':
    main()
