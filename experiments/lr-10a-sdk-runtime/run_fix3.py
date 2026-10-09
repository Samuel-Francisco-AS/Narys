#!/usr/bin/env python3
"""FIX-3 metadata/empty-session probes only; never inference or host auth."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path

from measure import measure
from run_fix2 import config_stat, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', choices=['metadata', 'sessions', 'metadata-existing-auth'])
    parser.add_argument('cli', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    binary = directory / 'target/debug/narys-lr10a-poc'
    before = config_stat()  # stat only, no credential/configuration contents.
    result, code = measure(binary, args.probe, args.cli.resolve(), 90)
    after = config_stat()
    result['fix3_provenance'] = {
        'phase': 'LR-10A FIX-3', 'observed_at': datetime.now(timezone.utc).isoformat(),
        'binary_sha256': digest(binary), 'cli_sha256': digest(args.cli.resolve()),
        'sources_sha256': {name: digest(directory / name) for name in
            ['src/main.rs', 'src/lib.rs', 'src/persistence.rs', 'src/boundary.rs',
             'boundary.py', 'measure.py', 'run_fix3.py', 'Cargo.toml', 'Cargo.lock']},
        'config_stat_before': before, 'config_stat_after': after,
        'config_stat_unchanged': before == after, 'config_contents_read': False,
        'host_credentials_exposed': False, 'host_network_shared': False,
        'inference_requests': 0, 'agentive_tool_requests': 0, 'driver_exit_code': code,
        'a9': 'BLOCKED', 'gates': ['BLOCKED_AUTH_BOUNDARY', 'BLOCKED_NETWORK_BOUNDARY',
                                 'BLOCKED_SUPERVISOR_FAILURE_CONTAINMENT', 'BLOCKED_REAL'],
    }
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'evidence': str(args.output), 'cleanup_complete': result.get('cleanup_complete'),
                     'config_stat_unchanged': before == after, 'driver_exit_code': code,
                     'a9': 'BLOCKED'}))
    return code if before == after else 2


if __name__ == '__main__':
    raise SystemExit(main())
