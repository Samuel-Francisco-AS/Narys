#!/usr/bin/env python3
"""Synthetic SDK + Python regressions inside the unchanged FIX-1 ownership worker."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import tempfile

from measure import measure
from run_fix2 import digest

ROOT = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts-dir', type=Path, required=True)
    args = parser.parse_args()
    out = args.artifacts_dir.absolute()
    out.mkdir(mode=0o700, parents=True, exist_ok=True)
    for name in ('a9-host-rust-tests.txt', 'a9-host-python-tests.txt',
                 'a9-host-owned-tests.json', 'a9-host-test-gateway.jsonl',
                 'a9-host-test-permissions.jsonl'):
        if (out / name).exists():
            raise SystemExit('Fresh evidence paths required')
    env = {'HOME':str(Path.home()),'PATH':'/usr/bin','LANG':'C',
           'COPILOT_SKIP_CLI_DOWNLOAD':'1','CARGO_BUILD_JOBS':'2',
           'RUSTC':'/usr/bin/rustc','RUSTDOC':'/usr/bin/rustdoc',
           'FIX4_EVIDENCE':str(out/'a9-host-test-gateway.jsonl'),
           'FIX3_SECURITY_EVIDENCE':str(out/'a9-host-test-permissions.jsonl')}
    commands = [['/usr/bin/cargo','test','--offline','--locked','--','--test-threads=1'],
                ['/usr/bin/python3','-m','unittest','discover','-s','tests','-p','test_*.py','-v']]
    with tempfile.TemporaryDirectory(prefix='narys-a9-tests-') as tmp:
        wrapper = Path(tmp)/'test-runner.py'
        wrapper.write_text('#!/usr/bin/python3\nimport json,subprocess\ncodes=[]\n'
            + ''.join(f'with open({str(out/name)!r},"x") as log:\n codes.append(subprocess.run({cmd!r},cwd={str(ROOT)!r},env={env!r},stdout=log,stderr=subprocess.STDOUT).returncode)\n'
                for name,cmd in zip(('a9-host-rust-tests.txt','a9-host-python-tests.txt'), commands))
            + 'print(json.dumps({"test_exit_codes":codes,"real_inference_calls":0,"real_cli_started":False}))\nraise SystemExit(int(any(codes)))\n')
        wrapper.chmod(0o700)
        report, code = measure(wrapper,'synthetic-tests',Path('/unused'),180)
    report['provenance'] = {'gate':'A9_HOST_ASSISTED','kind':'synthetic_and_kernel_regressions',
        'observed_at':datetime.now(timezone.utc).isoformat(),'commands':commands,
        'source_sha256':{p:digest(ROOT/p) for p in ('src/host_assisted.rs',
            'src/bin/a9-host-assisted.rs','tests/host_assisted.rs','fixtures/a9_host_cli.py',
            'tests/test_a9_host.py','run_a9_host.py','verify_a9_host.py','Cargo.toml','Cargo.lock')},
        'real_inference_calls':0,'driver_exit_code':code}
    (out/'a9-host-owned-tests.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'evidence':str(out),'exit':code,'cleanup_complete':report.get('cleanup_complete')}))
    return code


if __name__ == '__main__':
    raise SystemExit(main())
