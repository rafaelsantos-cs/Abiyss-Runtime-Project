"""Agent-facing Robot API.

    robot = Robot.simulation(tool="pen")       # or Robot(controller) with any backend
    robot.queue([{"action": "write", "text": "OI"}])
    robot.run()                                  # executes through the physics
    robot.arm.move_to(0.15, 0.0, 0.05)           # blocking by default
    robot.arm.move_joint("shoulder", 90)
    robot.gripper.open(); robot.gripper.close()
    robot.arm.stall_check(); robot.cam.frame()
    robot.status(); robot.telemetry()
    robot.arm.stop(); robot.emergency_stop(); robot.reset()

The agent decides WHAT to do; the controller and the backend decide HOW it
physically happens. Nothing here writes positions into the simulation.
Blocking calls advance simulated time deterministically; ``wait=False``
only enqueues.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Iterable

from .action_queue import ActionRecord, ActionStatus
from .backend import CameraFrame, RobotBackend
from .controller import RobotController
from .sim_backend import SimulationBackend
from .specs import ArmSpecs, load_specs
from .telemetry import TelemetryJournal


class _Part:
    def __init__(self, robot: "Robot") -> None:
        self._robot = robot

    def _do(self, action: dict[str, Any], wait: bool, timeout: float) -> ActionRecord:
        return self._robot.do(action, wait=wait, timeout=timeout)


class Arm(_Part):
    def move_to(self, x: float, y: float, z: float, *, pitch_deg: float = -90.0, speed: float | None = None, wait: bool = True, timeout: float = 30.0) -> ActionRecord:
        a: dict[str, Any] = {"action": "move_to", "x": x, "y": y, "z": z, "pitch_deg": pitch_deg}
        if speed is not None:
            a["speed"] = speed
        return self._do(a, wait, timeout)

    def move_line(self, x: float, y: float, z: float, *, pitch_deg: float = -90.0, speed_mps: float = 0.02, wait: bool = True, timeout: float = 60.0) -> ActionRecord:
        return self._do({"action": "move_line", "x": x, "y": y, "z": z, "pitch_deg": pitch_deg, "speed_mps": speed_mps}, wait, timeout)

    def move_joint(self, joint: str, angle_deg: float, *, speed: float | None = None, wait: bool = True, timeout: float = 30.0) -> ActionRecord:
        a: dict[str, Any] = {"action": "move_joint", "joint": joint, "angle_deg": angle_deg}
        if speed is not None:
            a["speed"] = speed
        return self._do(a, wait, timeout)

    def move_joints(self, angles_deg: dict[str, float], *, speed: float | None = None, wait: bool = True, timeout: float = 30.0) -> ActionRecord:
        a: dict[str, Any] = {"action": "move_joints", "angles_deg": angles_deg}
        if speed is not None:
            a["speed"] = speed
        return self._do(a, wait, timeout)

    def home(self, *, wait: bool = True, timeout: float = 30.0) -> ActionRecord:
        return self._do({"action": "home"}, wait, timeout)

    def write(self, text: str, *, wait: bool = True, timeout: float = 300.0, **options: Any) -> ActionRecord:
        return self._do({"action": "write", "text": text, **options}, wait, timeout)

    def pick(self, x: float, y: float, z: float = 0.0125, *, wait: bool = True, timeout: float = 60.0, **options: Any) -> ActionRecord:
        return self._do({"action": "pick", "x": x, "y": y, "z": z, **options}, wait, timeout)

    def place(self, x: float, y: float, z: float = 0.0125, *, wait: bool = True, timeout: float = 60.0, **options: Any) -> ActionRecord:
        return self._do({"action": "place", "x": x, "y": y, "z": z, **options}, wait, timeout)

    def stop(self) -> None:
        self._robot.controller.stop("arm.stop()")

    def stall_check(self) -> dict[str, Any]:
        return self._robot.controller.stall_check()

    def power(self, joint: str, on: bool, *, wait: bool = True) -> ActionRecord:
        return self._do({"action": "servo_power", "joint": joint, "on": on}, wait, 10.0)

    def set_pid(self, joint: str, *, kp: float | None = None, ki: float | None = None, kd: float | None = None, wait: bool = True) -> ActionRecord:
        a: dict[str, Any] = {"action": "set_pid", "joint": joint}
        for k, v in (("kp", kp), ("ki", ki), ("kd", kd)):
            if v is not None:
                a[k] = v
        return self._do(a, wait, 10.0)

    def step_response(self, joint: str, delta_deg: float, *, duration_s: float = 1.5) -> dict[str, Any]:
        rec = self._do({"action": "step_response", "joint": joint, "delta_deg": delta_deg, "duration_s": duration_s}, True, duration_s + 10.0)
        return rec.result


class Gripper(_Part):
    def open(self, width_mm: float | None = None, *, wait: bool = True) -> ActionRecord:
        a: dict[str, Any] = {"action": "gripper_open"}
        if width_mm is not None:
            a["width_mm"] = width_mm
        return self._do(a, wait, 10.0)

    def close(self, width_mm: float | None = None, *, hold_s: float = 0.5, wait: bool = True) -> ActionRecord:
        a: dict[str, Any] = {"action": "gripper_close", "hold_s": hold_s}
        if width_mm is not None:
            a["width_mm"] = width_mm
        return self._do(a, wait, 10.0)


class Cam(_Part):
    def frame(self) -> CameraFrame:
        return self._robot.controller.camera_frame()


class Robot:
    def __init__(self, controller: RobotController) -> None:
        self.controller = controller
        self.arm = Arm(self)
        self.gripper = Gripper(self)
        self.cam = Cam(self)

    # -- construction -----------------------------------------------------

    @classmethod
    def simulation(
        cls,
        *,
        tool: str = "gripper",
        seed: int | None = None,
        headless: bool = True,
        specs: ArmSpecs | str | Path | None = None,
        journal_path: str | Path | None = None,
        godot_log: str | Path | None = None,
        render_fps: int | None = None,
        view: str = "overview",
        mirror_audit: Any = None,
    ) -> "Robot":
        sp = specs if isinstance(specs, ArmSpecs) else load_specs(specs)
        backend = SimulationBackend(sp, headless=headless, log_path=godot_log, render_fps=render_fps, view=view)
        journal = TelemetryJournal(journal_path, backend="simulation", mirror=mirror_audit)
        ctl = RobotController(sp, backend, tool=tool, seed=seed, journal=journal)
        ctl.start()
        return cls(ctl)

    @classmethod
    def with_backend(cls, backend: RobotBackend, specs: ArmSpecs, **kwargs: Any) -> "Robot":
        ctl = RobotController(specs, backend, **kwargs)
        ctl.start()
        return cls(ctl)

    def close(self) -> None:
        self.controller.close()

    def __enter__(self) -> "Robot":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    # -- queue ----------------------------------------------------------------

    def do(self, action: dict[str, Any], *, wait: bool = True, timeout: float = 60.0) -> ActionRecord:
        rec = self.controller.submit(action)
        if wait:
            end = self.controller.sim_time + timeout
            while rec.status in (ActionStatus.PENDING, ActionStatus.RUNNING) and self.controller.sim_time < end:
                self.controller.tick()
        return rec

    def queue(self, actions: Iterable[dict[str, Any]], source: str = "api") -> list[ActionRecord]:
        return [self.controller.submit(a, source=source) for a in actions]

    def run(self, timeout_s: float = 600.0) -> bool:
        return self.controller.run_until_idle(timeout_s)

    def wait(self, seconds: float) -> None:
        """Let physics run without new commands (servos keep their targets)."""
        self.controller.run_for(seconds)

    # -- safety -------------------------------------------------------------

    def emergency_stop(self, reason: str = "api") -> None:
        self.controller.emergency_stop(reason)

    def reset(self) -> bool:
        return self.controller.reset_fault()

    def reset_world(self, tool: str | None = None, seed: int | None = None) -> None:
        self.controller.reset_world(tool, seed)

    # -- observation --------------------------------------------------------

    def status(self) -> dict[str, Any]:
        return self.controller.status()

    def telemetry(self) -> dict[str, Any]:
        return self.controller.telemetry()

    @property
    def journal(self) -> TelemetryJournal:
        return self.controller.journal
