"""Portable runner regression checks; no elapsed-performance assertions."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

from run_p6_baselines import bounded_process, peak_rss


class BaselineRunnerTests(unittest.TestCase):
    def test_os_peak_units_and_missing_data(self):
        self.assertEqual(peak_rss(" 123 maximum resident set size", "Darwin"), 123)
        self.assertEqual(peak_rss("Maximum resident set size (kbytes): 123", "Linux"), 123 * 1024)
        self.assertIsNone(peak_rss("", "Darwin"))
        self.assertIsNone(peak_rss("", "Windows"))

    def test_exit_status_and_output_are_preserved(self):
        result = bounded_process([sys.executable, "-c", "print('partial');raise SystemExit(7)"], 10)
        self.assertEqual(result.returncode, 7)
        self.assertEqual(result.stdout.strip(), "partial")

    @unittest.skipIf(os.name == "nt", "POSIX time wrapper process-tree regression")
    def test_timeout_kills_descendant_not_only_wrapper(self):
        with tempfile.TemporaryDirectory() as folder:
            marker = Path(folder) / "orphan"
            child = "import time,pathlib;time.sleep(1);pathlib.Path(" + repr(str(marker)) + ").touch()"
            parent = "import subprocess,sys;subprocess.Popen([sys.executable,'-c'," + repr(child) + "]).wait()"
            with self.assertRaises(subprocess.TimeoutExpired):
                bounded_process([sys.executable, "-c", parent], 0.2)
            time.sleep(1.1)
            self.assertFalse(marker.exists(), "measurement descendant survived timeout")


if __name__ == "__main__":
    unittest.main()
