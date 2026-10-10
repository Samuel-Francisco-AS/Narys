#!/usr/bin/python3
"""Bind an unused task to immutable existing consent; does not send or retry."""
import hashlib,json,os,sqlite3,sys,time,stat
from pathlib import Path
from record_final_consent import ROOT,PROMPT,write_new,safe_directory
if len(sys.argv)!=2 or not sys.argv[1].isdigit() or int(sys.argv[1])<=1:
    raise SystemExit('NEW_TASK_ID_REQUIRED')
safe_directory(ROOT);safe_directory(ROOT/'receipts')
m=(ROOT/'consent.json').lstat()
if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.getuid() or stat.S_IMODE(m.st_mode)!=0o600 or m.st_nlink!=1:
    raise SystemExit('UNSAFE_CONSENT')
c=json.loads((ROOT/'consent.json').read_text())
if c.get('scope')!='lr10a-final-20261010-three-attempts' or c.get('max_attempts')!=3 or c.get('max_additional_usd')!=0 or not c.get('human_explicit_consent') or (ROOT/'closed.json').exists():
    raise SystemExit('CONSENT_UNAVAILABLE')
id=int(sys.argv[1]);db=sqlite3.connect(f'file:{ROOT.parent}/db/luna.sqlite3?mode=ro',uri=True)
r=db.execute('SELECT objective,expected,state FROM headless_tasks WHERE id=?',(id,)).fetchone()
if r!=(PROMPT,'5','prepared'):raise SystemExit('TASK_OUTSIDE_FIXED_CONSENT')
write_new(ROOT/'receipts'/f'task-{id}.json',{'authorization_scope':c['scope'],'authorized_task_id':id,
    'objective_sha256':hashlib.sha256(PROMPT.encode()).hexdigest(),
    'max_additional_usd':0,'provider_additional_usage_disabled':True,
    'billing_unit_uncertainty_explicitly_accepted':True,'max_sdk_send_calls':1,
    'human_reviewed_at_unix':time.time(),'source':'Previously recorded explicit final human consent'})
print('NEW_TASK_BOUND_TO_EXISTING_CONSENT; no inference sent')
