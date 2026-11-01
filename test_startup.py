"""Start-with-Windows regressions; uses a throwaway registry key, never the real Run key."""
import unittest
import winreg
from unittest.mock import patch

import startup

TEST_KEY = r"Software\AIUsageTrackerTest\Run"


class StartupTests(unittest.TestCase):
    def setUp(self):
        self.p = patch.object(startup, "RUN_KEY", TEST_KEY)
        self.p.start()

    def tearDown(self):
        self.p.stop()
        for key in (TEST_KEY, r"Software\AIUsageTrackerTest"):
            try:
                winreg.DeleteKey(winreg.HKEY_CURRENT_USER, key)
            except OSError:
                pass

    def test_enable_disable_round_trip(self):
        self.assertFalse(startup.is_enabled())
        startup.enable()
        self.assertEqual(startup.current(), startup.command())
        startup.disable()
        self.assertFalse(startup.is_enabled())
        startup.disable()  # already off is fine

    def test_refresh_repoints_a_moved_app_but_never_turns_it_on(self):
        startup.refresh()
        self.assertFalse(startup.is_enabled())
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, TEST_KEY) as key:
            winreg.SetValueEx(key, startup.NAME, 0, winreg.REG_SZ, '"C:\\old place\\AIUsageTracker.exe"')
        startup.refresh()
        self.assertEqual(startup.current(), startup.command())

    def test_frozen_exe_runs_itself(self):
        with patch.object(startup.sys, "frozen", True, create=True), \
                patch.object(startup.sys, "executable", r"C:\Apps\AIUsageTracker.exe"):
            self.assertEqual(startup.command(), '"C:\\Apps\\AIUsageTracker.exe"')


if __name__ == "__main__":
    unittest.main()
