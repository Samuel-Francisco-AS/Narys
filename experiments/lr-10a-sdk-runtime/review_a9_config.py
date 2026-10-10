#!/usr/bin/env python3
"""Offline A9-FIX-3 review of pinned published evidence; never a runtime probe."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path

from config_integrity import classify, historical_snapshot, independent_observations

ROOT = Path(__file__).resolve().parent
HISTORY = ROOT / 'evidence/a9-fix-2-gui-sdk-metadata.json'
HISTORY_SHA = '1b76b3f4dfdacec1835ba774402897c202813c238c23b17a38500efd77b70916'
BASE = '4a75c632fc8e69fcd9511cd447489b3c1d3d5fff'


def review():
    raw = HISTORY.read_bytes()  # Git evidence, never personal config.
    if hashlib.sha256(raw).hexdigest() != HISTORY_SHA:
        raise ValueError('historical_evidence_identity_mismatch')
    history = json.loads(raw)
    config = classify(historical_snapshot(history['config_stat_before']),
                      historical_snapshot(history['config_stat_after']))
    auth = history['sdk_metadata']['sdk_report']['preflight']['auth']['authenticated']
    result = independent_observations(auth, config)
    result.update(phase='LR-10A A9-FIX-3', base=BASE, schema_version=1,
                  kind='offline_historical_reclassification_not_live_retest',
                  historical_source=HISTORY.name, historical_source_sha256=HISTORY_SHA,
                  historical_observed_at=history['observed_at'],
                  historical_implementation_commit='42756450fb8b942fdc2f7ec42b6ece364f3e5c73',
                  sdk_authenticated_with_gui='OBSERVED_REAL_PASS' if auth is True else 'NOT_PROVEN',
                  sdk_authenticated_headless='NOT_PROVEN',
                  config_mutation_observed=config['metadata_mutation'],
                  config_writer_attribution=config['writer_attribution'],
                  config_integrity_verification=config['integrity_verification'],
                  a9_attempt_marker='NOT_CLAIMED_NO_CONTENT_READ',
                  current_cli_invocations=0, current_authenticated_probes=0,
                  current_personal_config_observations=0,
                  process_cleanup='NO_RUNTIME_STARTED_THIS_REVIEW',
                  recovery_actions_performed=[],
                  next_real_observation='PENDING_USER_AUTHORIZATION')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = review()
    result['reviewed_at'] = datetime.now(timezone.utc).isoformat()
    result['source_sha256'] = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
                               for name in ('config_integrity.py', 'review_a9_config.py')}
    fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'review_completed': True, 'config_integrity_verification':
                     result['config_integrity_verification'], 'current_cli_invocations': 0}))
    return 0  # Offline review succeeded, explicitly not operational admission.


if __name__ == '__main__':
    raise SystemExit(main())
