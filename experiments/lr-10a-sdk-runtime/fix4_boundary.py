#!/usr/bin/env python3
"""FIX-4 synthetic RPC ELF only. Reuse FIX-3 kernel policy without broad mounts."""
from pathlib import Path
import sys
# -I deliberately omits the script directory: add ONLY this trusted repo module.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import boundary


def plan(program, workspace, state, store, logs):
    root = Path(__file__).resolve().parent
    approved = root / 'target/debug/network-fixture'
    if Path(program) != approved or approved.is_symlink():
        raise boundary.BoundaryError('unapproved_fix4_fixture')
    # Base template first validates ownership, layout and all FIX-3 prerequisites.
    built = boundary.plan(root / 'target/debug/boundary-fixture', workspace, state, store, logs)
    native, libraries = boundary.native_dependencies(approved)
    if libraries != built['library_mounts']:
        raise boundary.BoundaryError('fix4_dependency_closure_changed')
    index = built['args'].index(str(root / 'target/debug/boundary-fixture'))
    built['args'][index] = str(native)
    # Select the existing strict SDK argv validator, not the fixture's normal/timeout argv.
    built['program_kind'] = 'sdk-cli'
    built['fixture_kind'] = 'fix4_synthetic_rpc'
    return built


def main():
    try:
        if len(sys.argv) < 8 or sys.argv[6] != '--':
            raise boundary.BoundaryError('invalid_fix4_launcher')
        built = plan(*sys.argv[1:6])
        boundary.execute(built, boundary.sdk_arguments(sys.argv[7:]))
    except (boundary.BoundaryError, OSError):
        print('fix4_boundary_blocked', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
