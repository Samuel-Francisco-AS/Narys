"""Controlled Linux process cleanup tests; never launch Copilot."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('measure', Path(__file__).parents[1] / 'measure.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CleanupTests(unittest.TestCase):
    def run_fixture(self, body, deadline=2):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / 'fixture.py'
            binary.write_text('#!/usr/bin/python3\n' + body)
            binary.chmod(0o700)
            return module.measure(binary, 'metadata', Path('/unused'), deadline)

    def test_normal_exit_needs_no_recovery(self):
        result, code = self.run_fixture('print("{}")\n')
        self.assertEqual(code, 0)
        self.assertEqual(result['owned_survivors_after_recovery'], [])

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


if __name__ == '__main__':
    unittest.main()
