#!/usr/bin/env python3
"""Synthetic SDK permission broadcasts only. No tools/inference/network/auth."""
import json
import os
from pathlib import Path
import sys

summary = Path(os.environ['MOCK_SUMMARY'])
report = {'configurations': [], 'decisions': [], 'forbidden_methods': 0}

def save():
    summary.write_text(json.dumps(report))

def emit(payload):
    body = json.dumps(payload, separators=(',', ':')).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
    sys.stdout.buffer.flush()

def permission(sid, kind, index):
    emit({'jsonrpc': '2.0', 'method': 'session.event', 'params': {
        'sessionId': sid, 'event': {'id': 'synthetic-' + str(index), 'parentId': None,
        'timestamp': '2026-10-09T00:00:00Z', 'type': 'permission.requested',
        'data': {'requestId': 'synthetic-' + str(index), 'permissionRequest': {
            'kind': kind, 'managedApprovalRequired': index == 2,
            'tool': kind, 'arguments': 'synthetic-secret-must-not-be-exported'}}}}})

while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            save()
            sys.exit(0)
        if line in (b'\n', b'\r\n'):
            break
        key, value = line.decode().split(':', 1)
        headers[key.lower()] = value.strip()
    req = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    if 'id' not in req:
        continue
    method = req['method']; params = req.get('params') or {}
    result = {}; error = None
    if method in ('connect', 'ping'):
        result = {'ok': True, 'protocolVersion': 3, 'version': 'fixture',
                  'message': params.get('message'), 'timestamp': 1}
    elif method in ('session.create', 'session.resume'):
        # Export policy booleans/counts only, never request payloads or identities.
        report['configurations'].append({
            'operation': method, 'request_permission': params.get('requestPermission'),
            'zero_tools': params.get('availableTools') == [],
            'empty_mcp_servers': params.get('mcpServers') == {},
            'mcp_apps_disabled': params.get('requestMcpApps') is False,
            'config_discovery_disabled': params.get('enableConfigDiscovery') is False,
            'extensions_disabled': params.get('requestExtensions') is False,
            'hooks_disabled': params.get('hooks') is False,
            'plugins_empty': params.get('pluginDirectories') == [],
            'skills_disabled': params.get('enableSkills') is False,
            'file_hooks_disabled': params.get('enableFileHooks') is False,
            'host_git_disabled': params.get('enableHostGitOperations') is False,
            'instructions_skipped': False,
            'installed_plugins_empty': False,
            'builtin_skills_empty': False,
            'instruction_discovery_disabled': params.get('enableOnDemandInstructionDiscovery') is False,
            'additional_directories_empty': params.get('additionalDirectories') == []})
        save()
        sid = params['sessionId']
        result = {'sessionId': sid}
        for index, kind in enumerate(['shell', 'write', 'future-unknown-tool']):
            permission(sid, kind, index)
    elif method == 'session.permissions.handlePendingPermissionRequest':
        report['decisions'].append(params.get('result', {}).get('kind', 'invalid'))
        save()
    elif method == 'session.options.update':
        report['configurations'][-1]['instructions_skipped'] = params.get('skipCustomInstructions') is True
        report['configurations'][-1]['installed_plugins_empty'] = params.get('installedPlugins') == []
        report['configurations'][-1]['builtin_skills_empty'] = params.get('includedBuiltinSkills') == []
        save()
        if os.environ.get('MOCK_REJECT_OPTIONS') == '1':
            error = {'code': -32601, 'message': 'unsupported_security_option'}
        else:
            result = {'success': True}
    elif method in ('session.detach', 'session.skills.reload', 'runtime.shutdown'):
        result = {'success': True}
    else:
        report['forbidden_methods'] += 1; save()
        error = {'code': -32601, 'message': 'forbidden_method'}
    response = {'jsonrpc': '2.0', 'id': req['id']}
    response['error' if error else 'result'] = error if error else result
    emit(response)
