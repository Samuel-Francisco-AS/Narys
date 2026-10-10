#!/usr/bin/python3
"""Short, isolated idle/Stronghold observations. No inference or human unlock.

The existing Stronghold probe returns booleans only. Never export snapshot headers,
salts, keys, passwords, or historical content. This is not a long-run benchmark.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import time

HOME = Path.home()

def metrics(pid):
    proc = Path('/proc') / str(pid)
    fields = (proc/'stat').read_text().rsplit(')', 1)[1].split()
    status = dict(line.split(':', 1) for line in (proc/'status').read_text().splitlines() if ':' in line)
    return dict(cpu_seconds=(int(fields[11])+int(fields[12]))/os.sysconf('SC_CLK_TCK'),
                rss_kib=int(status['VmRSS'].split()[0]), hwm_kib=int(status['VmHWM'].split()[0]))

def cgroup(pid):
    proc = Path('/proc') / str(pid)
    path = Path('/sys/fs/cgroup')/(proc/'cgroup').read_text().strip().split('::', 1)[1].lstrip('/')
    return {name:(path/name).read_text().strip() for name in ('memory.current','memory.peak','memory.events','memory.pressure')}

def observe(pid, seconds=10, command=None):
    start = time.monotonic()
    before = metrics(pid); cg_before = cgroup(pid)
    samples = [before]
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE) if command else None
    while (child is not None and child.poll() is None) or (child is None and time.monotonic()-start < seconds):
        samples.append(metrics(pid))
        if time.monotonic()-start > 30:
            if child:
                child.terminate()
            raise RuntimeError('metrics_timeout_no_retry')
        time.sleep(.05)
    end = metrics(pid); samples.append(end)
    result = dict(elapsed_seconds=time.monotonic()-start, sample_interval_seconds=.05,
                  samples=len(samples), before=before, after=end,
                  peak_observed_rss_kib=max(s['rss_kib'] for s in samples),
                  cpu_seconds_delta=end['cpu_seconds']-before['cpu_seconds'],
                  cgroup_before=cg_before, cgroup_after=cgroup(pid))
    if child:
        stdout, stderr = child.communicate(timeout=5)
        value = json.loads(stdout)
        # Probe is defined to return only booleans on success; do not dump exceptions.
        assert child.returncode == 0 and value.get('ok') is True, 'stronghold_probe_failed'
        data = value['data']
        assert data == dict(existing_snapshot_opened=True, writes=False, migration=False, secret_values_returned=False)
        result['probe'] = data
        result['stderr_empty'] = stderr == b''
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--idle-only', action='store_true', help='no Stronghold access; suitable before human unlock')
    args = parser.parse_args()
    pid = int(subprocess.check_output(['systemctl','--user','show','narys-core.service','-p','MainPID','--value'], text=True))
    snapshot = HOME/'.local/share/br.com.assistente3d.app/luna-lr3.stronghold'
    # Public KDF work factor only; discard the full encrypted snapshot immediately.
    header = snapshot.read_bytes()
    match = re.search(rb'\n-> scrypt [^\n ]+ (\d+)\n', header)
    factor = int(match[1]) if match else None
    del header
    result = dict(observed_at=datetime.datetime.now().astimezone().isoformat(), pid=pid,
                  remote_inference=False, unlock_invoked=False, long_run_benchmark=False,
                  snapshot_scrypt_log_n=factor,
                  estimated_scrypt_v_buffer_bytes=128*8*(2**factor) if factor is not None else None,
                  idle_before=observe(pid))
    if not args.idle_only:
        result['stronghold_read'] = observe(pid, command=[str(HOME/'.local/lib/narys/narys-core'),'stronghold'])
        result['idle_after'] = observe(pid)
    print(json.dumps(result,indent=2))

if __name__ == '__main__':
    main()
