#!/usr/bin/env python3
"""Linux /proc sampling. Labels describe a scenario; this tool does not attest focus.
CPU is percent of one logical core, RSS is a sum (shared pages can be counted twice).
No credentials, command lines or conversation content are collected.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import time


def read_tree(root):
    rows = {}
    for file in Path('/proc').glob('[0-9]*/stat'):
        try:
            stat = file.read_text()
            tail = stat[stat.rfind(')') + 2:].split()
            pid = int(file.parent.name)
            rows[pid] = dict(pid=pid, ppid=int(tail[1]), name=stat[stat.find('(') + 1:stat.rfind(')')],
                             startTicks=int(tail[19]), cpuTicks=int(tail[11]) + int(tail[12]),
                             rssKiB=int(tail[21]) * os.sysconf('SC_PAGE_SIZE') // 1024)
        except (OSError, ValueError):
            continue
    selected = {root}
    while True:
        children = {pid for pid, row in rows.items() if row['ppid'] in selected}
        if children <= selected:
            break
        selected |= children
    if root not in rows:
        raise RuntimeError('Root process exited; sample is incomplete')
    return [rows[pid] for pid in sorted(selected) if pid in rows]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--scenario', required=True)
    parser.add_argument('--seconds', type=int, default=30)
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error('--seconds must be positive')
    ticks = os.sysconf('SC_CLK_TCK')
    print(json.dumps(dict(type='environment', scenario=args.scenario, utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                         platform=platform.platform(), session=os.getenv('XDG_SESSION_TYPE'), clockTicks=ticks,
                         logicalCpus=os.cpu_count(), durationSeconds=args.seconds)), flush=True)
    samples = []
    for index in range(args.seconds + 1):
        at = time.monotonic()
        rows = read_tree(args.pid)
        samples.append((at, rows))
        print(json.dumps(dict(type='sample', elapsedSeconds=round(at - samples[0][0], 3),
                             rssKiB=sum(row['rssKiB'] for row in rows), processes=rows)), flush=True)
        if index < args.seconds:
            time.sleep(max(0, samples[0][0] + index + 1 - time.monotonic()))
    cpu_ticks = 0
    # Integrate adjacent samples with identity checks; process churn is reported.
    churn = set()
    for previous, current in zip(samples, samples[1:]):
        old = {(row['pid'], row['startTicks']): row for row in previous[1]}
        new = {(row['pid'], row['startTicks']): row for row in current[1]}
        churn |= old.keys() ^ new.keys()
        cpu_ticks += sum(max(0, row['cpuTicks'] - old[key]['cpuTicks']) for key, row in new.items() if key in old)
    duration = samples[-1][0] - samples[0][0]
    print(json.dumps(dict(type='summary', elapsedSeconds=round(duration, 3),
                         cpuPercentOneCore=round(100 * cpu_ticks / ticks / duration, 3),
                         rssMinKiB=min(sum(r['rssKiB'] for r in rows) for _, rows in samples),
                         rssMaxKiB=max(sum(r['rssKiB'] for r in rows) for _, rows in samples),
                         rssFinalKiB=sum(r['rssKiB'] for r in samples[-1][1]),
                         processChurn=sorted(churn), cpuChurnLimitation=bool(churn))), flush=True)


if __name__ == '__main__':
    main()
