"""FIX-4 launcher preconditions only; kernel regression remains FIX-3 suite."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import boundary
import fix4_boundary


class Fix4LauncherTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.workspace = self.root / 'workspace'
        self.state = self.root / 'state'
        self.store = self.state / 'session-state'
        self.logs = self.root / 'logs'
        for p in (self.workspace, self.store, self.logs):
            p.mkdir(parents=True)
        self.program = ROOT / 'target/debug/network-fixture'

    def plan(self, program=None):
        return fix4_boundary.plan(program or self.program, self.workspace,
                                  self.state, self.store, self.logs)

    def test_unchanged_kernel_policy_and_only_owned_elf_replacement(self):
        original = boundary.plan(ROOT / 'target/debug/boundary-fixture', self.workspace,
                                 self.state, self.store, self.logs)
        new = self.plan()
        index = original['args'].index(str(ROOT / 'target/debug/boundary-fixture'))
        original['args'][index] = str(self.program)
        self.assertEqual(new['args'], original['args'])
        self.assertEqual(new['environment_names'], original['environment_names'])
        self.assertNotIn('--share-net', new['args'])

    def test_unknown_binary_blocked_before_dependency_inspection(self):
        with mock.patch.object(boundary, 'native_dependencies') as inspect:
            with self.assertRaisesRegex(boundary.BoundaryError, 'unapproved_fix4_fixture'):
                self.plan(Path('/usr/bin/python3'))
            inspect.assert_not_called()

    def test_dependency_change_no_fallback(self):
        real = boundary.native_dependencies
        def changed(program):
            p, libraries = real(program)
            return p, libraries + [('fake', '/lib64/unknown')] if p == self.program else libraries
        with mock.patch.object(boundary, 'native_dependencies', side_effect=changed):
            with self.assertRaisesRegex(boundary.BoundaryError, 'fix4_dependency_closure_changed'):
                self.plan()

    def test_external_or_symlink_data_blocked(self):
        with self.assertRaises(boundary.BoundaryError):
            fix4_boundary.plan(self.program, Path('/home'), self.state, self.store, self.logs)
        link = self.root / 'alias'
        link.symlink_to(self.workspace)
        with self.assertRaises(boundary.BoundaryError):
            fix4_boundary.plan(self.program, link, self.state, self.store, self.logs)


if __name__ == '__main__':
    unittest.main()
