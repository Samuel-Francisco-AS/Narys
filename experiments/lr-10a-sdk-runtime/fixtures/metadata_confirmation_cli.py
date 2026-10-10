#!/usr/bin/python3
"""Local protocol peer only: strict metadata method set, no network/provider."""
import json
import os
from pathlib import Path
import sys

root = Path(os.environ['METADATA_FIXTURE_ROOT'])
mode = os.environ['METADATA_FIXTURE_MODE']
methods = []
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            (root / 'methods.json').write_text(json.dumps(methods))
            sys.exit(0)
        if line in (b'\r\n', b'\n'):
            break
        key, value = line.decode().split(':', 1)
        headers[key.lower()] = value.strip()
    request = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    if 'id' not in request:
        continue
    method = request['method']
    methods.append(method)
    response = {'jsonrpc': '2.0', 'id': request['id']}
    if method in ('connect', 'ping'):
        response['result'] = {'ok': True, 'version': '1.0.95', 'protocolVersion': 3, 'timestamp': 1}
    elif method == 'status.get':
        if mode == 'status_error':
            response['error'] = {'code': -32000, 'message': 'synthetic-private-prose'}
        else:
            response['result'] = {'version': '1.0.91' if mode == 'wrong_version' else '1.0.95',
                                  'protocolVersion': 3}
    elif method == 'auth.getStatus':
        response['result'] = {'isAuthenticated': mode != 'negative',
                              'login': 'synthetic-private-identity'}
    elif method == 'runtime.shutdown':
        response['result'] = {'success': True}
    else:
        response['error'] = {'code': -32601, 'message': 'forbidden synthetic method'}
    data = json.dumps(response).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(data)).encode() + b'\r\n\r\n' + data)
    sys.stdout.buffer.flush()
