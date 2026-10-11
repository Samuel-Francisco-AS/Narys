#!/usr/bin/env python3
"""Pinned native metadata ONLY, offline sandbox, synthetic HOME, no session/send.

The only RPCs are ping/status.get/tools.list. Rejects a changed CLI checksum.
This does not invoke model inference, execute a tool or access authentication.
"""
import argparse
import hashlib
import json
import pathlib
import subprocess
import tempfile
import time

PIN = '9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99'
DRIVER = r'''
import json, subprocess, sys
p = subprocess.Popen(['/copilot', '--server', '--stdio', '--no-auto-update', '--disable-builtin-mcps', '--no-custom-instructions', '--log-level', 'none'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
results = {}
try:
    for i, method in enumerate(['ping', 'status.get', 'tools.list']):
        body = json.dumps({'jsonrpc':'2.0','id':i+1,'method':method,'params':{}}).encode()
        p.stdin.write(b'Content-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body); p.stdin.flush()
        while True:
            headers = {}
            while True:
                line = p.stdout.readline()
                if not line: raise RuntimeError('metadata transport closed')
                if line in (b'\r\n', b'\n'): break
                key, value = line.decode().split(':', 1); headers[key.lower()] = value.strip()
            size = int(headers['content-length'])
            if size > 4*1024*1024: raise RuntimeError('metadata size limit')
            message = json.loads(p.stdout.read(size))
            if message.get('id') == i+1:
                results[method] = message.get('result', {'error': message.get('error')}); break
    print(json.dumps(results))
finally:
    p.stdin.close()
    try: p.wait(timeout=2)
    except subprocess.TimeoutExpired: p.kill(); p.wait()
'''

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=pathlib.Path, required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    digest = hashlib.sha256(cli.read_bytes()).hexdigest()
    if digest != PIN:
        raise SystemExit('CLI pin mismatch')
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='lr10c-inventory-') as workspace:
        cmd = ['/usr/bin/bwrap', '--unshare-all', '--unshare-user', '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--disable-userns', '--assert-userns-disabled',
               '--ro-bind','/usr','/usr','--symlink','usr/bin','/bin','--symlink','usr/lib64','/lib64','--symlink','usr/lib','/lib',
               '--proc','/proc','--dev','/dev','--tmpfs','/tmp','--tmpfs','/home','--tmpfs','/run','--bind',workspace,'/workspace',
               '--ro-bind',str(cli),'/copilot','--clearenv','--setenv','HOME','/home','--setenv','PATH','/usr/bin','--chdir','/workspace',
               '--remount-ro','/','--','/usr/bin/python3','-I','-c',DRIVER]
        result = subprocess.run(cmd, env={}, capture_output=True, timeout=25, check=True)
    raw = json.loads(result.stdout)
    tools = raw.get('tools.list', {}).get('tools', [])
    # Persist descriptors, not logs/events/authentication. No model request.
    report = {'cli_sha256':digest,'cli_version':raw.get('status.get',{}).get('version'),
              'rpc_allowlist':['ping','status.get','tools.list'],'authenticated':False,'network_namespace':'unshared',
              'host_home_mounted':False,'inference_requests':0,'tool_executions':0,
              'elapsed_seconds':round(time.monotonic()-start,4),'tools':tools,
              'tools_list_error':raw.get('tools.list',{}).get('error')}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'version':report['cli_version'],'tools':len(tools),'error':report['tools_list_error'],'elapsed_seconds':report['elapsed_seconds']}))
if __name__ == '__main__':
    main()
