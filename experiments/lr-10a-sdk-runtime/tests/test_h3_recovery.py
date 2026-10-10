import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from h3_recovery import qualify, SERVICE


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.timer = dict(LoadState='loaded', ActiveState='active', SubState='waiting',
            Transient='yes', Unit=SERVICE, TimersMonotonic='{ OnActiveUSec=30min ; next_elapse=40min }',
            NextElapseUSecMonotonic='40min')
        self.service = dict(LoadState='loaded', Transient='yes', Type='oneshot', User='',
            DynamicUser='no', ExecStart='{ path=/usr/bin/systemctl ; argv[]=/usr/bin/systemctl start gdm.service ; ignore_errors=no ; pid=0 ; }')

    def test_registered_action_not_authenticated_login(self):
        r = qualify(self.timer, self.service)
        self.assertEqual(r['state'], 'REGISTERED_ROOT_GDM_RETURN')
        self.assertFalse(r['authenticated_gui_login_restored'])

    def test_absent_or_failed_timer_blocks(self):
        for timer in ({}, dict(self.timer, ActiveState='inactive'), dict(self.timer, SubState='elapsed')):
            self.assertEqual(qualify(timer, self.service)['state'], 'BLOCKED_RECOVERY_UNVERIFIED')

    def test_wrong_action_or_authority_blocks(self):
        for service in (dict(self.service, User='1000'), dict(self.service, DynamicUser='yes'),
                        dict(self.service, ExecStart='/usr/bin/systemctl start gdm.service'),
                        dict(self.service, ExecStart=self.service['ExecStart'].replace('start gdm', 'stop gdm'))):
            self.assertEqual(qualify(self.timer, service)['state'], 'BLOCKED_RECOVERY_UNVERIFIED')

    def test_unbounded_or_wrong_unit_blocks(self):
        for timer in (dict(self.timer, Unit='other.service'), dict(self.timer, TimersMonotonic=''),
                      dict(self.timer, NextElapseUSecMonotonic='infinity')):
            self.assertEqual(qualify(timer, self.service)['state'], 'BLOCKED_RECOVERY_UNVERIFIED')


if __name__ == '__main__':
    unittest.main()
