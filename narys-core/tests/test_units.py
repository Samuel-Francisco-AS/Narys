import unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class Units(unittest.TestCase):
    def test_service_no_gui_or_auto_provider_start(self):
        s=(ROOT/'ops/narys-core.service').read_text()
        self.assertIn('narys-core serve',s);self.assertIn('KillMode=control-group',s)
        self.assertNotIn('graphical-session',s);self.assertNotIn('ExecStartPost',s)
        self.assertIn('RuntimeDirectoryMode=0700',s);self.assertIn('LimitCORE=0',s)
    def test_password_uses_existing_human_helper_only(self):
        s=(ROOT/'src/main.rs').read_text()
        self.assertIn('h2_manual_unlock.py',s)
        self.assertNotIn('read_password',s);self.assertNotIn('UnlockWithMasterPassword',s)
    def test_install_does_not_restart_keyring_or_change_boot(self):
        s=(ROOT/'ops/install_user.py').read_text()
        self.assertNotIn("('restart'",s);self.assertNotIn('set-default',s);self.assertNotIn('sudo',s)
    def test_no_public_listener(self):
        s=(ROOT/'src/server.rs').read_text()
        self.assertIn('peer_cred',s);self.assertNotIn('TcpListener',s)
    def test_update_stops_only_owned_core_and_requires_reviewed_hashes(self):
        s=(ROOT/'ops/update_user.py').read_text()
        self.assertIn("digest(binary) != sys.argv[1]",s)
        self.assertIn("'stop', 'narys-core.service'",s)
        self.assertNotIn("'stop', 'gdm",s);self.assertNotIn("'restart'",s)
if __name__=='__main__':unittest.main()
