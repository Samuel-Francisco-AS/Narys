"""Synthetic proc trees and injected failures; never manipulate live processes."""
import copy
import errno
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import confirm_a9_metadata as driver
import metadata_policy as policy


class ConcurrencyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='narys-proc-fixture-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.proc = self.root / 'proc'
        self.proc.mkdir()
        self.cli = self.root / 'synthetic-native'
        self.cli.write_bytes(b'public synthetic executable identity')

    def process(self, pid, name, ppid=1, ticks=100, native=False):
        entry = self.proc / str(pid)
        entry.mkdir()
        (entry / 'comm').write_text(name)
        fields = ['S', str(ppid)] + ['0'] * 17 + [str(ticks)]
        (entry / 'stat').write_text(f'{pid} ({name}) ' + ' '.join(fields))
        (entry / 'exe').symlink_to(self.cli if native else '/usr/bin/python3')
        return entry

    def survey(self, denied=()):
        original = Path.stat
        def stat(path, *args, **kwargs):
            if path.name == 'exe' and int(path.parent.name) in denied:
                raise PermissionError(errno.EACCES, 'synthetic-private-prose')
            return original(path, *args, **kwargs)
        with mock.patch.object(Path, 'stat', new=stat):
            result = driver.concurrency(self.cli, self.proc)
        self.assertNotIn('synthetic-private-prose', json.dumps(result))
        print(json.dumps({'a9_fix4r_case': self._testMethodName, 'survey': result}))
        return result

    def test_executable_eacces_alone_preserves_core_identity_and_allows_metadata(self):
        self.process(123, 'ordinary-worker')
        r = self.survey(denied=(123,))
        self.assertEqual(r['state'], 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS')
        row = r['processes'][0]
        self.assertEqual(row['category'], 'PARTIALLY_INACCESSIBLE')
        self.assertEqual(row['uid'], os.getuid())
        self.assertEqual(row['comm'], 'ordinary-worker')
        self.assertEqual(row['start_ticks'], 100)
        self.assertFalse(r['exclusivity_proven'])

    def test_gnome_and_user_services_are_protected_even_with_exe_denied(self):
        for pid, name in enumerate(('gnome-shell', 'systemd', '(sd-pam)',
                                    'sshd-session', 'gnome-keyring-d', 'dbus-broker'), 120):
            self.process(pid, name)
        r = self.survey(denied=range(120, 126))
        self.assertEqual(r['state'], 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS')
        for row in r['processes']:
            self.assertEqual(row['category'], 'ESSENTIAL_PROCESS')
            self.assertEqual(row['termination_admission'], 'DENIED_PROTECTED_PROCESS')

    def test_codex_ancestors_and_needed_descendants_are_protected_by_relations(self):
        self.process(100, 'codex', ppid=99)
        self.process(101, 'bash', ppid=100)
        self.process(102, 'python3', ppid=101)
        self.process(103, 'node', ppid=100)
        with mock.patch.object(driver.os, 'getpid', return_value=102):
            r = self.survey()
        self.assertEqual(r['state'], 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS')
        self.assertTrue(all(row['category'] == 'OWN_CODEX_OR_HARNESS' for row in r['processes']))

    def test_pinned_native_copilot_is_identified_even_if_renamed(self):
        self.process(120, 'renamed', native=True)
        r = self.survey()
        self.assertEqual(r['copilot_processes'], 1)
        self.assertEqual(r['state'], 'BLOCKED_RELEVANT_CONCURRENCY')

    def test_copilot_name_with_denied_exe_remains_relevant_not_proven_native(self):
        self.process(120, 'copilot')
        r = self.survey(denied=(120,))
        self.assertEqual(r['copilot_processes'], 0)
        self.assertEqual(r['ambiguous_runtimes'], 1)
        self.assertTrue(r['processes'][0]['blocking'])

    def test_unrelated_node_bun_are_ambiguous_and_block_without_kill_permission(self):
        for pid, name in ((120, 'node'), (121, 'bun')):
            self.process(pid, name)
        r = self.survey(denied=(120,))
        self.assertEqual(r['ambiguous_runtimes'], 2)
        self.assertEqual(r['state'], 'BLOCKED_RELEVANT_CONCURRENCY')
        self.assertTrue(all(row['termination_admission'] == 'DENIED_NO_PROVEN_DISPOSABILITY'
                            for row in r['processes']))

    def test_process_disappears_during_scan_without_false_concurrency(self):
        self.process(120, 'ordinary-worker')
        with mock.patch.object(driver, 'proc_identity', side_effect=FileNotFoundError()):
            r = self.survey()
        self.assertEqual(r['vanished_during_scan'], 1)
        self.assertEqual(r['processes'], [])
        self.assertEqual(r['state'], 'METADATA_SURVEY_ACCEPTED_WITH_LIMITS')

    def test_pid_reuse_or_changed_identity_fails_closed(self):
        entry = self.process(120, 'ordinary-worker')
        first = driver.proc_identity(entry)
        second = dict(first, start_ticks=first['start_ticks'] + 1)
        with mock.patch.object(driver, 'proc_identity', side_effect=[first, second]):
            r = self.survey()
        self.assertEqual(r['state'], 'BLOCKED_RELEVANT_CONCURRENCY')
        self.assertEqual(r['processes'][0]['category'], 'SUSPICIOUS_OR_UNASSESSABLE')

    def test_scheduler_state_change_is_not_pid_reuse(self):
        entry = self.process(120, 'ordinary-worker')
        first = driver.proc_identity(entry)
        with mock.patch.object(driver, 'proc_identity', side_effect=[first, dict(first, state='R')]):
            r = self.survey()
        self.assertTrue(r['processes'][0]['identity_stable'])

    def test_missing_core_identity_still_blocks_unlike_exe_only_eacces(self):
        self.process(120, 'ordinary-worker')
        with mock.patch.object(driver, 'proc_identity', side_effect=PermissionError()):
            r = self.survey()
        self.assertEqual(r['state'], 'BLOCKED_RELEVANT_CONCURRENCY')
        self.assertEqual(r['processes'][0]['comm'], 'ordinary-worker')

    def test_external_essential_cannot_be_terminated_by_observation(self):
        self.process(120, 'systemd')
        with mock.patch.object(os, 'kill') as signal:
            r = self.survey(denied=(120,))
            self.assertEqual(driver.termination_admission(r['processes'][0]), 'DENIED_PROTECTED_PROCESS')
            signal.assert_not_called()
        self.assertEqual(r['processes_terminated'], 0)

    def test_concurrency_acceptance_or_failure_cannot_open_other_scopes(self):
        self.process(120, 'copilot', native=True)
        self.survey()
        forged = {'metadata_verification':'OBSERVATION_ACCEPTED', 'authenticated':True,
                  'concurrency':'PASS', 'force':True}
        for scope in ('SESSION_OPERATIONS', 'INFERENCE', 'AGENT_ACTIONS', 'HEADLESS_OPERATION'):
            self.assertEqual(policy.admission(scope, forged), 'BLOCKED')
        with self.assertRaises(TypeError):
            driver.concurrency(self.cli, self.proc, force=True)

    def test_recovery_has_fixed_independent_identity_no_arbitrary_retry_paths(self):
        old = driver.execution_plan('A9-FIX-4')
        new = driver.execution_plan('A9-FIX-4R')
        self.assertNotEqual(old[0], new[0])
        self.assertNotEqual(old[1], new[1])
        self.assertNotEqual(old[2], new[2])
        with self.assertRaises(ValueError):
            driver.execution_plan('A9-FIX-4R-retry')
        source = (Path(__file__).resolve().parents[1] / 'src/bin/a9-metadata-confirm-r.rs').read_text()
        self.assertIn('create_new(true)', source)
        self.assertIn('a9-fix-4r-runtime-reservation.json', source)
        self.assertNotIn('a9-fix-4-runtime-reservation.json', source)
        self.assertNotIn('lr10a-a9-host-attempt.json', source)


if __name__ == '__main__':
    unittest.main()
