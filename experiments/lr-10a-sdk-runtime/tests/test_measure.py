"""Controlled Linux process cleanup tests; never launch Copilot."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('measure', Path(__file__).parents[1] / 'measure.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CleanupTests(unittest.TestCase):
    observations = []

    def assert_reclaimed(self, result):
        for row in result.get('attributed_processes', []):
            try:
                current = module.identity(row['pid'])
            except FileNotFoundError:
                continue
            self.assertNotEqual(current['start_ticks'], row['start_ticks'],
                                'fixture identity still exists, including zombie')

    def run_fixture(self, body, deadline=2):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / 'fixture.py'
            manifest = Path(tmp) / 'identities.jsonl'
            # Independent fixture-owned PID/start-time inventory, including
            # children before setsid/double-fork. No command lines or environ.
            prelude = (
                'import os, json\n'
                'def record_identity():\n'
                '    pid = os.getpid()\n'
                '    data = open(f"/proc/{pid}/stat").read()\n'
                '    start = int(data[data.rfind(")") + 2:].split()[19])\n'
                f'    fd = os.open({str(manifest)!r}, '
                'os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)\n'
                '    os.write(fd, (json.dumps({"pid": pid, "start_ticks": start}) '
                '+ "\\n").encode())\n'
                '    os.close(fd)\n'
                'record_identity()\n'
                'os.register_at_fork(after_in_child=record_identity)\n'
            )
            binary.write_text('#!/usr/bin/python3\n' + prelude + body)
            binary.chmod(0o700)
            result, code = module.measure(binary, 'metadata', Path('/unused'), deadline)
            self.assert_reclaimed(result)
            fixture_rows = ([json.loads(line) for line in manifest.read_text().splitlines()]
                            if manifest.exists() else [])
            self.assert_reclaimed({'attributed_processes': fixture_rows})
            self.observations.append({'test': self.id(), 'result': result,
                                      'harness_exit_code': code,
                                      'independent_fixture_identities': fixture_rows,
                                      'fixture_identities_absent_after': True})
            return result, code

    def test_normal_exit_needs_no_recovery(self):
        result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 0)
        self.assertEqual(result['owned_survivors_after_recovery'], [])
        self.assertEqual(result['cleanup_status'], 'graceful_no_recovery')
        self.assertTrue(result['cleanup_complete'])
        self.assertTrue(result['kernel_children_exhausted'])
        self.assertFalse(result['sdk_shutdown_verified'])

    def test_premature_parent_exit_detects_and_reaps_descendant(self):
        result, code = self.run_fixture('import subprocess, time\n'
            'subprocess.Popen(["/usr/bin/python3", "-c", "import time; time.sleep(60)"], stdout=subprocess.DEVNULL)\n'
            'time.sleep(0.2)\nprint("{}")\n')
        self.assertEqual(code, 1)
        self.assertTrue(result['owned_survivors_before_recovery'])
        self.assertEqual(result['owned_survivors_after_recovery'], [])
        self.assertGreaterEqual(result['harness_reaped_descendants'], 1)

    def test_timeout_kills_owned_group_and_reports_failure(self):
        result, code = self.run_fixture('import time\ntime.sleep(60)\n', deadline=0.2)
        self.assertEqual(code, 1)
        self.assertTrue(result['timed_out'])
        self.assertEqual(result['owned_survivors_after_recovery'], [])


    def test_inherited_stdout_does_not_block_cleanup(self):
        result, code = self.run_fixture('import subprocess, time\n'
            'subprocess.Popen(["/usr/bin/python3", "-c", "import time; time.sleep(60)"])\n'
            'time.sleep(0.2)\nprint("{}")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['owned_survivors_after_recovery'], [])
        self.assertGreaterEqual(result['harness_reaped_descendants'], 1)


    def test_malformed_evidence_fails_closed_without_echoing_content(self):
        result, code = self.run_fixture('print("synthetic-secret")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['sdk_report']['output'], 'unrecognized')
        self.assertNotIn('synthetic-secret', str(result))

    def fast_parent_fixture(self, new_session=False, nested=False, crash=False):
        # A pipe handshake proves the child has started (and completed setsid)
        # before the root exits. after_launch waits for that exit before metrics.
        return (
            'import os, time\n'
            'read_fd, write_fd = os.pipe()\n'
            'pid = os.fork()\n'
            'if pid == 0:\n'
            '    os.close(read_fd)\n'
            + ('    os.setsid()\n' if new_session else '')
            + ('    if os.fork() != 0:\n'
               '        os._exit(0)\n' if nested else '')
            + '    os.write(write_fd, b"ready")\n'
            '    os.close(write_fd)\n'
            '    time.sleep(60)\n'
            '    os._exit(0)\n'
            'os.close(write_fd)\n'
            'assert os.read(read_fd, 5) == b"ready"\n'
            'os.close(read_fd)\n'
            + ('os.kill(os.getpid(), 9)\n' if crash else
               'os.write(1, b"{}\\n")\nos._exit(0)\n')
        )

    def run_after_root_exit(self, body):
        with mock.patch.object(module, 'after_launch',
                               side_effect=lambda proc: proc.wait(timeout=3)):
            result, code = self.run_fixture(body)
        self.assertEqual(code, 1)
        self.assertEqual(result['observed_owned_processes'], 0)
        self.assertGreaterEqual(result['unsampled_attributed_descendants'], 1)
        self.assertTrue(result['cleanup_complete'])
        self.assertEqual(result['cleanup_status'], 'descendants_recovered')
        self.assertGreaterEqual(result['harness_reaped_descendants'], 1)
        return result

    def test_T1_unsampled_child_after_root_exit(self):
        self.run_after_root_exit(self.fast_parent_fixture())

    def test_T2_unsampled_child_new_session(self):
        self.run_after_root_exit(self.fast_parent_fixture(new_session=True))

    def test_T3_zero_exit_with_live_child_requires_recovery(self):
        result = self.run_after_root_exit(self.fast_parent_fixture())
        self.assertEqual(result['exit_code'], 0)
        self.assertGreater(result['harness_recovery_signals'], 0)
        self.assertFalse(result['sdk_shutdown_verified'])

    def test_unsampled_double_fork_new_session(self):
        self.run_after_root_exit(self.fast_parent_fixture(new_session=True, nested=True))

    def test_repeated_adoption_recovers_previously_hidden_grandchild(self):
        body = self.fast_parent_fixture(new_session=True, nested=True).replace(
            '    if os.fork() != 0:\n        os._exit(0)',
            '    if os.fork() != 0:\n        time.sleep(60)\n        os._exit(0)')
        result = self.run_after_root_exit(body)
        self.assertGreaterEqual(result['harness_reaped_descendants'], 2)
        self.assertGreaterEqual(result['unsampled_attributed_descendants'], 2)

    def test_crashed_root_unsampled_child(self):
        result = self.run_after_root_exit(self.fast_parent_fixture(crash=True))
        self.assertEqual(result['exit_code'], -signal.SIGKILL)

    def test_T4_timeout_active_tree_new_session(self):
        # Readiness is communicated on a private file; the hook waits for it
        # before the measurement loop/deadline, independent of scheduling luck.
        with tempfile.TemporaryDirectory() as tmp:
            ready = Path(tmp) / 'ready'
            body = self.fast_parent_fixture(new_session=True).replace(
                'os.write(1, b"{}\\n")\nos._exit(0)',
                f'open({str(ready)!r}, "w").close()\ntime.sleep(60)')

            def wait_ready(proc):
                deadline = module.time.monotonic() + 3
                while not ready.exists():
                    if proc.poll() is not None or module.time.monotonic() > deadline:
                        raise AssertionError('fixture did not reach ready barrier')
                    module.time.sleep(0.005)

            with mock.patch.object(module, 'after_launch', side_effect=wait_ready):
                result, code = self.run_fixture(body, deadline=0.15)
        self.assertEqual(code, 1)
        self.assertTrue(result['timed_out'])
        self.assertTrue(result['cleanup_complete'])
        self.assertEqual(result['cleanup_status'], 'timeout_recovered')
        self.assertGreaterEqual(result['harness_reaped_descendants'], 1)

    def test_T5_external_child_untouched_and_unreaped(self):
        external = subprocess.Popen(['/usr/bin/python3', '-c',
                                     'import time; time.sleep(60)'])
        fd = os.pidfd_open(external.pid)
        external_identity = {'pid': external.pid,
                             'start_ticks': module.identity(external.pid)['start_ticks']}
        try:
            result = self.run_after_root_exit(self.fast_parent_fixture(new_session=True))
            self.assertIsNone(external.poll())
            self.assertEqual(select.select([fd], [], [], 0)[0], [])
            self.assertNotIn(external.pid, [r['pid'] for r in result['attributed_processes']])
            self.observations.append({'test': self.id(),
                                      'external_identity': external_identity,
                                      'external_alive_after_harness': True})
        finally:
            signal.pidfd_send_signal(fd, signal.SIGKILL)
            external.wait(timeout=3)
            os.close(fd)
        self.observations[-1]['external_fixture_reclaimed_by_test'] = True

    def test_T6_external_identity_never_opens_handle(self):
        with mock.patch.object(module, 'identity', return_value={
                'ppid': os.getpid() + 1, 'start_ticks': 42}), \
                mock.patch.object(module.os, 'pidfd_open') as opened:
            with self.assertRaisesRegex(module.AttributionError, 'external_not_attributed'):
                module.owned_handle(999999)
            opened.assert_not_called()

    def test_T6_pid_identity_change_closes_handle_without_signal(self):
        with mock.patch.object(module, 'identity', side_effect=[
                {'ppid': os.getpid(), 'start_ticks': 42},
                {'ppid': os.getpid(), 'start_ticks': 43}]), \
                mock.patch.object(module.os, 'pidfd_open', return_value=987), \
                mock.patch.object(module.os, 'close') as closed, \
                mock.patch.object(module.signal, 'pidfd_send_signal') as sent:
            with self.assertRaisesRegex(module.AttributionError, 'identity_changed'):
                module.owned_handle(999999)
            closed.assert_called_once_with(987)
            sent.assert_not_called()

    def test_T6_attribution_failure_remains_inconclusive_after_recovery(self):
        real = module.owned_handle
        failed = False

        def fail_first_adoption(pid):
            nonlocal failed
            if not failed and pid != root_pid[0]:
                failed = True
                raise module.AttributionError('identity_changed')
            return real(pid)

        root_pid = [None]

        def exited(proc):
            root_pid[0] = proc.pid
            proc.wait(timeout=3)

        # Only fail an adopted child after root attribution, then allow cleanup.
        def conditional(pid):
            if root_pid[0] is None:
                return real(pid)
            return fail_first_adoption(pid)

        with mock.patch.object(module, 'owned_handle', side_effect=conditional), \
                mock.patch.object(module, 'after_launch', side_effect=exited):
            result, code = self.run_fixture(self.fast_parent_fixture())
        self.assertEqual(code, 1)
        self.assertIn('identity_changed', result['ownership_errors'])
        self.assertEqual(result['cleanup_status'], 'inconclusive')
        self.assertFalse(result['cleanup_complete'])
        self.assertTrue(result['kernel_children_exhausted'])

    def test_T6_failed_recovery_signal_not_reported_complete(self):
        real = signal.pidfd_send_signal
        failed = False

        def fail_once(fd, sig, *args):
            nonlocal failed
            if sig == signal.SIGKILL and not failed:
                failed = True
                raise PermissionError('synthetic-private-error')
            return real(fd, sig, *args)

        with mock.patch.object(module.signal, 'pidfd_send_signal', side_effect=fail_once):
            result, code = self.run_fixture(self.fast_parent_fixture())
        self.assertEqual(code, 1)
        self.assertEqual(result['cleanup_status'], 'inconclusive')
        self.assertIn('recovery_signal_failed', result['ownership_errors'])
        self.assertFalse(result['cleanup_complete'])
        self.assertNotIn('synthetic-private-error', str(result))

    def test_T6_missing_identity_remains_inconclusive(self):
        real = module.owned_handle
        failed = False

        def missing_once(pid):
            nonlocal failed
            if not failed:
                failed = True
                raise FileNotFoundError('private-missing-identity')
            return real(pid)

        with mock.patch.object(module, 'owned_handle', side_effect=missing_once), \
                mock.patch.object(module, 'after_launch',
                                  side_effect=lambda proc: proc.wait(timeout=3)):
            result, code = self.run_fixture(self.fast_parent_fixture())
        self.assertEqual(code, 1)
        self.assertEqual(result['cleanup_status'], 'inconclusive')
        self.assertIn('identity_unavailable', result['ownership_errors'])
        self.assertFalse(result['cleanup_complete'])
        self.assertTrue(result['kernel_children_exhausted'])
        self.assertNotIn('private-missing-identity', str(result))

    def test_T6_unverifiable_wait_exhaustion_is_incomplete(self):
        real = os.waitpid

        def unknown_exhaustion(pid, options):
            try:
                return real(pid, options)
            except ChildProcessError:
                raise OSError('synthetic-private-error') from None

        # No actual survivor: the fixture exits, but the proof of exhaustion is
        # deliberately unavailable. Scope the fault to the private worker only.
        def exited(proc):
            proc.wait(timeout=3)
            module.os.waitpid = unknown_exhaustion

        with mock.patch.object(module, 'after_launch', side_effect=exited), \
                mock.patch.object(module, 'CLEANUP_SECONDS', 0.05):
            result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['cleanup_status'], 'recovery_incomplete')
        self.assertFalse(result['kernel_children_exhausted'])
        self.assertFalse(result['cleanup_complete'])
        self.assertIn('wait_failed', result['ownership_errors'])

    def test_T6_inventory_failure_blocks_false_empty_cleanup(self):
        def exited(proc):
            proc.wait(timeout=3)
            module.children = mock.Mock(side_effect=OSError('private-inventory-error'))

        with mock.patch.object(module, 'after_launch', side_effect=exited), \
                mock.patch.object(module, 'CLEANUP_SECONDS', 0.05):
            result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['cleanup_status'], 'recovery_incomplete')
        self.assertFalse(result['cleanup_complete'])
        self.assertFalse(result['kernel_children_exhausted'])
        self.assertIn('final_child_inventory_unavailable', result['ownership_errors'])
        self.assertNotIn('private-inventory-error', str(result))

    def test_worker_failure_is_inconclusive(self):
        with mock.patch.object(module, '_worker_measure', side_effect=RuntimeError('secret')):
            result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['cleanup_status'], 'inconclusive')
        self.assertEqual(result['ownership_errors'], ['ownership_worker_failed'])
        self.assertFalse(result['cleanup_complete'])
        self.assertNotIn('secret', str(result))

    def test_missing_pidfd_fails_before_launch(self):
        with mock.patch.object(module.os, 'pidfd_open', side_effect=OSError('secret')):
            result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['ownership_errors'], ['pidfd_unavailable'])
        self.assertFalse(result['cleanup_complete'])
        self.assertEqual(self.observations[-1]['independent_fixture_identities'], [])

    def test_caller_subreaper_and_sigchld_unchanged(self):
        def subreaper_flag():
            value = module.ctypes.c_int()
            self.assertEqual(module.ctypes.CDLL(None).prctl(
                37, module.ctypes.byref(value), 0, 0, 0), 0)
            return value.value

        before = subreaper_flag(), signal.getsignal(signal.SIGCHLD)
        self.run_after_root_exit(self.fast_parent_fixture(new_session=True))
        self.assertEqual((subreaper_flag(), signal.getsignal(signal.SIGCHLD)), before)

    def test_threaded_caller_refused_before_launch(self):
        with mock.patch.object(module.threading, 'active_count', return_value=2):
            result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 1)
        self.assertFalse(result['cleanup_complete'])
        self.assertEqual(result['ownership_errors'], ['invalid_deadline_or_threaded_caller'])
        self.assertEqual(self.observations[-1]['independent_fixture_identities'], [])

    def test_nondefault_sigchld_refused_before_launch(self):
        with mock.patch.object(module.signal, 'getsignal', return_value=signal.SIG_IGN):
            result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 1)
        self.assertEqual(result['ownership_errors'], ['caller_sigchld_not_default'])
        self.assertFalse(result['cleanup_complete'])
        self.assertEqual(self.observations[-1]['independent_fixture_identities'], [])


def record_evidence(path):
    """Run the entire fixture suite and publish sanitized, reproducible evidence."""
    class Results(unittest.TextTestResult):
        outcomes = []

        def addSuccess(self, test):
            super().addSuccess(test)
            self.outcomes.append({'test': test.id(), 'status': 'PASS'})

        def addFailure(self, test, err):
            super().addFailure(test, err)
            self.outcomes.append({'test': test.id(), 'status': 'FAIL'})

        def addError(self, test, err):
            super().addError(test, err)
            self.outcomes.append({'test': test.id(), 'status': 'ERROR'})

    started = module.time.monotonic()
    CleanupTests.observations = []
    log = io.StringIO()
    suite = unittest.defaultTestLoader.discover(str(Path(__file__).parent), 'test_*.py')
    # Discovery imports this file under its module name. Share the observation
    # list with that class rather than executing a different or partial suite.
    imported = sys.modules.get('test_measure')
    if imported is not None:
        imported.CleanupTests.observations = CleanupTests.observations
    results = unittest.TextTestRunner(stream=log, verbosity=2, resultclass=Results).run(suite)
    identities = {}
    for item in CleanupTests.observations:
        rows = item.get('independent_fixture_identities', []) + item.get(
            'result', {}).get('attributed_processes', [])
        if 'external_identity' in item:
            rows.append(item['external_identity'])
        for row in rows:
            identities[row['pid'], row['start_ticks']] = row
    remaining = []
    for (pid, start), row in identities.items():
        try:
            if module.identity(pid)['start_ticks'] == start:
                remaining.append(row)
        except FileNotFoundError:
            pass
    experiment = Path(__file__).parents[1]
    evidence = {
        'schema_version': 1, 'phase': 'LR-10A FIX-1',
        'command': ('python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py '
                    '--evidence experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.json'),
        'python': sys.version.split()[0], 'kernel': os.uname().release,
        'platform': 'Fedora Linux 44, SSH/headless',
        'tests_run': results.testsRun, 'tests': results.outcomes,
        'elapsed_seconds': round(module.time.monotonic() - started, 3),
        'successful': results.wasSuccessful() and not remaining,
        'observations': CleanupTests.observations,
        'post_suite_fixture_identity_count': len(identities),
        'post_suite_fixture_survivors': remaining,
        'post_suite_scope': 'fixture manifests, attributed identities and external control only',
        'real_copilot_cli_invocations': 0, 'inference_requests': 0,
        'copilot_quota_consumed_by_fixture_suite': 0,
        'sha256': {str(p.relative_to(experiment)): hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in [experiment / 'measure.py', Path(__file__).resolve(),
                             experiment / 'README.md']},
    }
    path.write_text(json.dumps(evidence, indent=2) + '\n')
    path.with_suffix('.txt').write_text(log.getvalue())
    print(log.getvalue(), end='')
    return 0 if evidence['successful'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--evidence', type=Path)
    args, remaining_args = parser.parse_known_args()
    if args.evidence:
        if remaining_args:
            parser.error('evidence mode always runs the complete suite')
        raise SystemExit(record_evidence(args.evidence))
    unittest.main()
