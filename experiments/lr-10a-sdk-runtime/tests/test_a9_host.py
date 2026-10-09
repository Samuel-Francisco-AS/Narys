"""Owned synthetic marker preparation only; no SDK/CLI or real state access."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from run_a9_host import marker_directory


class MarkerPreparation(unittest.TestCase):
    def test_fresh_directory_private_and_marker_not_claimed(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            self.assertFalse(marker_directory(home))
            private = home / '.local/state/narys'
            self.assertEqual(private.stat().st_mode & 0o777, 0o700)
            self.assertFalse((private / 'lr10a-a9-host-attempt.json').exists())

    def test_existing_marker_even_corrupt_blocks_without_reading(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            marker_directory(home)
            p = home / '.local/state/narys/lr10a-a9-host-attempt.json'
            p.write_bytes(b'corrupt-synthetic-marker')
            p.chmod(0o000)
            self.assertTrue(marker_directory(home))
            p.chmod(0o600)

    def test_symlink_parent_or_permissive_private_directory_denied(self):
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            (home / '.local').symlink_to('/nonexistent')
            with self.assertRaises(OSError):
                marker_directory(home)
        with tempfile.TemporaryDirectory() as root:
            home = Path(root)
            marker_directory(home)
            (home / '.local/state/narys').chmod(0o755)
            with self.assertRaises(ValueError):
                marker_directory(home)


if __name__ == '__main__':
    unittest.main()
