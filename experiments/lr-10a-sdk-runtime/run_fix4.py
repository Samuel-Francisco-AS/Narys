#!/usr/bin/env python3
"""FIX-4 owned test/unauthenticated offline metadata runner; never A9/auth loader."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import tempfile

from measure import measure, identity
from run_fix2 import config_stat, digest

ROOT = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['metadata', 'rust-tests'])
    parser.add_argument('--cli', type=Path)
    parser.add_argument('--artifacts-dir', type=Path,
                        help='fresh owned directory for Rust logs/observations; historical files never overwritten')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    before = config_stat()  # stat ONLY; never configuration or credentials.
    if args.mode == 'metadata':
        if args.cli is None:
            parser.error('--cli required for metadata')
        result, code = measure(ROOT / 'target/debug/auth-network-probe',
                               'metadata', args.cli.resolve(), 25)
    else:
        # Single-threaded ownership worker precedes Cargo/test/server threads.
        with tempfile.TemporaryDirectory(prefix='narys-fix4-cargo-') as job:
            wrapper = Path(job) / 'runner.py'
            artifacts = (args.artifacts_dir or ROOT / 'evidence').absolute()
            artifacts.mkdir(parents=True, exist_ok=True, mode=0o700)
            log = artifacts / 'fix-4-rust-tests.txt'
            observations = artifacts / 'fix-4-gateway-observations.jsonl'
            permissions = artifacts / 'fix-4-permission-regression.jsonl'
            for path in (log, observations, permissions, args.output):
                if path.exists():
                    raise SystemExit('new evidence path already exists; preserve it or choose a fresh run')
            env = {'HOME': str(Path.home()), 'PATH': '/usr/bin', 'LANG': 'C',
                   'COPILOT_SKIP_CLI_DOWNLOAD': '1', 'CARGO_BUILD_JOBS': '2',
                   'RUSTC': '/usr/bin/rustc', 'RUSTDOC': '/usr/bin/rustdoc',
                   'FIX4_EVIDENCE': str(observations),
                   'FIX3_SECURITY_EVIDENCE': str(permissions)}
            wrapper.write_text('#!/usr/bin/python3\nimport json, subprocess\n'
                + f'with open({str(log)!r},"w") as log:\n'
                + f' code=subprocess.run(["/usr/bin/cargo","test","--offline","--locked","--","--test-threads=1"],cwd={str(ROOT)!r},env={env!r},stdout=log,stderr=subprocess.STDOUT).returncode\n'
                + 'print(json.dumps({"cargo_exit_code":code,"inference_calls":0,"real_provider":False}))\n'
                + 'raise SystemExit(code)\n')
            wrapper.chmod(0o700)
            result, code = measure(wrapper, 'metadata', Path('/unused'), 180)
    after = config_stat()
    absent = True
    for row in result.get('attributed_processes', []):
        try:
            absent &= identity(row['pid'])['start_ticks'] != row['start_ticks']
        except FileNotFoundError:
            pass
    result['fix4_provenance'] = {
        'phase': 'LR-10A FIX-4', 'mode': args.mode,
        'observed_at': datetime.now(timezone.utc).isoformat(),
        'config_stat_before': before, 'config_stat_after': after,
        'config_stat_unchanged': before == after, 'config_contents_read': False,
        'owned_identities_absent_after': absent,
        'sources_sha256': {p: digest(ROOT / p) for p in ['measure.py', 'boundary.py',
            'fix4_boundary.py', 'run_fix4.py', 'src/auth_network.rs',
            'src/bin/auth-network-probe.rs', 'src/bin/network-fixture.rs',
            'tests/auth_network.rs', 'tests/test_fix4.py', 'Cargo.toml', 'Cargo.lock']},
        'binary_sha256': digest(ROOT / ('target/debug/auth-network-probe'
            if args.mode == 'metadata' else 'target/debug/network-fixture')),
        'cli_sha256': digest(args.cli.resolve()) if args.mode == 'metadata' else None,
        'inference_requests': 0, 'real_tokens_loaded': False, 'a9': 'BLOCKED_REAL',
        'driver_exit_code': code,
    }
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'mode': args.mode, 'driver_exit_code': code,
                     'cleanup_complete': result.get('cleanup_complete'),
                     'config_stat_unchanged': before == after,
                     'owned_identities_absent_after': absent}))
    return code if before == after and absent else 2


if __name__ == '__main__':
    raise SystemExit(main())
