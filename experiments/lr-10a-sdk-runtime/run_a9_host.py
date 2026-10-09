#!/usr/bin/env python3
"""Owned HOST-ASSISTED preflight only; no live send in this blocked candidate."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import stat

from measure import measure, identity
from run_fix2 import config_stat, digest

ROOT = Path(__file__).resolve().parent
CLI_SHA = 'be17b42705ca17490098d7b87f293300d72a094d125b6bb2b2557dc4a0a4f8a8'


def marker_directory(home):
    """Create only the authorized private state directory; reject symlinks.

    Every child is opened relative to a pinned directory FD. Existing narys
    permissions are verified, never silently changed. Never read an attempt file.
    """
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    fd = os.open(home, flags)
    try:
        for name in ('.local', 'state', 'narys'):
            try:
                os.mkdir(name, 0o700, dir_fd=fd)
            except FileExistsError:
                pass
            child = os.open(name, flags, dir_fd=fd)
            meta = os.fstat(child)
            if meta.st_uid != os.getuid() or (name == 'narys' and stat.S_IMODE(meta.st_mode) != 0o700):
                os.close(child)
                raise ValueError('unsafe_marker_directory')
            os.close(fd)
            fd = child
        try:
            os.stat('lr10a-a9-host-attempt.json', dir_fd=fd, follow_symlinks=False)
            claimed = True  # Any entry/corruption blocks, no contents inspected.
        except FileNotFoundError:
            claimed = False
        return claimed
    finally:
        os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cli', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.is_symlink():
        raise SystemExit('Evidence already exists; never overwrite it')
    cli = args.cli.resolve(strict=True)
    if digest(cli) != CLI_SHA:
        raise SystemExit('Pinned native CLI required; no runtime replacement')
    before = config_stat()
    attempted = marker_directory(Path.home())
    if attempted:
        result, code = {'gate':'A9_HOST_ASSISTED','state':'BLOCKED_PRE_SEND',
                        'code':'attempt_marker_exists','sdk_send_calls':0}, 1
    else:
        # Process-local allowlist, NOT a host sandbox. Normal CLI may use its
        # existing account/keyring. Values of credentials are never acquired.
        allowed = {k: os.environ[k] for k in ('HOME', 'DBUS_SESSION_BUS_ADDRESS',
                    'XDG_RUNTIME_DIR') if k in os.environ}
        allowed.update({'PATH':'/usr/bin', 'LANG':'C',
                        'COPILOT_SKIP_CLI_DOWNLOAD':'1'})
        os.environ.clear()
        os.environ.update(allowed)
        result, code = measure(ROOT/'target/debug/a9-host-assisted',
                               'preflight', cli, 65)
    after = config_stat()
    absent = True
    for row in result.get('attributed_processes', []):
        try:
            absent &= identity(row['pid'])['start_ticks'] != row['start_ticks']
        except FileNotFoundError:
            pass
    result['host_assisted_provenance'] = {
        'gate':'A9_HOST_ASSISTED','base':'4216ba2a6a0e75822587bf9032782ddc454338fa',
        'observed_at':datetime.now(timezone.utc).isoformat(),
        'cli_sha256':digest(cli), 'binary_sha256':digest(ROOT/'target/debug/a9-host-assisted'),
        'sources_sha256':{p:digest(ROOT/p) for p in ('src/host_assisted.rs',
            'src/bin/a9-host-assisted.rs','run_a9_host.py','measure.py','Cargo.toml','Cargo.lock')},
        'config_stat_before':before,'config_stat_after':after,
        'config_stat_unchanged':before==after,'config_contents_read':False,
        'owned_identities_absent_after':absent,'marker_present_before':attempted,
        'marker_present_after':marker_directory(Path.home()),
        'marker_directory_mode':'0700','marker_file_claimed':False,
        'mode':'HOST_ASSISTED_NOT_SANDBOX','sdk_send_calls':0,'driver_exit_code':code}
    with args.output.open('x') as output:
        output.write(json.dumps(result, indent=2)+'\n')
    print(json.dumps({'evidence':str(args.output),'state':'BLOCKED_PRE_SEND',
                      'sdk_send_calls':0,'cleanup_complete':result.get('cleanup_complete'),
                      'config_stat_unchanged':before==after}))
    return code if before==after and absent else 2


if __name__ == '__main__':
    raise SystemExit(main())
