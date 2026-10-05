"""Integration tests against the real Godot/Jolt lab (headless).

Skipped when no Godot 4 executable is available (set ABIYSS_GODOT or put
``godot`` on PATH). Run only these with ``pytest -m sim``.
"""

from __future__ import annotations

import math
import shutil
import subprocess
from pathlib import Path
from typing import Iterator

import pytest

from abiyss.robotics import Robot
from abiyss.robotics import scenarios as sc
from abiyss.robotics.scenarios import Lab
from abiyss.robotics.sim_backend import default_project_dir, find_godot

pytestmark = [pytest.mark.sim, pytest.mark.skipif(find_godot() is None, reason="Godot 4 not installed")]


@pytest.fixture(scope="module")
def lab(tmp_path_factory: pytest.TempPathFactory) -> Iterator[Lab]:
    out = tmp_path_factory.mktemp("robotics")
    robot = Robot.simulation(tool="gripper", headless=True, journal_path=out / "telemetry.jsonl", godot_log=out / "godot.log")
    try:
        yield Lab(robot, out)
    finally:
        robot.close()


def _assert_passed(result) -> None:
    failed = [c for c in result.checks if not c["ok"]]
    assert result.passed, failed


def test_godot_side_unit_tests() -> None:
    godot = find_godot()
    proc = subprocess.run([godot, "--headless", "--path", str(default_project_dir()), "--script", "res://tests/run_tests.gd"], capture_output=True, text=True, timeout=600)
    assert proc.returncode == 0, proc.stdout[-3000:] + proc.stderr[-3000:]
    assert "0 failed" in proc.stdout


def test1_write_oi(lab: Lab) -> None:
    _assert_passed(sc.scenario_write_oi(lab))


def test2_pick_four_objects(lab: Lab) -> None:
    r = sc.scenario_pick_four(lab)
    _assert_passed(r)
    outcomes = r.metrics["outcomes"]
    assert outcomes["D"] == "torque_insufficient" and outcomes["C_light_grip"] == "escaped"


def test3_shoulder_stall(lab: Lab) -> None:
    _assert_passed(sc.scenario_shoulder_stall(lab))


def test4_repeatability(lab: Lab) -> None:
    r = sc.scenario_repeatability(lab, n=20)
    _assert_passed(r)
    assert len(r.metrics["same_side"]["trials"]) == 20


def test5_gravity(lab: Lab) -> None:
    _assert_passed(sc.scenario_gravity(lab))


def test_emergency_stop(lab: Lab) -> None:
    _assert_passed(sc.scenario_emergency_stop(lab))


def test_free_base_loses_stability(lab: Lab) -> None:
    r = sc.scenario_stability(lab)
    _assert_passed(r)


def test_lockstep_determinism(lab: Lab) -> None:
    def run() -> list[float]:
        lab.reset("gripper", seed=99)
        lab.run_actions([{"action": "move_to", "x": 0.15, "y": 0.04, "z": 0.05}, {"action": "home"}], timeout=20)
        st = lab.ctl.state
        return [st["joints"][j]["link_pos"] for j in ("base_yaw", "shoulder", "elbow", "wrist")] + list(st["tcp"]["position"])
    assert run() == run()


def test_queue_executes_through_physics_not_teleport(lab: Lab) -> None:
    lab.reset("gripper")
    rec = lab.robot.arm.move_to(0.15, 0.0, 0.05, wait=False)
    lab.ctl.tick()
    lab.ctl.tick()
    # Two PWM periods after the command the arm has barely started moving.
    tcp = lab.ctl.state["tcp"]["position"]
    assert math.dist(tcp, (0.15, 0.0, 0.05)) > 0.03
    lab.robot.run(30)
    assert rec.status.value == "done"
    assert math.dist(lab.ctl.state["tcp"]["position"], (0.15, 0.0, 0.05)) < 0.02


@pytest.mark.skipif(shutil.which("xvfb-run") is None, reason="needs xvfb-run to render camera frames")
def test_camera_frame_rendered(tmp_path: Path) -> None:
    with Robot.simulation(tool="gripper", headless=False, godot_log=tmp_path / "godot.log") as robot:
        frame = robot.cam.frame()
        assert (frame.width, frame.height) == (320, 240)
        assert frame.data.startswith(b"\x89PNG\r\n\x1a\n")
        assert frame.pose["position"] and frame.intrinsics["fov_y_deg"] == 70.0
