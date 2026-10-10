#!/usr/bin/python3
"""Read-only installed CLI/persisted history proof. No submissions/unlocks/inference."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
HOME=Path.home();CLI=HOME/'.local/bin/narys'
BASE=Path(__file__).resolve().parents[2]/'docs/evidence/server-1c/host-before.json'
def call(*args):
    p=subprocess.run([str(CLI),*args,'--json'],capture_output=True,timeout=20,check=True,cwd='/tmp',env={'PATH':'/usr/bin:/bin'})
    return json.loads(p.stdout)['data']
def digest(v):return hashlib.sha256(json.dumps(v,sort_keys=True,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
def meta(p):
    s=p.stat();return dict(inode=s.st_ino,size=s.st_size,mtime_ns=s.st_mtime_ns,ctime_ns=s.st_ctime_ns,mode=s.st_mode,sha256=hashlib.file_digest(p.open('rb'),'sha256').hexdigest())
def main():
    before=json.loads(BASE.read_text());dbpath=HOME/'.local/state/narys/core/db/luna.sqlite3'
    with sqlite3.connect(f'file:{dbpath}?mode=ro',uri=True) as db:
        counts={t:db.execute('SELECT count(*) FROM '+t).fetchone()[0] for t in before['counts']}
        sessions=[];cursor=0
        while True:
            page=call('sessions','--after',str(cursor),'--limit','7');sessions+=page['sessions']
            cursor=page['next_session']
            if not page['has_more']:break
        assert len(sessions)==counts['conversation_sessions']
        message_count=0
        for s in sessions:
            sid=s['session_id'];messages=[];after=0
            while True:
                page=call('session',str(sid),'--after',str(after),'--limit','7');messages+=page['messages'];after=page['next_message']
                if not page['has_more']:break
            prior=[{'id':r[0],'role':r[1],'content':r[2],'created_at':r[3]} for r in db.execute('SELECT id,role,content,created_at FROM conversation_messages WHERE session_id=? ORDER BY id',[sid])]
            assert digest(prior)==digest(messages);message_count+=len(messages)
        runs=[]
        for r in db.execute('SELECT task_id,state,result_json FROM conversation_runs ORDER BY task_id'):
            task=call('task',str(r[0]));assert task['state']==r[1]
            assert digest(task.get('result'))==digest(json.loads(r[2]) if r[2] else None)
            runs.append({'task_id':r[0],'state':r[1],'result_equals_authority':True,
                'result_sha256':hashlib.sha256((r[2] or '').encode()).hexdigest()})
        listed=[];cursor=0
        while True:
            page=call('tasks','--after',str(cursor),'--limit','13');listed+=page['tasks'];cursor=page['next_task']
            if not page['has_more']:break
        assert len(listed)==db.execute('SELECT count(*) FROM (SELECT task_id FROM conversation_runs UNION SELECT task_id FROM task_records)').fetchone()[0]
        snapshot_equal=meta(HOME/'.local/share/br.com.assistente3d.app/luna-lr3.stronghold')==before['snapshot']
        assert snapshot_equal and counts==before['counts']
        assert [{k:r[k] for k in ('task_id','state','result_sha256')} for r in runs]==before['runs']
        return {'observed_at':subprocess.check_output(['date','-Iseconds'],text=True).strip(),
            'status':call('status'),'credentials':call('credentials','status'),'models':call('models'),
            'doctor':call('doctor'),'session_pages_complete':True,'sessions_verified':len(sessions),
            'messages_verified_exact':message_count,'product_tasks_listed':len(listed),'runs':runs,
            'counts_unchanged':True,'snapshot_metadata_and_bytes_unchanged':True,
            'schema':db.execute('PRAGMA user_version').fetchone()[0],'integrity':db.execute('PRAGMA quick_check').fetchone()[0],
            'fresh_clients_cwd_tmp_no_home_or_xdg_env':True,'remote_inference':False,'unlock_invoked':False}
if __name__=='__main__':print(json.dumps(main(),indent=2,ensure_ascii=False))
