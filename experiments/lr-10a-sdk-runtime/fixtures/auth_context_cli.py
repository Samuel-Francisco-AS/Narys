#!/usr/bin/python3
"""Metadata-only synthetic RPC. A context boolean is NOT real authentication."""
import json
import os
from pathlib import Path
import sys

root = Path(os.environ['A9_AUTH_FIXTURE_ROOT'])
report = {'session_operations': 0, 'send_calls': 0, 'unexpected_methods': 0}


def emit(request_id, result=None, error=None):
    value = {'jsonrpc': '2.0', 'id': request_id,
             'error' if error else 'result': error if error else result}
    data = json.dumps(value).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(data)).encode() + b'\r\n\r\n' + data)
    sys.stdout.buffer.flush()


while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            (root / 'metadata-summary.json').write_text(json.dumps(report))
            sys.exit(0)
        if line in (b'\r\n', b'\n'):
            break
        key, value = line.decode().split(':', 1)
        headers[key.lower()] = value.strip()
    request = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    if 'id' not in request:
        continue
    method = request['method']
    authenticated = (os.environ.get('A9_PUBLIC_AUTH_CONTEXT') == 'available'
                     and '--no-auto-login' not in sys.argv)
    if method in ('connect', 'ping'):
        emit(request['id'], {'ok': True, 'version': 'synthetic',
                             'protocolVersion': 3, 'timestamp': 1})
    elif method == 'auth.getStatus':
        emit(request['id'], {'isAuthenticated': authenticated,
                             'login': 'synthetic-identity-must-not-export'})
    elif method == 'models.list' and authenticated:
        emit(request['id'], {'models': [{'id': 'synthetic-zero', 'name': 'fixture',
            'capabilities': {}, 'billing': {'multiplier': 0}}]})
    elif method == 'account.getQuota' and authenticated:
        emit(request['id'], {'quotaSnapshots': {'premium_interactions': {
            'entitlementRequests': 1, 'isUnlimitedEntitlement': False,
            'overage': 0, 'overageAllowedWithExhaustedQuota': False,
            'remainingPercentage': 100, 'usageAllowedWithExhaustedQuota': False,
            'usedRequests': 0}}})
    elif method == 'runtime.shutdown':
        emit(request['id'], {'success': True})
    else:
        report['unexpected_methods'] += int(method not in ('models.list', 'account.getQuota'))
        report['session_operations'] += int(method.startswith('session.'))
        report['send_calls'] += int(method == 'session.send')
        emit(request['id'], error={'code': -32603,
            'message': 'public-synthetic-marker-not-for-evidence'})
