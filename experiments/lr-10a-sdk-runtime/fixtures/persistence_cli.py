#!/usr/bin/env python3
"""FIX-2 deterministic protocol/storage fixture, NEVER provider persistence proof.
No network/auth/config reads. All writes belong to the test's private directory.
Every unexpected RPC, especially inference/tool methods, records a violation.
"""
import json
import os
from pathlib import Path
import shutil
import sys

mode = os.environ.get('MOCK_MODE', 'persisted')
assert not any(key in os.environ for key in (
    'COPILOT_CLI_PATH', 'COPILOT_SDK_AUTH_TOKEN', 'GH_TOKEN', 'GITHUB_TOKEN'))
root = Path(os.environ['COPILOT_HOME']) / 'session-state'
manifest = Path(os.environ['MOCK_MANIFEST'])
fields = Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()
with manifest.open('a') as stream:
    stream.write(json.dumps({'pid': os.getpid(), 'start_ticks': int(fields[19])}) + '\n')
active = set()
detaches = set()


def emit(payload):
    body = json.dumps(payload, separators=(',', ':')).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
    sys.stdout.buffer.flush()


def stored(sid):
    file = root / sid / 'events.jsonl'
    if not file.exists():
        return False
    # These files are exclusively fixture-authored; never personal sessions.
    for line in file.read_text().splitlines():
        event = json.loads(line)
        assert event['type'] == 'session.start'
        assert event['data']['sessionId'] == sid
    return True


def start_event(sid):
    return {'id': 'fixture-start', 'timestamp': '2026-10-09T00:00:00Z',
            'parentId': None, 'type': 'session.start', 'data': {}}


while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            sys.exit(0)
        if line in (b'\r\n', b'\n'):
            break
        key, value = line.decode().split(':', 1)
        headers[key.lower()] = value.strip()
    req = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    method = req.get('method')
    if 'id' not in req:
        continue
    params = req.get('params') or {}
    sid = params.get('sessionId')
    error = None
    result = {}
    if method in ('connect', 'ping'):
        result = {'ok': True, 'version': 'fixture', 'protocolVersion': 3,
                  'message': params.get('message'), 'timestamp': 1}
    elif method == 'status.get':
        result = {'version': 'fixture', 'protocolVersion': 3}
    elif method == 'auth.getStatus':
        result = {'isAuthenticated': True, 'authType': 'fixture',
                  'statusMessage': 'synthetic-secret-must-never-be-logged'}
    elif method == 'session.create':
        assert params.get('availableTools') == []
        assert params.get('requestPermission') or params.get('requestPermissions') or params.get('hasPermissionHandler')
        directory = root / sid
        if mode == 'storage_unavailable':
            error = {'code': -32070, 'message': 'synthetic-secret-storage-error'}
        else:
            directory.mkdir(parents=True, exist_ok=True)
            (directory / 'workspace.yaml').write_text('fixture only\n')
            active.add(sid)
            if mode != 'empty':
                event = start_event(sid)
                event['data']['sessionId'] = sid
                (directory / 'events.jsonl').write_text(json.dumps(event) + '\n')
            result = {'sessionId': sid, 'workspacePath': str(directory)}
    elif method == 'session.getMessages':
        result = {'events': [start_event(sid)]}
    elif method == 'session.getMetadata':
        if mode == 'metadata_unavailable':
            error = {'code': -32601, 'message': 'synthetic-secret-metadata-error'}
        else:
            present = (root / sid / 'events.jsonl').exists()
            result = {'session': {'sessionId': sid, 'startTime': 'fixture',
                'modifiedTime': 'fixture', 'isRemote': False} if present else None}
    elif method == 'session.resume':
        assert params.get('availableTools') == []
        assert params.get('allowTranscriptRecovery') is False
        try:
            present = sid in active or stored(sid)
        except (ValueError, AssertionError):
            present = False
            error = {'code': -32603 if mode == 'corrupt_legacy' else -32075,
                     'message': 'synthetic-secret-corrupt-transcript'}
        if not error:
            if not present:
                error = {'code': -32000, 'message': 'Session not found: synthetic-secret'}
            else:
                active.add(sid)
                result = {'sessionId': 'wrong-owned-fixture-id' if mode == 'id_mismatch' else sid}
    elif method == 'session.abort':
        result = {'success': True}
    elif method == 'session.detach':
        if mode == 'detach_error' or (mode == 'late_disconnect' and sid in detaches):
            error = {'code': -32000, 'message': 'Session not found: synthetic-secret'}
        active.discard(sid)
        detaches.add(sid)
        result = {'success': True}
    elif method == 'session.delete':
        active.discard(sid)
        shutil.rmtree(root / sid, ignore_errors=True)
    elif method in ('runtime.shutdown', 'session.skills.reload', 'session.options.update'):
        result = {'success': True}
    else:
        with manifest.with_suffix('.violations').open('a') as stream:
            stream.write('forbidden_rpc\n')
        error = {'code': -32601, 'message': 'forbidden_or_unsupported_method'}
    response = {'jsonrpc': '2.0', 'id': req['id']}
    response['error' if error else 'result'] = error if error else result
    emit(response)
