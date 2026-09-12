import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).parents[1] / "demo_monitor.py"
SPEC = importlib.util.spec_from_file_location("demo_monitor", MODULE_PATH)
demo_monitor = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = demo_monitor
SPEC.loader.exec_module(demo_monitor)


class DemoMonitorTests(unittest.TestCase):
    def test_framing_discards_partial_head_and_keeps_details_together(self):
        lines = ["    old detail\n", "\n", "12:00 [AES-256-GCM] [TRANSFER] restored\n",
                 "    peer A: 2 pieces\n", "    peer B: 3 pieces\n", "\n"]
        self.assertEqual(list(demo_monitor.framed_blocks(lines)), [(
            "12:00 [AES-256-GCM] [TRANSFER] restored",
            "    peer A: 2 pieces", "    peer B: 3 pieces")])

    def test_remote_path_is_shell_quoted(self):
        source = demo_monitor.Source("vm1", "demo@vm:/var/lib/qfs demo/events.log")
        self.assertEqual(demo_monitor.tail_command(source), [
            "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--", "demo@vm",
            "tail -n 80 -F -- '/var/lib/qfs demo/events.log'"])

    def test_rejects_option_like_remote_host_and_removes_bidi_controls(self):
        with self.assertRaises(Exception):
            demo_monitor.parse_source("vm=-bad:/var/log/events")
        self.assertEqual(demo_monitor.sanitize("safe\u202eevil"), "safe?evil")

    def test_local_command_is_argument_safe(self):
        source = demo_monitor.Source("local", "/tmp/qfs events.log")
        self.assertEqual(demo_monitor.tail_command(source),
                         ["tail", "-n", "80", "-F", "--", "/tmp/qfs events.log"])


if __name__ == "__main__":
    unittest.main()
