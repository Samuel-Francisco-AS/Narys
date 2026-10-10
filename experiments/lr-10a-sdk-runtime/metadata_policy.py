"""Operation-scoped metadata observation, never a session/billing capability.

The CLI helper only stats HOME/.copilot/config.json. No content, token, session,
runtime or credential-service access. The historical classifier is unchanged.
"""
import json
import os
from pathlib import Path
import sys

import config_integrity as ci

OPERATIONS = frozenset(('METADATA_READ_ONLY', 'SESSION_OPERATIONS', 'INFERENCE',
                        'AGENT_ACTIONS', 'HEADLESS_OPERATION'))


def structural_check(row, uid):
    if not isinstance(row, dict) or row.get('state') != 'observed':
        return 'BLOCKED'
    meta = row.get('metadata')
    if (not isinstance(meta, dict) or set(meta) != ci.FIELDS
            or any(type(v) is not int or v < 0 for v in meta.values())
            or meta['uid'] != uid or meta['mode'] != 0o600 or meta['links'] != 1):
        return 'BLOCKED'
    return 'PASS_METADATA_ACCESS'


def observe(path):
    row = ci.snapshot(path, access_uid=os.getuid())
    return {'snapshot': row, 'structural_check': structural_check(row, os.getuid())}


def evaluate(phases, authenticated, *, service_ok, cleanup_ok):
    """Observational result only. No trusted=True/allow-drift/billing flag exists.

    Caller establishes origin separately. This pure classifier cannot certify a
    real SDK result, writer identity, bytes, or grant an operation capability.
    """
    structural = len(phases) >= 2 and all(structural_check(row, os.getuid()) ==
                                     'PASS_METADATA_ACCESS' for row in phases)
    comparisons = [ci.classify(a, b) for a, b in zip(phases, phases[1:])]
    drift = (any(r['metadata_mutation'] is True for r in comparisons)
             if comparisons and all(r['metadata_mutation'] is not None
                                    for r in comparisons) else None)
    observation = ('AUTH_TRUE_OBSERVED' if authenticated is True else
                   'AUTH_FALSE_OBSERVED' if authenticated is False else 'UNAVAILABLE')
    return {'auth_observation': observation,
            'CONFIG_STRUCTURAL_CHECK': 'PASS_METADATA_ACCESS' if structural else 'BLOCKED',
            'CONFIG_DRIFT_OBSERVED': drift,
            'CONFIG_WRITER_ATTRIBUTION': 'INCONCLUSIVE',
            'CONTENT_EQUIVALENCE': 'NOT_VERIFIED',
            'metadata_verification': ('OBSERVATION_ACCEPTED' if structural and
                service_ok is True and cleanup_ok is True and type(authenticated) is bool
                else 'BLOCKED'),
            'SESSION_ADMISSION': 'BLOCKED', 'FINANCIAL_ADMISSION': 'BLOCKED',
            'AGENT_ACTION_ADMISSION': 'BLOCKED', 'HEADLESS_AUTH': 'NOT_PROVEN',
            'REAL_INFERENCE': 'NOT_RUN', 'comparisons': comparisons}


def admission(operation, observation):
    """Not an executable dispatcher; metadata acceptance never transfers scope."""
    if operation not in OPERATIONS:
        return 'BLOCKED'
    if operation == 'METADATA_READ_ONLY' and isinstance(observation, dict):
        return ('METADATA_OBSERVATION_ONLY' if observation.get('metadata_verification')
                == 'OBSERVATION_ACCEPTED' else 'BLOCKED')
    return 'BLOCKED'


if __name__ == '__main__':
    if sys.argv[1:] != ['--snapshot']:
        raise SystemExit(2)
    home = os.environ.get('HOME')
    result = observe(Path(home) / '.copilot/config.json') if home else {
        'snapshot': {'state': 'failed', 'reason': 'home_unavailable'},
        'structural_check': 'BLOCKED'}
    print(json.dumps(result))
    raise SystemExit(0 if result['structural_check'] == 'PASS_METADATA_ACCESS' else 1)
