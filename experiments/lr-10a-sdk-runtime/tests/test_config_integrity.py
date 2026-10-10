"""Synthetic filesystem/contract tests only; never start Copilot or SDK auth."""
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
import review_a9_config as review


def record(case, result):
    # Only allowlisted classifier results. Captured by the synthetic test runner.
    print(json.dumps({'a9_config_case': case, 'evaluation': result}))


class IntegrityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='narys-config-test-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.path = self.root / 'synthetic-config.json'
        self.path.write_bytes(b'AAAA')
        self.path.chmod(0o600)

    def observe(self, before):
        return ci.classify(before, ci.snapshot(self.path))

    def test_01_stable_metadata_is_not_content_integrity_or_admission(self):
        result = self.observe(ci.snapshot(self.path))
        self.assertEqual(result['classification'], 'OBSERVATIONALLY_STABLE')
        self.assertFalse(result['metadata_mutation'])
        self.assertEqual(result['content_equivalence'], 'NOT_VERIFIED')
        self.assertEqual(result['integrity_verification'], 'INCONCLUSIVE')
        self.assertEqual(result['operational_admission'], 'BLOCKED')
        record('T1_stable', result)

    def test_02_equal_size_atomic_replacement_does_not_prove_equal_bytes(self):
        before = ci.snapshot(self.path)
        replacement = self.root / 'replacement'
        replacement.write_bytes(b'BBBB')
        replacement.chmod(0o600)
        os.replace(replacement, self.path)
        after = ci.snapshot(self.path)
        self.assertEqual(before['metadata']['bytes'], after['metadata']['bytes'])
        self.assertNotEqual(before['metadata']['inode'], after['metadata']['inode'])
        result = ci.classify(before, after)
        self.assertEqual(result['classification'], 'METADATA_CHANGE_UNATTRIBUTED')
        self.assertEqual(result['content_equivalence'], 'NOT_VERIFIED')
        self.assertEqual(result['integrity_verification'], 'BLOCKED')
        record('T2_equal_size_atomic_replace', result)

    def test_03_size_change_is_unknown_not_corruption_claim(self):
        before = ci.snapshot(self.path)
        self.path.write_bytes(b'public-synthetic-longer')
        result = self.observe(before)
        self.assertIn('bytes', result['changed_fields'])
        self.assertEqual(result['writer_attribution'], 'INCONCLUSIVE')
        record('T3_size', result)

    def test_04_timestamp_change_does_not_authorize_write(self):
        before = ci.snapshot(self.path)
        ns = before['metadata']['mtime_ns'] + 1_000_000_000
        os.utime(self.path, ns=(ns, ns))
        result = self.observe(before)
        self.assertIn('mtime_ns', result['changed_fields'])
        self.assertEqual(result['integrity_verification'], 'BLOCKED')
        record('T4_timestamp', result)

    def test_05_concurrent_synthetic_process_not_attributed_by_stat(self):
        before = ci.snapshot(self.path)
        # Pipe synchronization, no millisecond race/sleep or signals by PID.
        code = ('import os,pathlib,sys; sys.stdin.buffer.read(1); '
                'p=pathlib.Path(sys.argv[1]); p.write_bytes(b"BBBB"); '
                'n=int(sys.argv[2]); os.utime(p,ns=(n,n)); print("done")')
        child = subprocess.Popen([sys.executable, '-c', code, str(self.path),
                                  str(before['metadata']['mtime_ns'] + 1_000_000_000)],
                                 env={'PATH': '/usr/bin', 'LANG': 'C'},
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.DEVNULL)
        output, _ = child.communicate(b'x', timeout=3)
        self.assertEqual(child.returncode, 0)
        self.assertEqual(output.strip(), b'done')
        result = self.observe(before)
        self.assertEqual(result['writer_attribution'], 'INCONCLUSIVE')
        self.assertEqual(result['classification'], 'METADATA_CHANGE_UNATTRIBUTED')
        record('T5_concurrent_writer_stat_has_no_pid', result)

    def test_06_missing_and_metadata_access_denied_are_distinct_reasons(self):
        missing = ci.snapshot(self.root / 'missing')
        self.assertEqual(missing['reason'], 'missing')
        with mock.patch.object(ci.os, 'stat', side_effect=PermissionError(errno.EACCES,
                                 'public-fake-private-path-secret')):
            denied = ci.snapshot(self.path)
        self.assertEqual(denied['reason'], 'metadata_access_denied')
        self.assertNotIn('public-fake-private-path-secret', json.dumps(denied))
        self.assertEqual(ci.classify(missing, denied)['classification'], 'METADATA_UNAVAILABLE')
        record('T6_missing_real_EACCES_injected', ci.classify(missing, denied))

    def test_07_leaf_parent_symlinks_and_non_regular_file_rejected(self):
        link = self.root / 'link'
        link.symlink_to(self.path)
        parent = self.root / 'parent-link'
        parent.symlink_to(self.root, target_is_directory=True)
        before = ci.snapshot(self.path)
        for path in (link, parent / self.path.name, self.root):
            result = ci.classify(before, ci.snapshot(path))
            self.assertEqual(result['classification'], 'VERIFICATION_FAILED')
            self.assertEqual(result['operational_admission'], 'BLOCKED')
        record('T7_symlink_parent_leaf_non_regular', result)

    def test_08_observation_interrupted_or_failed_is_fail_closed(self):
        before = ci.snapshot(self.path)
        for error, classification in [(InterruptedError(), 'INCONCLUSIVE'),
                                      (OSError(errno.EIO, 'public-fake-secret'), 'VERIFICATION_FAILED')]:
            with mock.patch.object(ci.os, 'stat', side_effect=error):
                after = ci.snapshot(self.path)
            result = ci.classify(before, after)
            self.assertEqual(result['classification'], classification)
            self.assertEqual(result['operational_admission'], 'BLOCKED')
            self.assertNotIn('public-fake-secret', repr(result))
            record('T8_' + classification, result)

    def test_09_fixed_owned_synthetic_revision_proven_not_host_admission(self):
        result = ci.prove_synthetic_update()
        self.assertEqual(result['classification'], 'LEGITIMATE_CHANGE_PROVEN')
        self.assertEqual(result['writer_attribution'], 'KNOWN_SYNTHETIC_WRITER')
        self.assertEqual(result['integrity_verification'], 'PASS_SYNTHETIC_CONTRACT')
        self.assertEqual(result['content_equivalence'], 'DIFFERENT_VERIFIED_SYNTHETIC_BYTES')
        self.assertEqual(result['operational_admission'], 'BLOCKED')
        record('T9_owned_fixed_synthetic_update', result)

    def test_10_caller_claim_or_filesystem_event_is_not_writer_proof(self):
        before = ci.snapshot(self.path)
        self.path.write_bytes(b'BBBBBBBB')
        result = self.observe(before)
        with self.assertRaises(TypeError):
            ci.classify(before, ci.snapshot(self.path), legitimate=True)
        with self.assertRaises(TypeError):
            ci.prove_synthetic_update(self.path)
        self.assertEqual(result['writer_attribution'], 'INCONCLUSIVE')
        record('T10_unattributed_no_legitimacy_flag', result)

    def test_11_positive_auth_independent_of_inconclusive_integrity(self):
        before = ci.snapshot(self.path)
        with mock.patch.object(ci.os, 'stat', side_effect=InterruptedError()):
            result = ci.classify(before, ci.snapshot(self.path))
        gates = ci.independent_observations(True, result)
        self.assertTrue(gates['sdk_auth_observed'])
        self.assertEqual(gates['config']['integrity_verification'], 'INCONCLUSIVE')
        self.assertEqual(gates['financial_admission'], 'BLOCKED')
        record('T11_synthetic_auth_with_inconclusive_integrity', result)

    def test_12_integrity_failure_never_opens_financial_or_inference_gate(self):
        before = ci.snapshot(self.path)
        result = ci.classify(before, ci.snapshot(self.root / 'missing'))
        gates = ci.independent_observations(True, result)
        self.assertEqual(gates['financial_admission'], 'BLOCKED')
        self.assertEqual(gates['a9_real_inference'], 'NOT_RUN')
        self.assertEqual(gates['inference_calls'], 0)
        self.assertEqual(gates['real_session_operations'], 0)
        self.assertFalse(gates['marker_claimed'])
        record('T12_integrity_failure_financial_blocked', result)

    def test_13_malformed_coverage_booleans_or_unknown_states_never_pass(self):
        before = ci.snapshot(self.path)
        for after in (None, {}, {'state': 'unknown'},
                      {'state': 'observed', 'metadata': {'bytes': 4}},
                      {'state': 'observed', 'metadata': dict(before['metadata'], bytes=True)}):
            self.assertEqual(ci.classify(before, after)['classification'], 'VERIFICATION_FAILED')

    def test_14_relative_or_traversal_path_never_queries_filesystem(self):
        with mock.patch.object(ci.os, 'open') as opening:
            for path in ('relative', self.root / '..' / 'external', '/'):
                self.assertEqual(ci.snapshot(path)['state'], 'failed')
            opening.assert_not_called()

    def test_15_parent_open_race_is_verification_failure(self):
        with mock.patch.object(ci.os, 'fstat', return_value=mock.Mock(st_dev=-1, st_ino=-1)):
            result = ci.snapshot(self.path)
        self.assertEqual(result, {'state': 'failed', 'reason': 'path_identity_changed'})

    def test_16_stat_never_opens_leaf_for_content(self):
        actual = ci.os.open
        with mock.patch.object(ci.os, 'open', wraps=actual) as opening:
            self.assertEqual(ci.snapshot(self.path)['state'], 'observed')
        for call in opening.call_args_list:
            self.assertTrue(call.args[1] & os.O_PATH)
            self.assertTrue(call.args[1] & os.O_NOFOLLOW)
            self.assertNotEqual(call.args[0], self.path.name)

    def test_17_stable_later_snapshot_does_not_reclassify_previous_mutation(self):
        before = ci.snapshot(self.path)
        self.path.write_bytes(b'BBBBBBBB')
        after = ci.snapshot(self.path)
        previous = ci.classify(before, after)
        later = ci.classify(after, ci.snapshot(self.path))
        self.assertEqual(previous['classification'], 'METADATA_CHANGE_UNATTRIBUTED')
        self.assertEqual(later['classification'], 'OBSERVATIONALLY_STABLE')
        self.assertEqual(previous['writer_attribution'], 'INCONCLUSIVE')
        self.assertEqual(later['integrity_verification'], 'INCONCLUSIVE')

    def test_18_pinned_history_replay_is_not_new_auth_and_tampering_rejected(self):
        result = review.review()
        self.assertEqual(result['sdk_authenticated_with_gui'], 'OBSERVED_REAL_PASS')
        self.assertEqual(result['config_writer_attribution'], 'INCONCLUSIVE')
        self.assertEqual(result['config_integrity_verification'], 'BLOCKED')
        self.assertEqual(result['current_cli_invocations'], 0)
        self.assertEqual(result['current_personal_config_observations'], 0)
        fake = self.root / 'fake-history.json'
        fake.write_text('{"authenticated":true}')
        with mock.patch.object(review, 'HISTORY', fake):
            with self.assertRaisesRegex(ValueError, 'historical_evidence_identity_mismatch'):
                review.review()


if __name__ == '__main__':
    unittest.main()
