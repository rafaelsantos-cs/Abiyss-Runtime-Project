"""Simulation backend: the Godot 4 / Jolt robot lab driven in lockstep.

The Godot process owns rigid-body physics and the servo electromechanics; this
module owns nothing physical. It sends one pulse width per servo per control
period and asks Godot to advance exactly ``dt * physics_hz`` ticks.
"""

from __future__ import annotations

import base64
import json
import os
import shutil
import socket
import subprocess
import time
from pathlib import Path
from typing import Any

from .backend import BackendCapabilities, BackendError, CameraFrame, CameraSensor, RobotBackend, ServoChannel
from .specs import ArmSpecs, repo_root

PROTOCOL_VERSION = 1


def find_godot() -> str | None:
    env = os.environ.get("ABIYSS_GODOT")
    if env:
        return env
    for name in ("godot", "godot4", "Godot_v4"):
        path = shutil.which(name)
        if path:
            return path
    return None


def default_project_dir() -> Path:
    """$ABIYSS_ROBOT_LAB, else the repository's robot_lab/ (run from a checkout)."""
    env = os.environ.get("ABIYSS_ROBOT_LAB")
    return Path(env) if env else repo_root() / "robot_lab"


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


class SimulationServo(ServoChannel):
    def __init__(self, name: str, backend: "SimulationBackend") -> None:
        super().__init__(name)
        self._backend = backend

    def configure(self, **params: Any) -> None:
        self._backend.request("configure", servo=self.name, params=params)


class SimulationCamera(CameraSensor):
    def __init__(self, backend: "SimulationBackend") -> None:
        self._backend = backend

    def frame(self) -> CameraFrame:
        r = self._backend.request("camera", timeout=60.0)
        return CameraFrame(
            frame_id=int(r["frame_id"]),
            sim_time=float(r["sim_time"]),
            width=int(r["width"]),
            height=int(r["height"]),
            format=str(r["format"]),
            data=base64.b64decode(r["data"]),
            pose=dict(r.get("pose", {})),
            intrinsics=dict(r.get("intrinsics", {})),
            sensor=str(r.get("sensor", "SimulationCamera")),
        )


class SimulationBackend(RobotBackend):
    """Lockstep client of ``robot_lab`` (Godot 4.x, Jolt Physics)."""

    def __init__(
        self,
        specs: ArmSpecs,
        *,
        launch: bool = True,
        headless: bool = True,
        port: int | None = None,
        godot: str | None = None,
        project_dir: str | Path | None = None,
        log_path: str | Path | None = None,
        render_fps: int | None = None,
        rendering_driver: str | None = None,
        connect_timeout: float = 60.0,
        view: str = "overview",
    ) -> None:
        self.specs = specs
        self.launch = launch
        self.headless = headless
        self.port = port or (_free_port() if launch else 47011)
        self.godot = godot or find_godot()
        self.project_dir = Path(project_dir) if project_dir else default_project_dir()
        self.log_path = Path(log_path) if log_path else None
        self.render_fps = render_fps
        self.rendering_driver = rendering_driver or os.environ.get("ABIYSS_GODOT_RENDERING_DRIVER", "opengl3")
        self.connect_timeout = connect_timeout
        self.view = view
        self.process: subprocess.Popen[bytes] | None = None
        self._sock: socket.socket | None = None
        self._rfile: Any = None
        self._next_id = 1
        self._servos: dict[str, ServoChannel] = {}
        self._camera = SimulationCamera(self)
        self.info: dict[str, Any] = {}
        self.physics_hz = int(specs.physics_rate_hz)
        self.last_state: dict[str, Any] = {}
        self.ui_events: list[dict[str, Any]] = []
        self.capabilities = BackendCapabilities(
            name="simulation",
            deterministic=True,
            position_feedback=True,
            torque_feedback=True,
            current_feedback=True,
            configurable_servo_controller=True,
            camera=not headless,
            ground_truth=True,
        )
        for name in [*specs.joint_names, "gripper"]:
            self._servos[name] = SimulationServo(name, self)

    # -- process / connection -------------------------------------------------

    def _command(self) -> list[str]:
        if self.godot is None:
            raise BackendError("Godot 4 executable not found (set ABIYSS_GODOT or put 'godot' on PATH)")
        user = [f"--port={self.port}", f"--specs={self.specs.path}", f"--view={self.view}"]
        if self.headless:
            fps = self.render_fps or 50
            return [self.godot, "--headless", "--path", str(self.project_dir), "--fixed-fps", str(fps), "--", *user]
        fps = self.render_fps or 10
        cmd = [self.godot, "--path", str(self.project_dir), "--rendering-driver", self.rendering_driver, "--audio-driver", "Dummy", "--fixed-fps", str(fps), "--", *user]
        if not os.environ.get("DISPLAY") and not os.environ.get("WAYLAND_DISPLAY"):
            xvfb = shutil.which("xvfb-run")
            if xvfb is None:
                raise BackendError("no display and xvfb-run not found: cannot render; use headless=True")
            cmd = [xvfb, "-a", "-s", "-screen 0 1600x900x24", *cmd]
        return cmd

    def connect(self) -> dict[str, Any]:
        if self.launch:
            cmd = self._command()
            log = open(self.log_path, "ab") if self.log_path else subprocess.DEVNULL
            self.process = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        deadline = time.monotonic() + self.connect_timeout
        last_err: Exception | None = None
        while time.monotonic() < deadline:
            if self.process is not None and self.process.poll() is not None:
                raise BackendError(f"Godot exited with code {self.process.returncode} before accepting connections")
            try:
                self._sock = socket.create_connection(("127.0.0.1", self.port), timeout=2.0)
                break
            except OSError as exc:
                last_err = exc
                time.sleep(0.1)
        if self._sock is None:
            self.close()
            raise BackendError(f"cannot connect to robot lab on port {self.port}: {last_err}")
        self._sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self._rfile = self._sock.makefile("rb")
        self.info = self.request("hello")
        if int(self.info.get("protocol", -1)) != PROTOCOL_VERSION:
            raise BackendError(f"protocol mismatch: lab speaks {self.info.get('protocol')}, client {PROTOCOL_VERSION}")
        self.physics_hz = int(self.info.get("physics_hz", self.physics_hz))
        self.capabilities = BackendCapabilities(**{**self.capabilities.as_dict(), "camera": bool(self.info.get("camera_available"))})
        return self.info

    def request(self, op: str, timeout: float = 30.0, **fields: Any) -> dict[str, Any]:
        if self._sock is None:
            raise BackendError("backend not connected")
        rid = self._next_id
        self._next_id += 1
        payload = json.dumps({"id": rid, "op": op, **fields}, separators=(",", ":"), allow_nan=False) + "\n"
        self._sock.settimeout(timeout)
        try:
            self._sock.sendall(payload.encode("utf-8"))
            line = self._rfile.readline()
        except (OSError, ValueError) as exc:
            raise BackendError(f"{op}: transport error: {exc}") from exc
        if not line:
            code = self.process.poll() if self.process else None
            raise BackendError(f"{op}: robot lab closed the connection (process exit code {code})")
        reply = json.loads(line)
        if reply.get("id") is not None and int(reply["id"]) != rid:
            raise BackendError(f"{op}: out-of-order reply id {reply.get('id')} != {rid}")
        if not reply.get("ok", False):
            raise BackendError(f"{op}: {reply.get('error', 'unknown error')}")
        return reply

    def close(self) -> None:
        if self._sock is not None:
            try:
                self.request("quit", timeout=5.0)
            except Exception:
                pass
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None
        if self.process is not None:
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(self.process.pid, 9)
                except OSError:
                    self.process.kill()
                self.process.wait(timeout=5)
            self.process = None

    # -- RobotBackend -----------------------------------------------------------

    @property
    def servos(self) -> dict[str, ServoChannel]:
        return self._servos

    def reset(self, *, tool: str, seed: int, specs_override: dict[str, Any] | None = None) -> dict[str, Any]:
        fields: dict[str, Any] = {"tool": tool, "seed": int(seed)}
        if specs_override is not None:
            fields["specs"] = specs_override
        r = self.request("reset", timeout=60.0, **fields)
        self.masses = r.get("masses", {})
        self.last_state = r["state"]
        st = self.last_state
        for name, ch in self._servos.items():
            j = st["gripper"] if name == "gripper" else st["joints"][name]
            ch.pulse_us = float(j["pulse_us"])
            ch.powered = bool(j["powered"])
        return st

    def advance(self, dt: float) -> dict[str, Any]:
        ticks = max(1, int(round(dt * self.physics_hz)))
        cmds = {name: {"pulse_us": ch.pulse_us, "power": ch.powered} for name, ch in self._servos.items()}
        r = self.request("step", ticks=ticks, servos=cmds)
        self.last_state = r["state"]
        self.ui_events.extend(r.get("ui_events", []))
        return self.last_state

    def camera(self) -> CameraSensor:
        return self._camera

    def hud(self, status: dict[str, Any]) -> None:
        if not self.headless:
            self.request("hud", status=status)

    def set_view(self, name: str) -> None:
        if not self.headless:
            self.request("view", name=name)

    def screenshot(self, path: str | Path) -> Path:
        p = Path(path).resolve()
        p.parent.mkdir(parents=True, exist_ok=True)
        self.request("screenshot", path=str(p), timeout=60.0)
        return p

    def ink(self) -> list[list[list[float]]]:
        return self.request("ink")["strokes"]

    def clear_ink(self) -> None:
        self.request("clear_ink")

    def drain_ui_events(self) -> list[dict[str, Any]]:
        out, self.ui_events = self.ui_events, []
        return out
