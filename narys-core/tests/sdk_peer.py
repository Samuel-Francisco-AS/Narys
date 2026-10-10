#!/usr/bin/python3
"""Synthetic JSON-RPC peer. No credentials, network, provider or tools."""
import json, os, sys
from pathlib import Path
root = Path(os.environ['SYNTHETIC_ROOT'])
mode = os.environ['SYNTHETIC_MODE']
summary = {'send': 0, 'abort': 0, 'detach': 0, 'zero_tools': False, 'create':0, 'resume':0, 'history':0}
def emit(data):
    body = json.dumps(data).encode()
    sys.stdout.buffer.write(b'Content-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body)
    sys.stdout.buffer.flush()
def event(sid, kind, data):
    emit({'jsonrpc':'2.0','method':'session.event','params':{'sessionId':sid,
          'event':{'id':kind,'parentId':None,'timestamp':'2026-10-10T00:00:00Z','type':kind,'data':data}}})
while True:
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line: (root/'summary.json').write_text(json.dumps(summary)); sys.exit(0)
        if line in (b'\r\n', b'\n'): break
        k,v = line.decode().split(':',1); headers[k.lower()] = v.strip()
    req = json.loads(sys.stdin.buffer.read(int(headers['content-length'])))
    if 'id' not in req: continue
    method, p = req['method'], req.get('params') or {}
    result = {'success':True}
    if method in ('connect','ping'): result = {'ok':True,'version':'1.0.95','protocolVersion':3,'timestamp':1}
    elif method == 'status.get': result = {'version':'1.0.95','protocolVersion':3}
    elif method == 'auth.getStatus': result = {'isAuthenticated':True}
    elif method == 'models.list': result = {'models':[{'id':'auto','name':'synthetic','capabilities':{}}]}
    elif method == 'account.getQuota': result = {'quotaSnapshots':{'premium_interactions':{
        'entitlementRequests':1,'isUnlimitedEntitlement':False,'overage':0,
        'overageAllowedWithExhaustedQuota':False,'remainingPercentage':100,
        'usageAllowedWithExhaustedQuota':False,'usedRequests':0}}}
    elif method == 'session.create':
        summary['create'] += 1
        if mode == 'create_error':
            emit({'jsonrpc':'2.0','id':req['id'],'error':{'code':-32602,'message':'synthetic-private-secret'}})
            continue
        summary['zero_tools'] = (p.get('availableTools') == [] and p.get('requestPermission') is True
            and p.get('mcpServers') == {} and p.get('hooks') is False and p.get('enableSkills') is False)
        assert summary['zero_tools']
        assert p.get('configDir') == str(root/'session-state')
        result = {'sessionId':p['sessionId'],'workspacePath':str(root/'workspace')}
    elif method == 'session.resume':
        summary['resume'] += 1
        assert p.get('availableTools') == [] and p.get('mcpServers') == {} and p.get('hooks') is False
        result = {'sessionId':p['sessionId'],'workspacePath':str(root/'workspace')}
    elif method == 'session.getMessages':
        summary['history'] += 1
        result={'events':[{'id':'synthetic-message','parentId':None,'timestamp':'2026-10-10T00:00:00Z','type':'assistant.message','data':{'content':'5'}}]}
    elif method == 'session.send':
        summary['send'] += 1
        result = {'messageId':'synthetic'}
        emit({'jsonrpc':'2.0','id':req['id'],'result':result})
        if mode == 'normal':
            event(p['sessionId'],'assistant.message',{'content':'5'})
            event(p['sessionId'],'session.idle',{})
        elif mode == 'cancel': (root/'cancel').touch(mode=0o600)
        continue
    elif method == 'session.abort': summary['abort'] += 1
    elif method == 'session.detach': summary['detach'] += 1
    elif method == 'session.options.update' and mode == 'options_error':
        emit({'jsonrpc':'2.0','id':req['id'],'error':{'code':-32602,'message':'synthetic-private-secret'}})
        continue
    elif method not in ('session.options.update','runtime.shutdown','session.skills.reload'):
        emit({'jsonrpc':'2.0','id':req['id'],'error':{'code':-32601,'message':'synthetic'}});continue
    emit({'jsonrpc':'2.0','id':req['id'],'result':result})
