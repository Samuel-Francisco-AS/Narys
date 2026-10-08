#!/usr/bin/env python3
"""LR-9B: real Tauri Close/Reopen/Quit, fixed native Exec/PTY fixtures, no network.
Build with --features lr9b-probe. Isolates DBus, app data and synthetic vault.
No generic shell IPC, provider request or real credentials.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

if not os.getenv('NARYS_LR9B_PROBE_BUS'):
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, __file__, *sys.argv[1:]], {**os.environ, 'NARYS_LR9B_PROBE_BUS': '1'})
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', default='src-tauri/target/debug/assistente-3d')
parser.add_argument('--output', default='/tmp/narys-lr9b-native.json')
args = parser.parse_args()
binary = Path(args.binary).resolve()
evidence = {'pass': False, 'binarySha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'realTauri': True, 'providerInvocationRequestedByHarness': False, 'snapshots': []}
with tempfile.TemporaryDirectory(prefix='narys-lr9b-native-') as temporary:
    directory = Path(temporary)
    shutil.copyfile(Path(__file__).with_name('fixtures')/'perf1c-identity.json', directory/'identity.json')
    env = {**os.environ, 'XDG_DATA_HOME': str(directory/'data'), 'NARYS_PERF1C_PROBE': str(directory), 'NARYS_PERF1C_ENDPOINT': 'http://127.0.0.1:9/not-used', 'LIBGL_ALWAYS_SOFTWARE': '1'}
    log = open(directory/'native.log', 'w+')
    app = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    sequence = 0

    def until(check, timeout=30):
        end = time.monotonic()+timeout
        while time.monotonic() < end:
            result = check()
            if result:
                return result
            if app.poll() is not None:
                raise RuntimeError('native app exited unexpectedly')
            time.sleep(.1)
        raise TimeoutError('condition not reached')

    def command(action):
        global sequence
        sequence += 1
        pending = directory/'request.tmp'
        pending.write_text(json.dumps({'sequence': sequence, 'action': action}))
        pending.replace(directory/'request.json')

        def read():
            try:
                value = json.loads((directory/'response.json').read_text())
                if value['sequence'] == sequence:
                    return value
            except (OSError, json.JSONDecodeError):
                pass
        value = until(read)
        assert value['error'] is None, value
        evidence['snapshots'].append({'action': action, **value})
        return value

    def webprocesses():
        # Walk the full descendant tree, including any intermediate launcher.
        processes = {}
        for stat in Path('/proc').glob('[0-9]*/stat'):
            try:
                fields = stat.read_text().rsplit(')', 1)[1].split()
                processes[int(stat.parent.name)] = (int(fields[1]), fields[0], (stat.parent/'comm').read_text().strip())
            except (OSError, ValueError):
                pass
        children = {}
        for pid, (parent, _, _) in processes.items():
            children.setdefault(parent, []).append(pid)
        descendants = set()
        pending = [app.pid]
        while pending:
            for pid in children.get(pending.pop(), []):
                if pid not in descendants:
                    descendants.add(pid)
                    pending.append(pid)
        return [pid for pid in descendants if processes[pid][1] != 'Z' and processes[pid][2].startswith('WebKitWeb')]

    def state():
        return command('lr9b_snapshot')

    try:
        initial = until(lambda: (s if s['mainPresent'] else None) if (s := command('snapshot')) else None, 90)
        until(lambda: len(webprocesses()) == 1)
        command('lr9b_start')
        active = until(lambda: (s if s['execution']['runs']['ptyState'] == 'Running' and s['execution']['runs']['sleeperState'] == 'Running' and s['execution']['runs']['floodState'] == 'Running' else None) if (s := state()) else None)
        broker = active['execution']['brokerAddress']
        runs = active['execution']['runs']
        assert len({runs['ptyId'], runs['floodId'], runs['sleeperId']}) == 3
        pids = [runs[k] for k in ['ptyPid', 'floodPid', 'sleeperPid']]
        assert all(pids)
        command('close')
        until(lambda: not (s := state())['mainPresent'] and not s['windows'] and not webprocesses())
        command('lr9b_burst')
        until(lambda: (directory/'pty-flood-done').exists())
        flooded = until(lambda: (s if s['execution']['runs']['floodState'] == 'Completed' and 'PTY_ALIVE' in s['execution']['runs']['ptyTail'] else None) if (s := state()) else None)
        assert not flooded['windows'] and not flooded['mainPresent'] and not webprocesses()
        execution = flooded['execution']
        assert execution['brokerAddress'] == broker and execution['active'] == 2 and execution['sessions'] == 1
        runs = execution['runs']
        assert runs['ptyGap'] and runs['ptyDroppedBytes'] > 0 and runs['ptyRetainedBytes'] <= 2*1024*1024 and runs['ptyRetainedChunks'] <= 512
        result = runs['floodResult']
        assert result['reaped'] and result['stdoutTotal'] == result['stderrTotal'] == 768*8192
        assert result['stdoutDropped'] > 0 and result['stderrDropped'] > 0
        evidence['headlessWebKitWebProcesses'] = webprocesses()
        second = subprocess.run([str(binary)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
        assert second.returncode == 0
        reopened = until(lambda: (s if s['mainPresent'] and len(s['windows']) == 1 else None) if (s := state()) else None)
        until(lambda: len(webprocesses()) == 1)
        assert reopened['execution']['brokerAddress'] == broker and reopened['registryAddress'] == initial['registryAddress']
        assert reopened['execution']['runs']['ptyPid'] == pids[0] and reopened['execution']['runs']['sleeperPid'] == pids[2]
        command('lr9b_resize')
        until(lambda: '39 111' in (s := state())['execution']['runs']['ptyTail'] and 'AFTER_REOPEN' in s['execution']['runs']['ptyTail'])
        # Quit begins with both a PTY and a structured child still active, Headless.
        command('close')
        until(lambda: not (s := state())['windows'] and not webprocesses())
        started = time.monotonic()
        command('quit')
        app.wait(timeout=12)
        evidence['quitElapsedSeconds'] = round(time.monotonic()-started, 3)
        assert app.returncode == 0
        # Direct children must be reaped before app exit; existence as zombies fails.
        assert all(not Path(f'/proc/{pid}').exists() for pid in pids), pids
        evidence['quitExitCode'] = app.returncode
        evidence['remainingExecutionPids'] = []
        evidence['pass'] = True
        print(json.dumps({k: v for k, v in evidence.items() if k != 'snapshots'}), flush=True)
    except BaseException:
        log.flush()
        print((directory/'native.log').read_text()[-12000:], file=sys.stderr)
        raise
    finally:
        if app.poll() is None:
            try:
                command('quit')
                app.wait(timeout=12)
            except (OSError, RuntimeError, TimeoutError, subprocess.TimeoutExpired):
                app.terminate()
                app.wait(timeout=15)
        Path(args.output).write_text(json.dumps(evidence, indent=2, ensure_ascii=False)+'\n')
