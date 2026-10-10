#!/usr/bin/python3
"""Record the user's already explicit final LR-10A consent, never reset it.

Operator records consent, not provider billing. Same-UID host is trusted;
this file is not an authentication or financial-control bypass.
"""
import hashlib, json, os, stat, time
from pathlib import Path
ROOT=Path.home()/'.local/state/narys/core/lr10a-final-authorization'
PROMPT='Responda somente o número da soma de alpha=2 e beta=3. Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.'
def write_new(path,value):
    fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,'w') as f:
        json.dump(value,f,ensure_ascii=False); f.flush(); os.fsync(f.fileno())
    fd=os.open(path.parent,os.O_DIRECTORY); os.fsync(fd); os.close(fd)
def safe_directory(path):
    m=path.lstat()
    if not stat.S_ISDIR(m.st_mode) or m.st_uid!=os.getuid() or stat.S_IMODE(m.st_mode)!=0o700 or path.resolve()!=path:
        raise SystemExit('UNSAFE_CONSENT_DIRECTORY')
if __name__=='__main__':
    safe_directory(ROOT.parent)
    ROOT.mkdir(mode=0o700) # Existing reservation/consent is never overwritten.
    (ROOT/'receipts').mkdir(mode=0o700)
    write_new(ROOT/'consent.json',{'scope':'lr10a-final-20261010-three-attempts',
        'human_explicit_consent':True,'max_attempts':3,'max_additional_usd':0,
        'provider_additional_usage_disabled':True,
        'billing_unit_uncertainty_explicitly_accepted':True,
        'objective_sha256':hashlib.sha256(PROMPT.encode()).hexdigest(),
        'recorded_at_unix':time.time(),
        'source':'User: LR-10A IMPLEMENTAÇÃO DEFINITIVA E FECHAMENTO; previous additional-budget-disabled confirmation',
        'uncertain_delivery_counts':True,'automatic_retry_authorized':False})
    print('FINAL_CONSENT_RECORDED; maximum 3; zero additional payment authorized')
