#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import signal
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).with_name("simulation-performance")
loader = importlib.machinery.SourceFileLoader("simulation_performance", str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class SimulationPerformanceTests(unittest.TestCase):
    def test_summary_weights_means_and_keeps_p95_window_scoped(self):
        result = module.summarize(
            [
                {"samples": 2, "mean_ms": 2.0, "p95_ms": 3.0, "max_ms": 5.0, "ticks_over_16_67_ms": 1},
                {"samples": 4, "mean_ms": 6.0, "p95_ms": 8.0, "max_ms": 20.0, "ticks_over_16_67_ms": 2},
            ]
        )
        self.assertEqual(result["samples"], 6)
        self.assertAlmostEqual(result["weighted_mean_ms"], 14 / 3)
        self.assertEqual(result["maximum_window_p95_ms"], 8.0)
        self.assertEqual(result["max_tick_ms"], 20.0)
        self.assertEqual(result["ticks_over_16_67_ms"], 3)

    def test_summary_rejects_empty_or_invalid_windows(self):
        with self.assertRaises(ValueError):
            module.summarize([])
        with self.assertRaises(ValueError):
            module.summarize([{"samples": 0, "mean_ms": 1.0, "p95_ms": 1.0, "max_ms": 1.0, "ticks_over_16_67_ms": 0}])

    def test_output_names_and_command_are_repeatable(self):
        prefix = Path("target/performance/run")
        self.assertEqual(module.output_path(prefix, 8), Path("target/performance/run-8-npcs.jsonl"))
        self.assertEqual(
            module.benchmark_command(Path("bench"), 3600, 300, 42, 12),
            ["bench", "--ticks", "3600", "--window", "300", "--npcs", "12", "--seed", "42"],
        )

    def test_binary_path_honors_target_dir_relative_to_root(self):
        self.assertEqual(
            module.binary_path({"CARGO_TARGET_DIR": "build-cache"}),
            module.ROOT / "build-cache/release/examples/simulation_performance",
        )
        self.assertEqual(
            module.binary_path({"CARGO_TARGET_DIR": "/tmp/turfrace-target"}),
            Path("/tmp/turfrace-target/release/examples/simulation_performance"),
        )

    def test_timeout_kills_process_group_and_reaps_after_escalation(self):
        class FakeProcess:
            pid = 321
            returncode = -signal.SIGKILL

            def __init__(self):
                self.communicate_calls = 0

            def communicate(self, **kwargs):
                self.communicate_calls += 1
                if self.communicate_calls < 3:
                    raise subprocess.TimeoutExpired("benchmark", kwargs["timeout"])

        process = FakeProcess()
        with mock.patch.object(module.subprocess, "Popen", return_value=process) as popen, mock.patch.object(
            module.os, "killpg"
        ) as killpg:
            status = module.run_checked(["benchmark"], 1, "run", {}, stdout=subprocess.DEVNULL)

        self.assertEqual(status, 124)
        popen.assert_called_once_with(
            ["benchmark"], env={}, start_new_session=True, stdout=subprocess.DEVNULL
        )
        self.assertEqual(
            killpg.call_args_list,
            [mock.call(321, signal.SIGTERM), mock.call(321, signal.SIGKILL)],
        )
        self.assertEqual(process.communicate_calls, 3)

    def test_read_jsonl_rejects_malformed_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "run.jsonl"
            path.write_text("not json\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                module.read_jsonl(path)


if __name__ == "__main__":
    unittest.main()
