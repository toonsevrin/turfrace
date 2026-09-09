import tempfile
import unittest
from pathlib import Path

from performance_budget import aggregate, percentile, release_artifact, validate_record


def record(**changes):
    value = {
        "schema_version": 1,
        "status": "ok",
        "error": None,
        "views": 8,
        "scenario": "idle",
        "canvas": {"width": 1920, "height": 1080},
        "warmup_frames": 2,
        "measured_frames": 4,
        "startup": {
            "source": "native_process_wall_clock",
            "startup_to_first_first_ms": 10.0,
            "startup_to_readiness_ms": 25.0,
            "readiness_wait_ms": 20.0,
        },
        "timing": {
            "source": "native_process_wall_clock_first_to_first",
            "sample_count": 4,
            "median_ms": 16.0,
            "p95_ms": 17.0,
            "min_ms": 15.0,
            "max_ms": 18.0,
            "equivalent_p95_fps": 58.8,
            "fixed_update_cpu_ms": None,
        },
        "simulation": {
            "competitors": 12,
            "match_seed": 0x5EEDCAFE,
            "npc_roster_seed": 0x5EEDBEEF,
            "npc_difficulty": "normal",
            "npc_controllers": 12,
            "human_controllers": 0,
            "human_competitors": 8,
            "player_speed": 0.0,
            "match_elapsed_seconds": 1.0,
            "captures_observed": 0,
            "capture_activity_verified": True,
        },
        "camera_count": 8,
        "cameras": [
            {
                "slot": index,
                "subject_x": 0.0,
                "subject_y": 0.0,
                "x": 0,
                "y": 0,
                "width": 240,
                "height": 135,
                "pixels": 32400,
                "cpu_ms": None,
                "gpu_ms": None,
            }
            for index in range(8)
        ],
        "validation": {
            "canonical_view_cameras": True,
            "controller_workload": True,
            "browser_verified": False,
            "hardware_verified": False,
            "memory_verified": False,
        },
    }
    value.update(changes)
    if "scenario" in changes:
        is_idle = value["scenario"] == "idle"
        value["simulation"]["player_speed"] = 0.0 if is_idle else 18.0
        value["simulation"]["capture_activity_verified"] = is_idle
    if "views" in changes:
        views = value["views"]
        value["simulation"]["human_competitors"] = views
        value["camera_count"] = views
        value["cameras"] = [
            {
                "slot": index,
                "subject_x": 0.0,
                "subject_y": 0.0,
                "x": 0,
                "y": 0,
                "width": 240,
                "height": 135,
                "pixels": 32400,
                "cpu_ms": None,
                "gpu_ms": None,
            }
            for index in range(views)
        ]
    return value


class PerformanceBudgetTests(unittest.TestCase):
    def test_percentile_interpolates_and_handles_empty_input(self):
        self.assertEqual(percentile([], 0.95), None)
        self.assertEqual(percentile([4.0, 1.0, 3.0, 2.0], 0.5), 2.5)

    def test_percentile_rejects_invalid_quantile(self):
        with self.assertRaises(ValueError):
            percentile([1.0], 1.1)

    def test_aggregate_keeps_hardware_budgets_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            report = aggregate([record()], Path(directory))
        self.assertEqual(report["budgets"]["target_fps"]["status"], "unverified")
        self.assertFalse(report["environment"]["browser_verified"])
        self.assertIsNone(report["release_artifact"]["raw_bytes"])

    def test_record_requires_first_to_first_samples_and_readiness(self):
        invalid = record()
        invalid["timing"] = None
        with self.assertRaises(ValueError):
            validate_record(invalid)
        invalid = record()
        invalid["startup"]["startup_to_readiness_ms"] = 0
        with self.assertRaises(ValueError):
            validate_record(invalid)

    def test_record_rejects_unmeasured_cpu_gpu_or_physical_claims(self):
        invalid = record()
        invalid["timing"]["fixed_update_cpu_ms"] = 8.0
        with self.assertRaises(ValueError):
            validate_record(invalid)
        invalid = record()
        invalid["cameras"][0]["gpu_ms"] = 1.0
        with self.assertRaises(ValueError):
            validate_record(invalid)
        invalid = record()
        invalid["validation"]["browser_verified"] = True
        with self.assertRaises(ValueError):
            validate_record(invalid)

    def test_record_requires_constant_controller_workload(self):
        invalid = record()
        invalid["simulation"]["npc_controllers"] = 8
        with self.assertRaises(ValueError):
            validate_record(invalid)
        invalid = record(views=4)
        invalid["simulation"]["human_competitors"] = 8
        with self.assertRaises(ValueError):
            validate_record(invalid)
        invalid = record()
        invalid["simulation"]["npc_roster_seed"] = 1
        with self.assertRaises(ValueError):
            validate_record(invalid)

    def test_capture_heavy_zero_capture_is_reported_not_invented(self):
        valid = record(scenario="capture-heavy")
        validate_record(valid)
        valid["simulation"]["capture_activity_verified"] = True
        with self.assertRaises(ValueError):
            validate_record(valid)

    def test_aggregate_rejects_mixed_or_duplicate_views(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                aggregate([record(views=1), record(views=1)], Path(directory))
            with self.assertRaises(ValueError):
                aggregate([record(views=1), record(views=2, scenario="capture-heavy")], Path(directory))

    def test_release_artifact_requires_one_wasm(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(
                release_artifact(Path(directory))["status"], "not_measured"
            )
            (Path(directory) / "dist").mkdir()
            (Path(directory) / "dist" / "placeholder.wasm").mkdir()
            self.assertEqual(
                release_artifact(Path(directory))["status"], "not_measured"
            )


if __name__ == "__main__":
    unittest.main()
