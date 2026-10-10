#!/usr/bin/python3
"""Read-only evidence of preserved historical rows. No messages/secrets printed."""
import json, sqlite3
from pathlib import Path
import server_1a_audit as baseline

def preserved_rows(old, new):
    report={}
    with baseline.connect(old) as prior, baseline.connect(new) as current:
        for (table,) in prior.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT IN ('cognitive_role_policies','cognitive_role_targets','cognitive_rate_state','server_events') ORDER BY name"):
            columns=[r[1] for r in prior.execute('PRAGMA table_info("'+table+'")')]
            names=','.join('"'+c.replace('"','""')+'"' for c in columns)
            old_rows=prior.execute('SELECT '+names+' FROM "'+table+'"').fetchall()
            new_rows=set(current.execute('SELECT '+names+' FROM "'+table+'"').fetchall())
            report[table]={'historical_count':len(old_rows),'all_original_rows_equal':all(row in new_rows for row in old_rows)}
    return report

def main():
    evidence=baseline.main()
    with baseline.connect(baseline.DB) as db:
        backup=Path(db.execute("SELECT backup_directory FROM server_migrations WHERE name='server-1a-desktop-takeover'").fetchone()[0])
        evidence['historical_rows']=preserved_rows(backup/'desktop.sqlite3',baseline.DB)
        evidence['historical_rows_preserved']=all(r['all_original_rows_equal'] for r in evidence['historical_rows'].values())
        evidence['runs']=[dict(zip(['task_id','session_id','state','error_code','result_present'],row)) for row in db.execute('SELECT task_id,session_id,state,error_code,result_json IS NOT NULL FROM conversation_runs ORDER BY task_id')]
        evidence['provider_permissions']=[dict(zip(['provider_id','enabled','free_tier_confirmed'],row)) for row in db.execute('SELECT provider_id,enabled,free_tier_confirmed FROM server_provider_permissions ORDER BY provider_id')]
    evidence['protocol']['providers']=json.loads(baseline.command(str(baseline.HOME/'.local/lib/narys/narys-core'),'providers'))
    return evidence
if __name__=='__main__':print(json.dumps(main(),ensure_ascii=False,indent=2))
