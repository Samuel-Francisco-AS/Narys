#!/usr/bin/env python3
"""One headless metadata/lifecycle sample. Only /proc/stat, never cmdline/environ.
CPU is sampled process-tree CPU (lower bound when short-lived processes escape a
50ms sample). RSS sums resident pages (shared pages can be counted twice).
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import tempfile


def snapshot():
    rows = {}
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            data = (entry / 'stat').read_text()
            fields = data[data.rfind(')') + 2:].split()
            rows[int(entry.name)] = {'ppid': int(fields[1]), 'pgrp': int(fields[2]),
                'start_ticks': int(fields[19]), 'cpu_ticks': int(fields[11]) + int(fields[12]),
                'rss_bytes': int(fields[21]) * os.sysconf('SC_PAGE_SIZE'), 'state': fields[0]}
        except (OSError, ValueError, IndexError):
            pass
    return rows


def measure(binary, probe, cli, deadline_seconds=90):
    # Linux test harness only: adopt descendants so recovery can reap them.
    # This is not the SDK's behavior and is not a production supervisor.
    if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
        raise OSError("cannot enable local cleanup subreaper")
    before = snapshot()
    started = time.monotonic()
    # The POC and its SDK-owned children share a fresh process group. No shell.
    capture = tempfile.TemporaryFile()
    proc = subprocess.Popen([str(binary), probe, str(cli)], stdin=subprocess.DEVNULL, stdout=capture,
        stderr=subprocess.DEVNULL, start_new_session=True,
        env={**{k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY')},
             'NARYS_LR10A_OWNED_HARNESS': '1'})
    known = {}
    peak_rss = 0
    peak_processes = 0
    timed_out = False
    while proc.poll() is None:
        rows = snapshot()
        owned = {pid: row for pid, row in rows.items() if row['pgrp'] == proc.pid}
        # Include descendants even if they changed their process group.
        for _ in range(16):
            more = {pid: row for pid, row in rows.items()
                    if row['ppid'] in owned and pid not in owned}
            if not more:
                break
            owned.update(more)
        peak_rss = max(peak_rss, sum(r['rss_bytes'] for r in owned.values()))
        peak_processes = max(peak_processes, len(owned))
        for pid, row in owned.items():
            known[(pid, row['start_ticks'])] = row['cpu_ticks']
        if time.monotonic() - started > deadline_seconds:
            timed_out = True
            os.killpg(proc.pid, signal.SIGKILL)
            break
        time.sleep(0.05)
    # Output is metadata-only and bounded by the fixed probes (no tool/model text).
    proc.wait(timeout=3)
    total_output = capture.tell()
    capture.seek(0)
    out = capture.read(4 * 1024 * 1024)
    capture.close()
    after = snapshot()
    survivors = [pid for (pid, start) in known if pid != proc.pid and pid in after
                 and after[pid]['start_ticks'] == start]
    cleanup_reaped = 0
    if survivors:
        # Only observed PID + start-time identities from this invocation.
        for pid in survivors:
            current = snapshot().get(pid)
            if current and (pid, current['start_ticks']) in known:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
    cleanup_deadline = time.monotonic() + 2
    while time.monotonic() < cleanup_deadline:
        try:
            pid, _ = os.waitpid(-1, os.WNOHANG)
            if pid:
                cleanup_reaped += 1
                continue
        except ChildProcessError:
            break
        time.sleep(0.02)
    final = snapshot()
    remaining = [pid for (pid, start) in known if pid != proc.pid and pid in final
                 and final[pid]['start_ticks'] == start]
    try:
        report = json.loads(out)
        recognized = isinstance(report, dict)
        if not recognized:
            report = {'output': 'unrecognized', 'stdout_bytes': len(out)}
    except (ValueError, UnicodeDecodeError):
        report = {'output': 'unrecognized', 'stdout_bytes': len(out)}
        recognized = False
    result = {'probe': probe, 'sample_kind': 'fresh_process_warm_os_caches',
        'headless': True, 'poll_interval_ms': 50, 'exit_code': proc.returncode,
        'stdout_truncated': total_output > len(out),
        'timed_out': timed_out, 'wall_ms': round((time.monotonic() - started) * 1000, 2),
        'peak_process_tree_rss_bytes': peak_rss, 'peak_owned_processes': peak_processes,
        'sampled_cpu_seconds_lower_bound': sum(known.values()) / os.sysconf('SC_CLK_TCK'),
        'system_processes_before': len(before), 'system_processes_after': len(after),
        'observed_owned_processes': len(known), 'owned_survivors_before_recovery': survivors, 'owned_survivors_after_recovery': remaining,
        'harness_reaped_descendants': cleanup_reaped, 'sdk_report': report}
    return result, (1 if survivors or remaining or timed_out or proc.returncode or not recognized or total_output > len(out) else 0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('probe', choices=['metadata', 'metadata-existing-auth', 'sessions', 'sessions-existing-auth'])
    parser.add_argument('cli', type=Path)
    parser.add_argument('--binary', type=Path, default=Path(__file__).parent / 'target/debug/narys-lr10a-poc')
    args = parser.parse_args()
    result, code = measure(args.binary.resolve(), args.probe, args.cli.resolve())
    print(json.dumps(result, indent=2))
    raise SystemExit(code)
