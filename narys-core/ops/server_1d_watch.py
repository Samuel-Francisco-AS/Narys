#!/usr/bin/python3
"""Read-only sampling while the human sends the authorized Groq gate via SSH.

Never submits, unlocks or reads secrets. Exports only state/timings/metrics and
hashes of new validation runs in session39. No message/result text is exported.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import time

from server_1d_metrics import cgroup, metrics

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--after-task', type=int, required=True)
    parser.add_argument('--seconds', type=int, default=120)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert args.after_task >= 192 and 1 <= args.seconds <= 600
    pid = int(subprocess.check_output(['systemctl','--user','show','narys-core.service','-p','MainPID','--value'],text=True))
    dbpath = Path.home()/'.local/state/narys/core/db/luna.sqlite3'
    start = time.monotonic(); before = metrics(pid); cg_before = cgroup(pid)
    peak = before['rss_kib']; count = 0; timeline = []; last = None
    report = dict(started_at=datetime.datetime.now().astimezone().isoformat(), pid=pid,
                  after_task=args.after_task, remote_submissions_by_monitor=0,
                  unlock_invoked=False, before=before, cgroup_before=cg_before,
                  sample_interval_seconds=.05, long_run_benchmark=False)
    with sqlite3.connect(f'file:{dbpath}?mode=ro',uri=True) as db:
        while time.monotonic()-start < args.seconds:
            sample = metrics(pid); peak = max(peak,sample['rss_kib']); count += 1
            if count % 4 == 0:
                rows = db.execute('SELECT task_id,state,error_code,result_json FROM conversation_runs WHERE session_id=39 AND task_id>? ORDER BY task_id',[args.after_task]).fetchall()
                state = [(r[0],r[1]) for r in rows]
                if state != last:
                    timeline.append(dict(elapsed_seconds=time.monotonic()-start, states=state)); last = state
                report.update(elapsed_seconds=time.monotonic()-start, samples=count,
                              after=sample, peak_observed_rss_kib=peak,
                              cpu_seconds_delta=sample['cpu_seconds']-before['cpu_seconds'],
                              timeline=timeline, cgroup_after=cgroup(pid),
                              runs=[dict(task_id=r[0],state=r[1],error_code=r[2],result_present=r[3] is not None,
                                         result_sha256=hashlib.sha256((r[3] or '').encode()).hexdigest()) for r in rows])
                temporary=args.output.with_suffix('.tmp')
                temporary.write_text(json.dumps(report,indent=2)+'\n'); temporary.replace(args.output)
                if rows and all(r[1] not in ('pending','running') for r in rows):
                    break
            time.sleep(.05)
    report['finished_at'] = datetime.datetime.now().astimezone().isoformat()
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('elapsed_seconds','peak_observed_rss_kib','cpu_seconds_delta','runs')}))

if __name__ == '__main__':
    main()
