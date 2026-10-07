#!/usr/bin/env python3
"""Native release comparison, fresh process tree per sample, isolated SQLite.
Requires a built release binary; never reads or changes the user's real database.
10s warmup + 30s /proc sampler inherited from PERF-1A. Focus is NOT attested.
Run sequentially with no compiler/test/probe workload in progress.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', default='src-tauri/target/release/assistente-3d')
parser.add_argument('--rounds', type=int, default=2)
args = parser.parse_args()
binary = Path(args.binary).resolve()
print(json.dumps({'type':'binary','path':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'release':True,'focus':'not attested'}),flush=True)
with tempfile.TemporaryDirectory(prefix='perf1b-native-') as directory:
    env = {**os.environ, 'XDG_DATA_HOME': directory, 'LIBGL_ALWAYS_SOFTWARE': '1'}
    app = None
    try:
        # Seed through the actual migration path once; all later runs share this data.
        app = subprocess.Popen([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        database = None
        for _ in range(200):
            if app.poll() is not None: raise RuntimeError('Native bootstrap exited')
            matches = list(Path(directory).rglob('luna.sqlite3'))
            if matches:
                try:
                    with sqlite3.connect(matches[0]) as conn:
                        conn.execute('SELECT presentation_mode FROM shell_settings').fetchone()
                    database = matches[0]
                    break
                except sqlite3.Error: pass
            time.sleep(.1)
        if database is None: raise RuntimeError('Migrated isolated database missing')
        app.terminate(); app.wait(timeout=15); app = None
        for index in range(args.rounds):
            # Alternate order to expose warm-cache/order effects.
            for mode in (['presence','economy'] if index % 2 == 0 else ['economy','presence']):
                with sqlite3.connect(database) as conn:
                    conn.execute('UPDATE shell_settings SET presentation_mode=? WHERE id=1',(mode,))
                app = subprocess.Popen([str(binary)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                print(json.dumps({'type':'scenario','round':index+1,'mode':mode,'pid':app.pid,'warmupSeconds':10}),flush=True)
                time.sleep(10)
                if app.poll() is not None: raise RuntimeError('Native application exited during warmup')
                result = subprocess.run([sys.executable,str(Path(__file__).with_name('perf1a-process-baseline.py')),'--pid',str(app.pid),'--scenario',f'release-{mode}-round-{index+1}-focus-unattested','--seconds','30'],check=True,stdout=subprocess.PIPE,text=True)
                print(result.stdout,end='',flush=True)
                app.terminate(); app.wait(timeout=15); app = None
                time.sleep(2)
    finally:
        if app is not None and app.poll() is None:
            app.terminate(); app.wait(timeout=15)
