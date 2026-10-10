#!/usr/bin/python3
"""Human-only one-task financial attestation, never a provider proof or bypass.
Use ONLY after the operator has explicitly authorized the remaining uncertainty
and independently checked additional usage is disabled in GitHub billing.
"""
import json, os, sys, time, hashlib, subprocess
from pathlib import Path
if len(sys.argv)!=2 or not sys.argv[1].isdigit() or not sys.stdin.isatty() or not sys.stdout.isatty():
    raise SystemExit('PRIVATE_HUMAN_TERMINAL_AND_TASK_ID_REQUIRED')
if 'NARYS_LR10A_OWNED_HARNESS' in os.environ: raise SystemExit('HUMAN_REVIEW_REQUIRED')
query=subprocess.run([str(Path.home()/'.local/lib/narys/narys-core'),'result',sys.argv[1]],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,check=True)
result=json.loads(query.stdout)['data']
if result['state']!='prepared':raise SystemExit('TASK_NOT_PREPARED')
job=Path(result['private_directory'])
task=json.loads((job/'task.json').read_text())
print('Workspace autorizado:', str(job/'workspace'))
print('Tarefa:', task['objective'])
print('A API pinada informa requests; custo máximo em AI Credits não está comprovado.')
print('Este registro só admite a tarefa indicada; NÃO autoriza pagamento ou retry.')
print('Confirme no GitHub que uso adicional está desativado e que existe franquia.')
if input('Digite SEM_COBRANCA_ADICIONAL para autorizar a incerteza residual desta única tarefa: ')!='SEM_COBRANCA_ADICIONAL':
    raise SystemExit('FINANCIAL_REVIEW_NOT_GRANTED')
root=Path.home()/'.local/state/narys/core'
m=root.lstat()
if not root.is_dir() or root.is_symlink() or m.st_uid!=os.getuid() or m.st_mode&0o777!=0o700:raise SystemExit('UNSAFE_STATE')
receipt={'authorized_task_id':int(sys.argv[1]),'max_additional_usd':0,
         'provider_additional_usage_disabled':True,'human_reviewed_at_unix':time.time(),
         'billing_unit_uncertainty_explicitly_accepted':True,'max_sdk_send_calls':1,'objective_sha256':hashlib.sha256(task['objective'].encode()).hexdigest()}
fd=os.open(root/'financial-review.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
with os.fdopen(fd,'w') as f: json.dump(receipt,f);f.flush();os.fsync(f.fileno())
print('ONE_TASK_REVIEW_RECORDED; nenhuma inferência enviada')
