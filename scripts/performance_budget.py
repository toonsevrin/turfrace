#!/usr/bin/env python3
"""Aggregate Turfrace benchmark JSON without treating headless timing as a gate."""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import json
import math
import platform
import shutil
import subprocess
from pathlib import Path
from typing import Any

_ALLOWED_VIEWS = {1, 2, 4, 8}
_ALLOWED_SCENARIOS = {"idle", "capture-heavy"}
_MATCH_SEED = 0x5EEDCAFE
_NPC_ROSTER_SEED = 0x5EEDBEEF


def percentile(values: list[float], quantile: float) -> float | None:
    """Return a nearest-rank-free interpolated percentile for small samples."""
    if not values:
        return None
    if not 0.0 <= quantile <= 1.0:
        raise ValueError("quantile must be between 0 and 1")
    ordered = sorted(values)
    position = (len(ordered) - 1) * quantile
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    fraction = position - lower
    return ordered[lower] + (ordered[upper] - ordered[lower]) * fraction


def _number(value: Any, name: str) -> None:
    if not isinstance(value, (int, float)) or isinstance(value, bool) or not math.isfinite(value):
        raise ValueError(f"{name} must be a finite number")


def _integer(value: Any, name: str, minimum: int = 0) -> None:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise ValueError(f"{name} must be an integer >= {minimum}")


def validate_record(record: dict[str, Any]) -> None:
    """Reject incomplete or mixed benchmark records before writing an aggregate."""
    if not isinstance(record, dict):
        raise ValueError("benchmark record must be an object")
    if record.get("schema_version") != 1:
        raise ValueError("benchmark record schema_version must be 1")
    views = record.get("views")
    if views not in _ALLOWED_VIEWS:
        raise ValueError("benchmark record views must be one of 1, 2, 4, or 8")
    if record.get("scenario") not in _ALLOWED_SCENARIOS:
        raise ValueError("benchmark record has an unknown scenario")
    if record.get("status") != "ok" or record.get("error") is not None:
        raise ValueError("failed benchmark records cannot be aggregated")
    canvas = record.get("canvas")
    if not isinstance(canvas, dict):
        raise ValueError("benchmark record must have a positive canvas")
    _integer(canvas.get("width"), "canvas.width", 1)
    _integer(canvas.get("height"), "canvas.height", 1)
    warmup = record.get("warmup_frames")
    measured = record.get("measured_frames")
    _integer(warmup, "warmup_frames")
    _integer(measured, "measured_frames", 1)

    startup = record.get("startup")
    if not isinstance(startup, dict) or startup.get("source") != "native_process_wall_clock":
        raise ValueError("benchmark record is missing native startup timing")
    for name in ("startup_to_first_first_ms", "startup_to_readiness_ms", "readiness_wait_ms"):
        _number(startup.get(name), f"startup.{name}")
        if startup[name] < 0:
            raise ValueError(f"startup.{name} cannot be negative")
    if startup["startup_to_readiness_ms"] <= 0:
        raise ValueError("benchmark record did not measure presentation readiness")

    timing = record.get("timing")
    if not isinstance(timing, dict) or timing.get("source") != "native_process_wall_clock_first_to_first":
        raise ValueError("benchmark record is missing First-to-First timing")
    _integer(timing.get("sample_count"), "timing.sample_count", 1)
    if timing["sample_count"] != measured:
        raise ValueError("timing sample count does not match measured_frames")
    for name in ("median_ms", "p95_ms", "min_ms", "max_ms", "equivalent_p95_fps"):
        _number(timing.get(name), f"timing.{name}")
        if timing[name] < 0:
            raise ValueError(f"timing.{name} cannot be negative")
    if (
        timing["equivalent_p95_fps"] <= 0
        or timing["max_ms"] < timing["min_ms"]
        or not timing["min_ms"] <= timing["median_ms"] <= timing["max_ms"]
        or not timing["min_ms"] <= timing["p95_ms"] <= timing["max_ms"]
    ):
        raise ValueError("timing frame bounds are invalid")
    if "fixed_update_cpu_ms" not in timing or timing["fixed_update_cpu_ms"] is not None:
        raise ValueError("fixed-update CPU timing must remain explicitly null")

    simulation = record.get("simulation")
    if not isinstance(simulation, dict) or simulation.get("competitors") != 12:
        raise ValueError("benchmark must preserve the twelve-competitor workload")
    _integer(simulation.get("competitors"), "simulation.competitors", 12)
    if simulation.get("match_seed") != _MATCH_SEED or simulation.get("npc_roster_seed") != _NPC_ROSTER_SEED:
        raise ValueError("benchmark seeds do not match the canonical workload")
    if simulation.get("npc_difficulty") != "normal":
        raise ValueError("benchmark must use normal NPC difficulty")
    _integer(simulation.get("npc_controllers"), "simulation.npc_controllers")
    _integer(simulation.get("human_controllers"), "simulation.human_controllers")
    _integer(simulation.get("human_competitors"), "simulation.human_competitors")
    _integer(simulation.get("captures_observed"), "simulation.captures_observed")
    _number(simulation.get("player_speed"), "simulation.player_speed")
    _number(simulation.get("match_elapsed_seconds"), "simulation.match_elapsed_seconds")
    if simulation["player_speed"] < 0:
        raise ValueError("simulation.player_speed cannot be negative")
    if record["scenario"] == "idle" and simulation["player_speed"] != 0:
        raise ValueError("idle benchmark must use zero player speed")
    if record["scenario"] == "capture-heavy" and simulation["player_speed"] <= 0:
        raise ValueError("capture-heavy benchmark must use normal nonzero speed")
    if simulation["match_elapsed_seconds"] < 0:
        raise ValueError("simulation.match_elapsed_seconds cannot be negative")
    if simulation["npc_controllers"] != 12 or simulation["human_controllers"] != 0:
        raise ValueError("benchmark must use twelve NPC controllers and no human controllers")
    if simulation["human_competitors"] != views:
        raise ValueError("benchmark human presentation subjects do not match views")
    expected_capture_verification = record["scenario"] == "idle" or simulation["captures_observed"] > 0
    if simulation.get("capture_activity_verified") is not expected_capture_verification:
        raise ValueError("capture activity verification is inconsistent with observed captures")
    cameras = record.get("cameras")
    _integer(record.get("camera_count"), "camera_count")
    if not isinstance(cameras, list) or record["camera_count"] != len(cameras) or len(cameras) != views:
        raise ValueError("camera_count does not match the requested views")
    slots = []
    for index, camera in enumerate(cameras):
        if not isinstance(camera, dict):
            raise ValueError(f"cameras[{index}] must be an object")
        _integer(camera.get("slot"), f"cameras[{index}].slot")
        slots.append(camera["slot"])
        for name in ("subject_x", "subject_y"):
            _number(camera.get(name), f"cameras[{index}].{name}")
        for name in ("x", "y", "width", "height", "pixels"):
            _integer(camera.get(name), f"cameras[{index}].{name}")
        if camera["width"] <= 0 or camera["height"] <= 0 or camera["pixels"] <= 0:
            raise ValueError(f"cameras[{index}] has an empty viewport")
        if camera["pixels"] != camera["width"] * camera["height"]:
            raise ValueError(f"cameras[{index}] has an invalid pixel area")
        if (
            camera["x"] + camera["width"] > canvas["width"]
            or camera["y"] + camera["height"] > canvas["height"]
        ):
            raise ValueError(f"cameras[{index}] lies outside the canvas")
        if (
            "cpu_ms" not in camera
            or "gpu_ms" not in camera
            or camera["cpu_ms"] is not None
            or camera["gpu_ms"] is not None
        ):
            raise ValueError("per-camera CPU/GPU timing must remain explicitly null")
    if sorted(slots) != list(range(views)):
        raise ValueError("camera slots are not the canonical view slots")
    validation = record.get("validation")
    if (
        not isinstance(validation, dict)
        or validation.get("canonical_view_cameras") is not True
        or validation.get("controller_workload") is not True
        or validation.get("browser_verified") is not False
        or validation.get("hardware_verified") is not False
        or validation.get("memory_verified") is not False
    ):
        raise ValueError("benchmark validation flags are incomplete or make unverified claims")


def release_artifact(root: Path) -> dict[str, Any]:
    """Measure an existing dist WASM artifact, or state why it was not measured."""
    files = sorted(path for path in (root / "dist").glob("*.wasm") if path.is_file())
    if len(files) != 1:
        return {
            "status": "not_measured",
            "raw_bytes": None,
            "gzip_bytes": None,
            "brotli_bytes": None,
            "reason": "Expected exactly one existing dist/*.wasm; run scripts/build-web first.",
        }
    wasm = files[0].read_bytes()
    result: dict[str, Any] = {
        "status": "measured_existing_artifact",
        "path": str(files[0]),
        "raw_bytes": len(wasm),
        "gzip_bytes": len(gzip.compress(wasm, compresslevel=9, mtime=0)),
        "brotli_bytes": None,
    }
    brotli = shutil.which("brotli")
    if brotli:
        completed = subprocess.run(
            [brotli, "--quality=11", "--stdout"],
            input=wasm,
            stdout=subprocess.PIPE,
            check=True,
        )
        result["brotli_bytes"] = len(completed.stdout)
    else:
        result["brotli_reason"] = "brotli executable unavailable"
    return result


def aggregate(records: list[dict[str, Any]], root: Path) -> dict[str, Any]:
    if not records:
        raise ValueError("at least one benchmark record is required")
    for record in records:
        validate_record(record)
    first = records[0]
    for record in records[1:]:
        if record["scenario"] != first["scenario"] or record["canvas"] != first["canvas"]:
            raise ValueError("all records must use the same scenario and canvas")
        if record["warmup_frames"] != first["warmup_frames"] or record["measured_frames"] != first["measured_frames"]:
            raise ValueError("all records must use the same warmup and measured frame counts")
    view_counts = [record["views"] for record in records]
    if len(set(view_counts)) != len(view_counts):
        raise ValueError("each view count may occur only once")
    return {
        "schema_version": 1,
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "benchmark": {
            "scenario": first["scenario"],
            "canvas": first["canvas"],
            "warmup_frames": first["warmup_frames"],
            "measured_frames": first["measured_frames"],
            "view_counts": view_counts,
            "workload": "12 seeded Normal NPC controllers (match 0x5eedcafe, roster 0x5eedbeef); first N Competitor.kind values expose local cameras",
        },
        "environment": {
            "host_platform": platform.platform(),
            "renderer_backend": "configured by caller; inspect stderr and VK_ICD_FILENAMES",
            "browser_verified": False,
            "hardware_verified": False,
            "memory_verified": False,
            "verification_note": "Native headless timings may use software Vulkan and are not browser or physical-GPU acceptance evidence.",
        },
        "budgets": {
            "target_fps": {
                "target": 60,
                "minimum": 45,
                "status": "unverified",
                "reason": "Requires a physical browser run at the target viewport and quality setting.",
            },
            "fixed_update_p95_cpu_ms": {
                "target": 8,
                "status": "unverified",
                "reason": "The production app exposes no fixed-system CPU timer; benchmark wall time includes rendering and scheduling.",
            },
            "memory_after_loading_mb": {
                "target": 256,
                "status": "unverified",
                "reason": "Requires browser task-manager/devtools or a native allocator measurement.",
            },
        },
        "release_artifact": release_artifact(root),
        "runs": records,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    try:
        records = [json.loads(path.read_text()) for path in args.inputs]
        report = aggregate(records, args.root)
    except (OSError, json.JSONDecodeError, TypeError, ValueError) as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"PERFORMANCE_REPORT {args.output}")


if __name__ == "__main__":
    main()
