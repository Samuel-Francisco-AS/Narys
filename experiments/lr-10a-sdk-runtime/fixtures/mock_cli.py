#!/usr/bin/env python3
"""Deterministic local JSON-RPC fixture. No network, credentials or inference.
Unknown methods fail closed. Session persistence is fixture-only, never CLI proof.
"""
import json
import os
import subprocess
import sys
import time

mode = os.environ.get('MOCK_MODE', 'normal')
pid_file = os.environ.get('MOCK_PID_FILE')
if pid_file:
    with open(pid_file, 'w') as f:
        f.write(str(os.getpid()))
child = None
if mode == 'descendant':
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
    with open(pid_file + '.child', 'w') as f:
        f.write(str(child.pid))
if mode == 'early_exit':
    sys.exit(7)


def emit(payload):
    body = json.dumps(payload, separators=(',', ':')).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
    sys.stdout.buffer.flush()


def event(sid, kind, eid, parent=None):
    emit({'jsonrpc': '2.0', 'method': 'session.event', 'params': {
        'sessionId': sid, 'event': {'id': eid, 'timestamp': '2026-10-09T00:00:00Z',
        'parentId': parent, 'type': kind, 'data': {}}}})

state_file = os.path.join(os.path.dirname(pid_file), 'fixture-sessions.json') if pid_file else None
sessions = set(json.load(open(state_file))) if state_file and os.path.exists(state_file) else set()
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            if child:
                child.terminate()
                child.wait()
            sys.exit(0)
        if line in (b'\r\n', b'\n'):
            break
        key, value = line.decode().split(':', 1)
        headers[key.lower()] = value.strip()
    req = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    method = req.get('method')
    if 'id' not in req:
        continue
    if mode == 'hang_start' and method in ('connect', 'ping'):
        time.sleep(60)
    if mode == 'malformed':
        sys.stdout.buffer.write(b'Content-Length: 1\r\n\r\n!')
        sys.stdout.buffer.flush()
        continue
    params = req.get('params') or {}
    error = None
    result = {}
    if method in ('connect', 'ping'):
        result = {'ok': True, 'version': 'fixture', 'protocolVersion': 999 if mode == 'bad_version' else 3,
                  'message': params.get('message'), 'timestamp': 1}
    elif method == 'status.get':
        result = {'version': 'fixture', 'protocolVersion': 3}
    elif method == 'auth.getStatus':
        result = {'isAuthenticated': mode != 'unauth', 'authType': 'fixture',
                  'statusMessage': 'synthetic-secret-must-never-be-logged'}
    elif method == 'models.list':
        result = {'models': []}
    elif method == 'account.getQuota':
        if mode == 'quota_unavailable':
            error = {'code': -32601, 'message': 'synthetic-secret-must-never-be-logged'}
        else:
            result = {'quotaSnapshots': {}}
    elif method == 'session.create':
        # Verify the POC didn't silently widen tools or skip permission callbacks.
        assert params.get('availableTools') == []
        assert params.get('requestPermission') or params.get('requestPermissions') or params.get('hasPermissionHandler')
        sid = params['sessionId']
        sessions.add(sid)
        if state_file:
            with open(state_file, 'w') as f:
                json.dump(sorted(sessions), f)
        event('unrelated-session', 'session.idle', 'unrelated-id')
        event(sid, 'session.start', 'fixture-start')
        event(sid, 'session.idle', 'fixture-idle', 'fixture-start')
        if mode == 'session_error':
            error = {'code': -32000, 'message': 'synthetic-secret-must-never-be-logged'}
        else:
            result = {'sessionId': sid}
    elif method == 'session.resume':
        sid = params['sessionId']
        if sid not in sessions:
            error = {'code': -32000, 'message': 'session missing'}
        else:
            result = {'sessionId': sid}
    elif method == 'session.abort':
        if mode == 'abort_timeout':
            continue
        result = {'success': True}
    elif method in ('session.detach', 'session.skills.reload', 'session.options.update'):
        result = {'success': True}
        if mode == 'detach_error' and method == 'session.detach':
            error = {'code': -32000, 'message': 'synthetic-secret-must-never-be-logged'}
    elif method == 'session.delete':
        sessions.discard(params['sessionId'])
        if state_file:
            with open(state_file, 'w') as f:
                json.dump(sorted(sessions), f)
    elif method == 'runtime.shutdown':
        if mode == 'stop_timeout':
            continue
    else:
        error = {'code': -32601, 'message': 'forbidden_or_unsupported_method'}
    response = {'jsonrpc': '2.0', 'id': req['id']}
    response['error' if error else 'result'] = error if error else result
    emit(response)
