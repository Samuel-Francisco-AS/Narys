#!/usr/bin/env python3
"""Synthetic send/persistence peer ONLY. No provider/auth/network/tool execution."""
import json
import os
from pathlib import Path
import sys

root = Path(os.environ['A9_SYNTHETIC_ROOT'])
mode = os.environ.get('A9_SYNTHETIC_MODE', 'normal')
report = {'send_calls': 0, 'create_calls': 0, 'resume_calls': 0,
          'prompt_matches': False, 'zero_tools': True, 'request_permission': True,
          'instructions_disabled': True, 'unexpected_methods': 0}
store = root / 'synthetic-history.json'


def emit(payload):
    data = json.dumps(payload).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(data)).encode() + b'\r\n\r\n' + data)
    sys.stdout.buffer.flush()


def save():
    pending = root / 'summary.pending'
    pending.write_text(json.dumps(report))
    pending.replace(root / 'summary.json')


while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            save()
            sys.exit(0)
        if line in (b'\r\n', b'\n'):
            break
        k, v = line.decode().split(':', 1)
        headers[k.lower()] = v.strip()
    req = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    if 'id' not in req:
        continue
    method = req['method']
    p = req.get('params') or {}
    result, error = {}, None
    if method in ('connect', 'ping'):
        result = {'ok': True, 'version': 'synthetic', 'protocolVersion': 3,
                  'message': p.get('message'), 'timestamp': 1}
    elif method in ('session.create', 'session.resume'):
        report['create_calls' if method.endswith('create') else 'resume_calls'] += 1
        report['zero_tools'] &= p.get('availableTools') == []
        report['request_permission'] &= p.get('requestPermission') is True
        assert p.get('mcpServers') == {} and p.get('hooks') is False
        assert p.get('requestExtensions') is False and p.get('enableSkills') is False
        assert p.get('requestMcpApps') is False and p.get('pluginDirectories') == []
        sid = p['sessionId']
        if method == 'session.resume' and (not store.exists() or json.loads(store.read_text())['sid'] != sid):
            error = {'code': -32000, 'message': 'missing synthetic session'}
        else:
            result = {'sessionId': sid}
    elif method == 'session.options.update':
        report['instructions_disabled'] &= p.get('skipCustomInstructions') is True
        result = {'success': True}
    elif method == 'session.send':
        report['send_calls'] += 1
        report['prompt_matches'] = p.get('prompt') == ('Responda somente o número da soma de alpha=2 e beta=3. '
            'Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.')
        save()
        if mode == 'timeout':
            continue
        elif mode == 'error':
            error = {'code': -32000, 'message': 'synthetic-private-error-not-exported'}
        else:
            store.write_text(json.dumps({'sid':p['sessionId']}))
            result = {'messageId':'synthetic-message'}
    elif method == 'session.getMessages':
        result = {'events':[{'id':'synthetic-answer','parentId':None,'timestamp':'2026-10-09T00:00:00Z',
                            'type':'assistant.message','data':{'content':'5'}}]}
    elif method in ('session.detach','session.abort','runtime.shutdown','session.skills.reload'):
        result = {'success':True}
    else:
        report['unexpected_methods'] += 1
        error = {'code':-32601,'message':'synthetic forbidden method'}
    save()
    emit({'jsonrpc':'2.0','id':req['id'], 'error' if error else 'result':error if error else result})
