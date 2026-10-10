"""Operation-scoped fixtures only, no actual Copilot or personal configuration."""
import copy
import errno
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import config_integrity as ci
import metadata_policy as policy
import confirm_a9_metadata as driver


class MetadataPolicyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='narys-operation-policy-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.file = self.root / 'synthetic-config'
        self.file.write_bytes(b'AAAA')
        self.file.chmod(0o600)
        self.before = policy.observe(self.file)['snapshot']

    def evaluate(self, after=None, auth=True, cleanup=True, services=True):
        return policy.evaluate([self.before, after or policy.observe(self.file)['snapshot']],
            auth, service_ok=services, cleanup_ok=cleanup)

    def record(self, name, result):
        print(json.dumps({'a9_fix4_synthetic_case': name, 'evaluation': result}))

    def test_stable_only_accepts_metadata_not_content_or_operations(self):
        r = self.evaluate()
        self.assertFalse(r['CONFIG_DRIFT_OBSERVED'])
        self.assertEqual(r['CONTENT_EQUIVALENCE'], 'NOT_VERIFIED')
        self.assertEqual(r['metadata_verification'], 'OBSERVATION_ACCEPTED')
        self.record('stable', r)

    def test_equal_size_atomic_replacement_preserves_auth_observation_with_drift(self):
        replacement = self.root / 'replacement'
        replacement.write_bytes(b'BBBB')
        replacement.chmod(0o600)
        os.replace(replacement, self.file)
        r = self.evaluate()
        self.assertTrue(r['CONFIG_DRIFT_OBSERVED'])
        self.assertEqual(r['auth_observation'], 'AUTH_TRUE_OBSERVED')
        self.assertEqual(r['metadata_verification'], 'OBSERVATION_ACCEPTED')
        self.assertEqual(r['CONFIG_WRITER_ATTRIBUTION'], 'INCONCLUSIVE')
        self.record('atomic_equal_size_auth_drift', r)

    def test_size_and_timestamp_change_recorded_without_content_claim(self):
        for field in ('bytes', 'mtime_ns', 'ctime_ns'):
            after = copy.deepcopy(self.before)
            after['metadata'][field] += 100
            r = self.evaluate(after)
            self.assertTrue(r['CONFIG_DRIFT_OBSERVED'])
            self.assertEqual(r['metadata_verification'], 'OBSERVATION_ACCEPTED')
            self.record(field, r)

    def test_negative_auth_stable_is_not_positive_auth(self):
        r = self.evaluate(auth=False)
        self.assertEqual(r['auth_observation'], 'AUTH_FALSE_OBSERVED')
        self.assertEqual(r['metadata_verification'], 'OBSERVATION_ACCEPTED')
        self.record('negative_auth_stable', r)

    def test_unsafe_symlink_parent_leaf_and_traversal_block(self):
        link = self.root / 'link'
        link.symlink_to(self.file)
        parent = self.root / 'parent'
        parent.symlink_to(self.root)
        for path in (link, parent / self.file.name, self.root / '..' / self.file.name):
            r = self.evaluate(policy.observe(path)['snapshot'])
            self.assertEqual(r['metadata_verification'], 'BLOCKED')
            self.assertEqual(r['CONFIG_STRUCTURAL_CHECK'], 'BLOCKED')
        self.record('unsafe_paths', r)

    def test_missing_access_denied_or_interruption_blocks_without_private_prose(self):
        rows = [policy.observe(self.root / 'missing')['snapshot']]
        for error in (PermissionError(errno.EACCES, 'synthetic-private-prose'), InterruptedError()):
            with mock.patch.object(ci.os, 'stat', side_effect=error):
                rows.append(policy.observe(self.file)['snapshot'])
        for row in rows:
            r = self.evaluate(row)
            self.assertEqual(r['metadata_verification'], 'BLOCKED')
            self.assertNotIn('synthetic-private-prose', json.dumps(r))
            self.record('unavailable_' + row['state'], r)

    def test_mode_owner_hardlink_and_parent_permissions_fail_closed(self):
        for field, value in [('uid', os.getuid() + 1), ('mode', 0o644), ('links', 2)]:
            after = copy.deepcopy(self.before)
            after['metadata'][field] = value
            self.assertEqual(self.evaluate(after)['metadata_verification'], 'BLOCKED')
        self.file.chmod(0o644)
        self.assertEqual(policy.observe(self.file)['structural_check'], 'BLOCKED')
        self.file.chmod(0o600)
        self.root.chmod(0o777)
        self.assertEqual(policy.observe(self.file)['structural_check'], 'BLOCKED')
        self.root.chmod(0o700)

    def test_concurrent_unknown_writer_drift_is_observation_not_legitimate_write(self):
        child = subprocess.Popen([sys.executable, '-c',
            'import pathlib,sys;sys.stdin.buffer.read(1);pathlib.Path(sys.argv[1]).write_bytes(b"BBBBB")',
            str(self.file)], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            env={'PATH': '/usr/bin'})
        child.communicate(b'x', timeout=3)
        self.assertEqual(child.returncode, 0)
        r = self.evaluate()
        self.assertTrue(r['CONFIG_DRIFT_OBSERVED'])
        self.assertEqual(r['CONFIG_WRITER_ATTRIBUTION'], 'INCONCLUSIVE')
        self.record('concurrent_unknown_writer', r)

    def test_cleanup_and_service_failure_preserve_auth_but_block_verification(self):
        for kwargs in ({'cleanup':False}, {'services':False}):
            r = self.evaluate(**kwargs)
            self.assertEqual(r['auth_observation'], 'AUTH_TRUE_OBSERVED')
            self.assertEqual(r['metadata_verification'], 'BLOCKED')
            self.record('failed_dependency', r)

    def test_metadata_acceptance_cannot_transfer_to_session_send_finance_or_headless(self):
        r = self.evaluate()
        self.assertEqual(policy.admission('METADATA_READ_ONLY', r), 'METADATA_OBSERVATION_ONLY')
        for op in policy.OPERATIONS - {'METADATA_READ_ONLY'} | {'send', 'unknown'}:
            self.assertEqual(policy.admission(op, r), 'BLOCKED')
        for field in ('FINANCIAL_ADMISSION', 'SESSION_ADMISSION', 'AGENT_ACTION_ADMISSION'):
            self.assertEqual(r[field], 'BLOCKED')
        with self.assertRaises(TypeError):
            policy.evaluate([self.before], True, service_ok=True, cleanup_ok=True, allow_paid=True)
        forged = dict(r, FINANCIAL_ADMISSION='PASS', allow_paid=True, fixture=True)
        self.assertEqual(policy.admission('INFERENCE', forged), 'BLOCKED')
        self.record('scope_cannot_expand', r)

    def test_historical_classifier_and_writer_evidence_remain_unchanged(self):
        after = copy.deepcopy(self.before)
        after['metadata']['inode'] += 1
        self.assertEqual(ci.classify(self.before, after)['classification'], 'METADATA_CHANGE_UNATTRIBUTED')
        self.assertEqual(ci.classify(self.before, after)['integrity_verification'], 'BLOCKED')

    def test_offline_record_must_match_final_sources_and_binary(self):
        record = {'state':'PASS_OFFLINE','real_cli_started':False,'source_sha256':{'fixture':'hash'},
                  'binary_sha256':'binary'}
        self.assertTrue(driver.offline_verified(record, {'fixture':'hash'}, 'binary'))
        for changed in (dict(record, real_cli_started=True), dict(record, state='PASS_REAL'),
                        dict(record, source_sha256={}), dict(record, binary_sha256='other')):
            self.assertFalse(driver.offline_verified(changed, {'fixture':'hash'}, 'binary'))

    def test_concurrency_copilot_ambiguous_or_inaccessible_blocks_no_signals(self):
        cli = self.file
        with tempfile.TemporaryDirectory() as tmp:
            proc = Path(tmp)
            for name in ('copilot', 'node', 'bun'):
                entry = proc / '123'
                entry.mkdir(exist_ok=True)
                (entry / 'comm').write_text(name)
                fields = ['S', '1'] + ['0'] * 17 + ['100']
                (entry / 'stat').write_text('123 (' + name + ') ' + ' '.join(fields))
                (entry / 'exe').unlink(missing_ok=True)
                (entry / 'exe').symlink_to('/usr/bin/python3')
                self.assertEqual(driver.concurrency(cli, proc)['state'], 'BLOCKED_RELEVANT_CONCURRENCY')
            (entry / 'comm').write_text('renamed-native')
            (entry / 'stat').write_text('123 (renamed-native) ' + ' '.join(fields))
            (entry / 'exe').unlink()
            (entry / 'exe').symlink_to(cli)
            self.assertEqual(driver.concurrency(cli, proc)['copilot_processes'], 1)
            with mock.patch.object(Path, 'read_text', side_effect=PermissionError()):
                self.assertGreater(driver.concurrency(cli, proc)['unavailable'], 0)

    def test_driver_binary_and_protocol_have_no_send_model_quota_or_marker_claim_calls(self):
        root = Path(__file__).resolve().parents[1]
        source = (root / 'src/metadata_confirmation.rs').read_text()
        for forbidden in ('.send(', 'send_and_wait', '.list_models(', '.get_quota(',
                          '.create_session(', '.resume_session(', 'claim_attempt('):
            self.assertNotIn(forbidden, source)
        self.assertNotIn('preflight(&', (root / 'src/bin/a9-metadata-confirm.rs').read_text())
        self.assertNotIn('marker_directory(', (root / 'confirm_a9_metadata.py').read_text())


if __name__ == '__main__':
    unittest.main()
