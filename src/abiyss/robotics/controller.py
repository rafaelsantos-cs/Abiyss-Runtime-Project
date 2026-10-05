"""Robot controller: queue -> motion programs -> servo pulses -> backend.

One call to :meth:`RobotController.tick` is one PWM period (20 ms, from the
datasheets). In each period the controller:

1. applies external requests (API server, HUD buttons) at the tick boundary;
2. advances the running action program by one period (programs are Python
   generators: every ``yield`` is one period of physics);
3. clamps targets to the soft limits, converts them to pulse widths with the
   PWM resolution of the output stage and hands them to the backend;
4. lets the backend advance time (lockstep physics or real time);
5. evaluates safety on the observed state and journals what happened.

The controller never writes positions into the world. It only chooses pulse
widths and power, exactly what it could do with real servos.
"""

from __future__ import annotations

import json
import math
import threading
from collections import deque
from typing import Any, Callable, Generator, Iterable

from ..errors import ValidationError
from .action_queue import ActionQueue, ActionRecord, ActionStatus
from .backend import BackendError, CameraFrame, CapabilityNotSupported, RobotBackend
from .kinematics import ARM_JOINTS, ArmKinematics, IKError, home_pose
from .safety import SafetyState, SafetySupervisor
from .specs import ArmSpecs, value
from .telemetry import TelemetryJournal, joint_sample
from .text import layout
from .trajectory import plan_joint_move, plan_line, plan_polyline

ALL_SERVOS = (*ARM_JOINTS, "gripper")
Program = Generator[None, None, dict[str, Any]]


class ActionFailed(Exception):
    """Raised inside a program to fail the current action with a reason."""


class RobotController:
    def __init__(
        self,
        specs: ArmSpecs,
        backend: RobotBackend,
        *,
        tool: str = "gripper",
        seed: int | None = None,
        journal: TelemetryJournal | None = None,
        hud_every: int = 5,
        speed_scale: float = 0.25,
    ) -> None:
        self.specs = specs
        self.backend = backend
        self.tool = tool
        self.seed = specs.default_seed if seed is None else int(seed)
        self.journal = journal or TelemetryJournal(None, backend=backend.capabilities.name)
        self.hud_every = max(1, hud_every)
        self.speed_scale = speed_scale
        self.dt = 1.0 / specs.control_rate_hz
        self.kin = ArmKinematics(specs, tool)
        self.queue = ActionQueue()
        self.safety = SafetySupervisor(specs)
        self.state: dict[str, Any] = {}
        self.sim_time = 0.0
        self.ticks = 0
        self.total_ticks = 0          # never reset: simulated seconds = total_ticks * dt
        self.cmd: dict[str, float] = {}
        self.power: dict[str, bool] = {n: True for n in ALL_SERVOS}
        self.cmd_tcp: tuple[float, float, float] | None = None
        self._program: Program | None = None
        self._inbox: deque[tuple[Callable[[], Any], threading.Event | None, list]] = deque()
        self._lock = threading.RLock()
        self.pid_metrics: dict[str, dict[str, Any]] = {}
        self.ui_handlers: dict[str, Callable[[], None]] = {}
        self.view_request: str | None = None
        self.trace_joint: str | None = None
        self.connected = False
        self.last_frame: CameraFrame | None = None

    # ------------------------------------------------------------------ setup

    def start(self) -> dict[str, Any]:
        info = self.backend.connect()
        self.connected = True
        self.journal.write("run_start", 0.0, backend_info=info, capabilities=self.backend.capabilities.as_dict(), specs=str(self.specs.path), seed=self.seed, tool=self.tool, control_rate_hz=self.specs.control_rate_hz)
        self.reset_world(self.tool, self.seed)
        return info

    def reset_world(self, tool: str | None = None, seed: int | None = None, *, base_mount: str | None = None) -> None:
        if tool is not None:
            self.tool = tool
            self.kin.set_tool(tool)
        if seed is not None:
            self.seed = int(seed)
        self.queue.cancel_all(self.sim_time, "world reset")
        self._program = None
        if base_mount is not None:
            # Only the mounting changes: kinematics and masses stay identical.
            raw = json.loads(json.dumps(self.specs.raw))
            raw["arm"]["base_mount"] = base_mount
            st = self.backend.reset(tool=self.tool, seed=self.seed, specs_override=raw)  # type: ignore[call-arg]
        else:
            st = self.backend.reset(tool=self.tool, seed=self.seed)
        self.state = st
        self.sim_time = float(st.get("sim_time", 0.0))
        self.ticks = 0
        self.safety = SafetySupervisor(self.specs)
        self.power = {n: True for n in ALL_SERVOS}
        self.cmd = home_pose(self.specs)
        self.cmd["gripper"] = self.specs.gripper.home
        self.cmd_tcp = self.kin.forward(self.cmd).tcp
        self.journal.write("reset", self.sim_time, tool=self.tool, seed=self.seed)

    def close(self) -> None:
        if self.connected:
            self.journal.write("run_end", self.sim_time, ticks=self.ticks, safety_state=self.safety.state.value, stalls=self.safety.stall_history)
            self.backend.close()
            self.connected = False
        self.journal.close()

    # -------------------------------------------------------------- utilities

    @property
    def has_feedback(self) -> bool:
        return self.backend.capabilities.position_feedback

    def joint_state(self, name: str) -> dict[str, Any]:
        if name == "gripper":
            return self.state.get("gripper", {})
        return self.state.get("joints", {}).get(name, {})

    def measured(self, name: str) -> float:
        j = self.joint_state(name)
        if self.has_feedback and "link_pos" in j:
            return float(j["link_pos"])
        return float(self.cmd[name])

    def measured_pose(self) -> dict[str, float]:
        return {n: self.measured(n) for n in ARM_JOINTS}

    def soft_limits(self, name: str) -> tuple[float, float]:
        if name == "gripper":
            return self.specs.gripper.soft_limits
        return self.specs.joint(name).soft_limits

    def pulse_for(self, name: str, angle: float) -> float:
        servo = self.specs.servo_for(name)
        if name == "gripper":
            center, direction = self.specs.gripper.center, 1.0
        else:
            j = self.specs.joint(name)
            center, direction = j.center, j.direction
        pulse = servo.pulse_neutral_us + direction * (angle - center) / (math.pi / 2.0) * servo.pulse_per_90_us
        res = self.specs.pwm_resolution_us
        return round(pulse / res) * res

    def max_speed(self, name: str) -> float:
        return self.specs.servo_for(name).no_load_speed

    def submit(self, action: dict[str, Any], source: str = "api") -> ActionRecord:
        with self._lock:
            if not self.safety.accepts_commands():
                self.journal.write("command_rejected", self.sim_time, action=action, source=source, reason=f"safety state {self.safety.state.value}")
                raise ValidationError(f"command rejected: robot in {self.safety.state.value} ({self.safety.reason}); call reset() first")
            rec = self.queue.submit(action, self.sim_time)
            self.journal.write("command", self.sim_time, action_id=rec.id, action=rec.action, params=rec.params, source=source)
            return rec

    def call_soon(self, fn: Callable[[], Any], wait: bool = False, timeout: float = 30.0) -> Any:
        """Run ``fn`` on the control thread at the next tick boundary."""
        ev = threading.Event() if wait else None
        box: list = []
        self._inbox.append((fn, ev, box))
        if ev is not None:
            if not ev.wait(timeout):
                raise TimeoutError("controller did not process the request in time")
            if box and isinstance(box[0], BaseException):
                raise box[0]
            return box[0] if box else None
        return None

    # ---------------------------------------------------------------- ticking

    def tick(self) -> dict[str, Any]:
        with self._lock:
            self._drain_inbox()
            self._handle_ui()
            if self.safety.accepts_commands():
                self._run_program()
            self._command_servos()
            try:
                self.state = self.backend.advance(self.dt)
            except BackendError as exc:
                self.safety._transition(SafetyState.FAULT, f"backend error: {exc}", self.sim_time)
                self.journal.write("backend_error", self.sim_time, error=str(exc))
                for ev in self.safety.drain_events():
                    self.journal.write(ev.pop("event"), ev.pop("sim_time", self.sim_time), **ev)
                raise
            self.sim_time = float(self.state.get("sim_time", self.sim_time + self.dt))
            self.ticks += 1
            self.total_ticks += 1
            self._evaluate_safety()
            self._journal_tick()
            if self.ticks % self.hud_every == 0:
                self._push_hud()
            return self.state

    def run_until_idle(self, timeout_s: float = 120.0) -> bool:
        """Tick until the queue is empty (or a latched safety state stops it)."""
        end = self.sim_time + timeout_s
        while self.sim_time < end:
            self.tick()
            if self.queue.idle() and self._program is None:
                return True
            if not self.safety.accepts_commands() and self._program is None:
                return False
        return False

    def run_for(self, seconds: float) -> None:
        end = self.sim_time + seconds
        while self.sim_time < end - 1e-9:
            self.tick()

    def _drain_inbox(self) -> None:
        while self._inbox:
            fn, ev, box = self._inbox.popleft()
            try:
                box.append(fn())
            except BaseException as exc:  # returned to the caller thread
                box.append(exc)
            if ev is not None:
                ev.set()

    def _handle_ui(self) -> None:
        drain = getattr(self.backend, "drain_ui_events", None)
        if drain is None:
            return
        for ev in drain():
            self.journal.write("ui_event", self.sim_time, **ev)
            button = str(ev.get("button", ""))
            if button.startswith("trace:"):
                self.trace_joint = button[6:]
                continue
            handler = self.ui_handlers.get(button)
            try:
                if button == "estop":
                    self.emergency_stop("E-STOP button (HUD)")
                elif button == "stop":
                    self.stop("stop button (HUD)")
                elif button == "reset":
                    self.reset_fault()
                elif handler is not None:
                    handler()
            except ValidationError as exc:
                self.journal.write("command_rejected", self.sim_time, source="ui", button=button, reason=str(exc))

    # ------------------------------------------------------------ programs

    def _run_program(self) -> None:
        if self._program is None:
            rec = self.queue.pop_next(self.sim_time)
            if rec is None:
                return
            self.journal.write("action_start", self.sim_time, action_id=rec.id, action=rec.action, params=rec.params)
            self._program = self._make_program(rec)
        try:
            next(self._program)
        except StopIteration as stop:
            result = stop.value or {}
            done = self.queue.finish(ActionStatus.DONE, self.sim_time, result=result)
            self._program = None
            if done is not None:
                self.journal.write("action_end", self.sim_time, action_id=done.id, action=done.action, status="done", duration=round(self.sim_time - (done.started_at or 0.0), 4), result=result)
        except (ActionFailed, IKError, ValidationError, CapabilityNotSupported) as exc:
            failed = self.queue.finish(ActionStatus.FAILED, self.sim_time, error=str(exc))
            self._program = None
            if failed is not None:
                self.journal.write("action_end", self.sim_time, action_id=failed.id, action=failed.action, status="failed", error=str(exc))
                self.journal.write("error", self.sim_time, action_id=failed.id, action=failed.action, error=str(exc))
            # Hold where we are: the commanded pose is left unchanged.

    def _make_program(self, rec: ActionRecord) -> Program:
        p = rec.params
        name = rec.action
        if name == "move_to":
            return self._p_move_to((p["x"], p["y"], p["z"]), math.radians(p.get("pitch_deg", -90.0)), p.get("speed"), p.get("settle_s", 0.2))
        if name == "move_line":
            return self._p_move_line((p["x"], p["y"], p["z"]), math.radians(p.get("pitch_deg", -90.0)), p.get("speed_mps", 0.02))
        if name == "move_joint":
            return self._p_move_joints({p["joint"]: math.radians(p["angle_deg"])}, p.get("speed"), p.get("settle_s", 0.2))
        if name == "move_joints":
            return self._p_move_joints({k: math.radians(v) for k, v in p["angles_deg"].items()}, p.get("speed"), 0.2)
        if name == "home":
            goal = home_pose(self.specs)
            return self._p_move_joints(goal, p.get("speed"), 0.3)
        if name == "gripper_open":
            width = p.get("width_mm")
            angle = self.specs.gripper.soft_limits[1] if width is None else self.specs.gripper.angle_for_opening(width / 1000.0)
            return self._p_gripper(angle, 0.3, "open")
        if name == "gripper_close":
            width = p.get("width_mm")
            angle = self.specs.gripper.soft_limits[0] if width is None else self.specs.gripper.angle_for_opening(width / 1000.0)
            return self._p_gripper(angle, p.get("hold_s", 0.5), "close")
        if name == "write":
            return self._p_write(p)
        if name == "pick":
            return self._p_pick(p)
        if name == "place":
            return self._p_place(p)
        if name == "wait":
            return self._p_wait(p["seconds"])
        if name == "servo_power":
            return self._p_power(p["joint"], p["on"])
        if name == "set_pid":
            return self._p_set_pid(p)
        if name == "step_response":
            return self._p_step_response(p["joint"], math.radians(p["delta_deg"]), p.get("duration_s", 1.5))
        raise ValidationError(f"no program for action {name}")

    def _check_limits(self, goal: dict[str, float]) -> None:
        for n, a in goal.items():
            lo, hi = self.soft_limits(n)
            if a < lo - 1e-9 or a > hi + 1e-9:
                raise ActionFailed(f"{n} target {math.degrees(a):.1f} deg outside soft limits [{math.degrees(lo):.0f}, {math.degrees(hi):.0f}]")

    def _wait_ticks(self, seconds: float) -> Generator[None, None, None]:
        for _ in range(max(0, int(round(seconds / self.dt)))):
            yield

    def _p_move_joints(self, goal: dict[str, float], speed: float | None, settle: float) -> Program:
        self._check_limits(goal)
        start = {n: self.cmd[n] for n in goal}
        move = plan_joint_move(start, goal, {n: self.max_speed(n) for n in goal}, speed_scale=speed or self.speed_scale)
        for n in goal:
            j = self.joint_state(n)
            self.journal.write("move_joint", self.sim_time, phase="start", joint=n, target_deg=math.degrees(goal[n]), position_deg=math.degrees(self.measured(n)), velocity_dps=math.degrees(float(j.get("link_vel", 0.0) or 0.0)), torque_nm=j.get("tau_motor"), duration=round(move.duration, 4))
        t = 0.0
        while not move.done(t):
            t += self.dt
            self.cmd.update(move.at(t))
            self._update_cmd_tcp()
            yield
        yield from self._wait_ticks(settle)
        out = {}
        for n in goal:
            j = self.joint_state(n)
            err = goal[n] - self.measured(n)
            out[n] = {"target_deg": round(math.degrees(goal[n]), 3), "position_deg": round(math.degrees(self.measured(n)), 3), "error_deg": round(math.degrees(err), 3)}
            self.journal.write("move_joint", self.sim_time, phase="end", joint=n, target_deg=math.degrees(goal[n]), position_deg=math.degrees(self.measured(n)), error_deg=math.degrees(err), velocity_dps=math.degrees(float(j.get("link_vel", 0.0) or 0.0)), torque_nm=j.get("tau_motor"))
        return {"joints": out, "duration": round(move.duration + settle, 4)}

    def _update_cmd_tcp(self) -> None:
        self.cmd_tcp = self.kin.forward(self.cmd).tcp

    def _tcp_report(self, target: Iterable[float]) -> dict[str, Any]:
        target = tuple(target)
        out: dict[str, Any] = {"target_mm": [round(v * 1000, 2) for v in target]}
        fk_meas = self.kin.forward(self.measured_pose()).tcp if self.has_feedback else None
        if fk_meas is not None:
            out["fk_from_measured_mm"] = [round(v * 1000, 2) for v in fk_meas]
        true = self.state.get("tcp", {}).get("position")
        if true is not None:
            out["true_mm"] = [round(v * 1000, 2) for v in true]
            out["error_mm"] = round(math.dist(true, target) * 1000, 3)
        return out

    SAFE_TRANSIT_Z = 0.07      # m above the bench for transit moves
    CLEARANCE_Z = 0.008        # m: minimum height of tool/wrist/elbow along a path

    def _path_min_height(self, start: dict[str, float], goal: dict[str, float], samples: int = 24) -> float:
        """Lowest point of elbow, wrist and TCP along the joint-space path."""
        low = float("inf")
        for i in range(1, samples + 1):
            s = i / samples
            q = {**self.cmd, **{k: start[k] + (goal[k] - start[k]) * s for k in goal}}
            fk = self.kin.forward(q)
            pts = [fk.joint_points["elbow"], fk.joint_points["wrist"], fk.tcp]
            if i < samples:
                low = min(low, *(p[2] for p in pts))
            else:
                low = min(low, fk.joint_points["elbow"][2], fk.joint_points["wrist"][2])
        return low

    def _p_move_to(self, target: tuple[float, float, float], pitch: float, speed: float | None, settle: float) -> Program:
        goal = self.kin.inverse(target, pitch)
        start = {n: self.cmd[n] for n in ARM_JOINTS}
        transit: dict[str, Any] | None = None
        if self._path_min_height(start, goal) < self.CLEARANCE_Z:
            # The direct joint-space path would sweep the tool through the
            # bench: lift, cross at a safe height, then descend vertically.
            here = self.cmd_tcp or self.kin.forward(self.cmd).tcp
            z_safe = max(self.SAFE_TRANSIT_Z, here[2], target[2])
            transit = {"from_mm": [round(v * 1000, 1) for v in here], "safe_z_mm": round(z_safe * 1000, 1)}
            self.journal.write("path_replanned", self.sim_time, reason="joint path below bench clearance", **transit)
            if here[2] < z_safe - 1e-4:
                yield from self._p_move_line((here[0], here[1], z_safe), pitch, 0.04)
            yield from self._p_move_joints(self.kin.inverse((target[0], target[1], z_safe), pitch), speed, 0.05)
            if target[2] < z_safe - 1e-4:
                yield from self._p_move_line(target, pitch, 0.04)
            result: dict[str, Any] = {"joints": {}}
            yield from self._wait_ticks(settle)
        else:
            result = yield from self._p_move_joints(goal, speed, settle)
        self.cmd_tcp = tuple(target)  # type: ignore[assignment]
        result["tcp"] = self._tcp_report(target)
        if transit:
            result["transit"] = transit
        return result

    def _ik_or_fail(self, point: Iterable[float], pitch: float) -> dict[str, float]:
        try:
            return self.kin.inverse(tuple(point), pitch)
        except IKError as exc:
            raise ActionFailed(f"IK failed at {tuple(round(v, 4) for v in point)}: {exc}") from exc

    def _p_move_line(self, target: tuple[float, float, float], pitch: float, speed: float) -> Program:
        start = self.cmd_tcp or self.kin.forward(self.cmd).tcp
        line = plan_line(start, target, speed)
        self._ik_or_fail(target, pitch)
        t = 0.0
        while not line.done(t):
            t += self.dt
            pt = line.at(t)
            self.cmd.update(self._ik_or_fail(pt, pitch))
            self.cmd_tcp = pt
            yield
        self.cmd_tcp = target
        yield from self._wait_ticks(0.1)
        return {"tcp": self._tcp_report(target), "duration": round(line.duration, 4)}

    def _p_polyline(self, points: list[tuple[float, float, float]], pitch: float, speed: float) -> Generator[None, None, None]:
        path = plan_polyline(points, speed)
        for pt in points:
            self._ik_or_fail(pt, pitch)
        t = 0.0
        while not path.done(t):
            t += self.dt
            pt = path.at(t)
            self.cmd.update(self._ik_or_fail(pt, pitch))
            self.cmd_tcp = pt
            yield
        self.cmd_tcp = tuple(points[-1])  # type: ignore[assignment]

    def _p_gripper(self, angle: float, hold: float, label: str) -> Program:
        lo, hi = self.specs.gripper.soft_limits
        angle = min(max(angle, lo), hi)
        start = self.cmd["gripper"]
        move = plan_joint_move({"gripper": start}, {"gripper": angle}, {"gripper": self.max_speed("gripper")}, speed_scale=0.5)
        t = 0.0
        while not move.done(t):
            t += self.dt
            self.cmd["gripper"] = move.at(t)["gripper"]
            yield
        yield from self._wait_ticks(hold)
        g = self.state.get("gripper", {})
        held = [k for k, o in self.state.get("objects", {}).items() if o.get("held")]
        info = {"command": label, "target_opening_mm": round(self.specs.gripper.opening_for(angle) * 1000, 2), "measured_opening_mm": round(float(g.get("opening", float("nan"))) * 1000, 2) if "opening" in g else None, "finger_force_n": g.get("finger_force_each"), "stalled": g.get("stalled"), "held": held}
        self.journal.write("gripper", self.sim_time, **info)
        return info

    def _p_wait(self, seconds: float) -> Program:
        yield from self._wait_ticks(seconds)
        return {"waited_s": seconds}

    def _p_power(self, joint: str, on: bool) -> Program:
        self.power[joint] = bool(on)
        j = self.joint_state(joint)
        self.journal.write("servo_power", self.sim_time, joint=joint, on=bool(on), position_deg=math.degrees(self.measured(joint)), torque_nm=j.get("tau_motor"))
        if on:
            # Re-arm at the measured position so the servo does not jump.
            self.cmd[joint] = min(max(self.measured(joint), self.soft_limits(joint)[0]), self.soft_limits(joint)[1])
        yield
        return {"joint": joint, "on": bool(on)}

    def _p_set_pid(self, p: dict[str, Any]) -> Program:
        if not self.backend.capabilities.configurable_servo_controller:
            raise CapabilityNotSupported("servo controller gains are not configurable on this backend")
        params = {k: float(p[k]) for k in ("kp", "ki", "kd") if k in p}
        self.backend.servos[p["joint"]].configure(**params)
        self.journal.write("pid_config", self.sim_time, joint=p["joint"], **params)
        yield
        return {"joint": p["joint"], **params}

    def _p_step_response(self, joint: str, delta: float, duration: float) -> Program:
        if not self.has_feedback:
            raise CapabilityNotSupported("step response needs position feedback")
        start = self.cmd[joint]
        goal = start + delta
        self._check_limits({joint: goal})
        y0 = self.measured(joint)
        samples: list[tuple[float, float, float]] = []
        t0 = self.sim_time
        self.cmd[joint] = goal
        self._update_cmd_tcp()
        for _ in range(max(1, int(round(duration / self.dt)))):
            yield
            j = self.joint_state(joint)
            samples.append((self.sim_time - t0, float(j["link_pos"]), float(j["tau_motor"])))
        metrics = step_metrics(samples, y0, goal, self.specs.servo_for(joint).deadband_rad())
        metrics.update({"joint": joint, "step_deg": round(math.degrees(delta), 3), "gains": {k: self.joint_state(joint).get(k) for k in ("kp", "ki", "kd")}})
        self.pid_metrics[joint] = metrics
        self.journal.write("pid_metrics", self.sim_time, **metrics)
        return metrics

    # write / pick / place ----------------------------------------------------

    def _writing_cfg(self) -> dict[str, Any]:
        return self.specs.workspace["writing"]

    def _paper_top(self) -> float:
        paper = self.specs.workspace["paper"]
        return float(paper["center"][2]) + float(paper["size"][2]) / 2.0

    def _p_write(self, p: dict[str, Any]) -> Program:
        if self.tool != "pen":
            raise ActionFailed("write() needs the pen tool (reset the world with tool='pen')")
        cfg = self._writing_cfg()
        strokes = layout(
            p["text"],
            origin=p.get("origin", cfg["origin"]),
            letter_height=p.get("letter_height", cfg["letter_height"]),
            letter_width=p.get("letter_width", cfg["letter_width"]),
            spacing=p.get("spacing", cfg["letter_spacing"]),
            up_axis=cfg["up_axis"],
            advance_axis=cfg["advance_axis"],
        )
        speed = float(p.get("speed_mps", cfg["speed"]))
        z_paper = self._paper_top()
        z_down = z_paper - float(cfg["pen_press_depth"])
        z_up = z_paper + float(cfg["pen_up_height"])
        pitch = -math.pi / 2.0
        for stroke in strokes:              # validate the whole job before moving
            for (x, y) in stroke:
                self._ik_or_fail((x, y, z_down), pitch)
                self._ik_or_fail((x, y, z_up), pitch)
        ink_before = self._ink_count()
        self.journal.write("write_plan", self.sim_time, text=p["text"], strokes=len(strokes), speed_mps=speed, pen_press_depth=cfg["pen_press_depth"])
        for i, stroke in enumerate(strokes):
            x0, y0 = stroke[0]
            if i == 0:
                yield from self._p_move_to((x0, y0, z_up), pitch, None, 0.15)
            else:
                # Pen-plotter style transit: straight line at pen-up height.
                yield from self._p_move_line((x0, y0, z_up), pitch, 0.04)
                yield from self._wait_ticks(0.1)
            yield from self._p_move_line((x0, y0, z_down), pitch, 0.02)
            yield from self._p_polyline([(x, y, z_down) for (x, y) in stroke], pitch, speed)
            yield from self._wait_ticks(0.05)
            xe, ye = stroke[-1]
            yield from self._p_move_line((xe, ye, z_up), pitch, 0.03)
            self.journal.write("write_stroke", self.sim_time, index=i, points=len(stroke))
        result: dict[str, Any] = {"text": p["text"], "strokes": len(strokes), "ideal": [[[round(x, 5), round(y, 5)] for (x, y) in s] for s in strokes]}
        if self.backend.capabilities.ground_truth:
            ink = self.backend.ink()[ink_before:]
            result["ink"] = [[[round(a, 5), round(b, 5)] for (a, b) in s] for s in ink]
            result["analysis"] = ink_analysis(strokes, ink)
        return result

    def _ink_count(self) -> int:
        if not self.backend.capabilities.ground_truth:
            return 0
        return len(self.backend.ink())

    def _object_near(self, x: float, y: float) -> str | None:
        best, dist = None, 0.03
        for k, o in self.state.get("objects", {}).items():
            d = math.dist((x, y), o["position"][:2])
            if d < dist:
                best, dist = k, d
        return best

    def _p_pick(self, p: dict[str, Any]) -> Program:
        if self.tool != "gripper":
            raise ActionFailed("pick() needs the gripper tool")
        x, y = float(p["x"]), float(p["y"])
        z = float(p.get("z", 0.0125))
        approach = float(p.get("approach_height", 0.05))
        pitch = -math.pi / 2.0
        self._ik_or_fail((x, y, z), pitch)
        self._ik_or_fail((x, y, z + approach), pitch)
        label = p.get("label") or (self._object_near(x, y) if self.backend.capabilities.ground_truth else None)
        before = self.state.get("objects", {}).get(label, {}).get("position") if label else None
        yield from self._p_gripper(self.specs.gripper.soft_limits[1], 0.1, "open")
        yield from self._p_move_to((x, y, z + approach), pitch, None, 0.2)
        yield from self._p_move_line((x, y, z), pitch, 0.03)
        yield from self._wait_ticks(0.15)
        grip = yield from self._p_gripper(self.specs.gripper.soft_limits[0], 0.6, "close")
        yield from self._p_move_line((x, y, z + approach), pitch, 0.03)
        yield from self._wait_ticks(0.4)
        result: dict[str, Any] = {"grip": grip, "tcp": self._tcp_report((x, y, z + approach))}
        if label and before is not None:
            obj = self.state["objects"][label]
            lift = float(obj["position"][2]) - float(before[2])
            result.update({"object": label, "lift_mm": round(lift * 1000, 2), "held": bool(obj.get("held")), "lifted": lift > approach * 0.5})
            self.journal.write("pick_result", self.sim_time, object=label, lift_mm=round(lift * 1000, 2), held=bool(obj.get("held")), lifted=lift > approach * 0.5)
        return result

    def _p_place(self, p: dict[str, Any]) -> Program:
        x, y = float(p["x"]), float(p["y"])
        z = float(p.get("z", 0.0125))
        approach = float(p.get("approach_height", 0.05))
        pitch = -math.pi / 2.0
        self._ik_or_fail((x, y, z), pitch)
        held = [k for k, o in self.state.get("objects", {}).items() if o.get("held")]
        yield from self._p_move_to((x, y, z + approach), pitch, None, 0.2)
        yield from self._p_move_line((x, y, z + 0.004), pitch, 0.03)
        width = p.get("open_width_mm")
        release = self.specs.gripper.soft_limits[1] if width is None else self.specs.gripper.angle_for_opening(width / 1000.0)
        yield from self._p_gripper(release, 0.3, "open")
        yield from self._p_move_line((x, y, z + approach), pitch, 0.04)
        yield from self._wait_ticks(0.3)
        result: dict[str, Any] = {"target_mm": [round(x * 1000, 1), round(y * 1000, 1), round(z * 1000, 1)], "held_before": held}
        if held and self.backend.capabilities.ground_truth:
            obj = self.state["objects"][held[0]]
            result["object"] = held[0]
            result["placement_error_mm"] = round(math.dist(obj["position"][:2], (x, y)) * 1000, 2)
            result["object_tilt_deg"] = round(float(obj.get("tilt_deg", 0.0)), 2)
            self.journal.write("place_result", self.sim_time, object=held[0], placement_error_mm=result["placement_error_mm"])
        return result

    # -------------------------------------------------------------- servos

    def _command_servos(self) -> None:
        for name in ALL_SERVOS:
            lo, hi = self.soft_limits(name)
            angle = min(max(self.cmd[name], lo), hi)
            ch = self.backend.servos[name]
            ch.set_pulse_us(self.pulse_for(name, angle))
            powered = self.power[name] and name not in self.safety.powered_off_by_safety
            if self.safety.state == SafetyState.EMERGENCY_STOP and self.safety.estop_mode == "power_off":
                powered = False
            ch.set_power(powered)

    # ------------------------------------------------------------- safety

    def _static_torques(self) -> dict[str, float] | None:
        """Model-based holding torque at the measured pose (no payload: the
        controller does not know the mass of what it holds)."""
        try:
            return self.kin.gravity_torques(self.measured_pose())
        except (KeyError, ValueError):
            return None

    def _gripper_holding(self) -> bool:
        """Gripper closing against something (servo saturated/stalled while
        commanded to close). Observable on hardware with current sensing."""
        g = self.state.get("gripper", {})
        closing = self.cmd.get("gripper", 0.0) <= self.specs.gripper.closed + math.radians(5.0)
        return bool(closing and (g.get("stalled") or g.get("saturated")))

    def _lever_arms(self) -> dict[str, float] | None:
        try:
            fk = self.kin.forward(self.measured_pose())
        except (KeyError, ValueError):
            return None
        out = {}
        for n in ("shoulder", "elbow", "wrist"):
            p = fk.joint_points[n]
            out[n] = math.hypot(fk.tcp[0] - p[0], fk.tcp[1] - p[1])
        return out

    def _evaluate_safety(self) -> None:
        if not self.has_feedback:
            return
        before = self.safety.state
        self.safety.evaluate(
            self.state,
            self.sim_time,
            static_torques=self._static_torques(),
            expected_contacts={"paper"} if self.tool == "pen" else None,
            gripper_holding=self._gripper_holding(),
            lever_arms=self._lever_arms(),
        )
        after = self.safety.state
        if after != before and after in (SafetyState.STALL, SafetyState.FAULT):
            cancelled = self.queue.cancel_all(self.sim_time, f"safety: {self.safety.reason}")
            self._program = None
            # Hold the measured pose: stop pushing the trajectory into the obstacle/load.
            for n in ALL_SERVOS:
                self.cmd[n] = min(max(self.measured(n), self.soft_limits(n)[0]), self.soft_limits(n)[1])
            self._update_cmd_tcp()
            for rec in cancelled:
                self.journal.write("action_end", self.sim_time, action_id=rec.id, action=rec.action, status="cancelled", error=rec.error)

    def _journal_tick(self) -> None:
        for ev in self.safety.drain_events():
            name = ev.pop("event")
            self.journal.write(name, ev.pop("sim_time", self.sim_time), **ev)
        for c in self.state.get("collisions", []):
            self.journal.write("collision", c.get("sim_time", self.sim_time), phase="begin" if c.get("event") == "collision_begin" else "end", a=c.get("a"), b=c.get("b"))
        if self.ticks % self.journal.sample_every == 0 and "joints" in self.state and self.has_feedback:
            extra = {}
            tcp = self.state.get("tcp")
            if tcp:
                extra["tcp_mm"] = [round(v * 1000, 3) for v in tcp["position"]]
            if self.cmd_tcp is not None:
                extra["tcp_cmd_mm"] = [round(v * 1000, 3) for v in self.cmd_tcp]
            self.journal.write("joint_state", self.sim_time, tick=self.ticks, safety=self.safety.state.value, joints=joint_sample(self.state, ALL_SERVOS), **extra)

    # ---------------------------------------------------------- operator API

    def stop(self, reason: str = "stop()") -> None:
        """Controlled stop: cancel the queue and hold the commanded pose."""
        cancelled = self.queue.cancel_all(self.sim_time, reason)
        self._program = None
        self.journal.write("stop", self.sim_time, reason=reason, cancelled=[r.id for r in cancelled])

    def emergency_stop(self, reason: str = "emergency_stop()") -> None:
        cancelled = self.queue.cancel_all(self.sim_time, f"EMERGENCY_STOP: {reason}")
        self._program = None
        for n in ALL_SERVOS:
            self.cmd[n] = min(max(self.measured(n), self.soft_limits(n)[0]), self.soft_limits(n)[1])
        self._update_cmd_tcp()
        self.safety.emergency_stop(reason, self.sim_time)
        self.journal.write("emergency_stop", self.sim_time, reason=reason, mode=self.safety.estop_mode, cancelled=[r.id for r in cancelled])
        for ev in self.safety.drain_events():
            self.journal.write(ev.pop("event"), ev.pop("sim_time", self.sim_time), **ev)

    def reset_fault(self) -> bool:
        ok = self.safety.reset(self.sim_time)
        if ok:
            for n in ALL_SERVOS:
                self.power[n] = True
                self.cmd[n] = min(max(self.measured(n), self.soft_limits(n)[0]), self.soft_limits(n)[1])
            self._update_cmd_tcp()
        for ev in self.safety.drain_events():
            self.journal.write(ev.pop("event"), ev.pop("sim_time", self.sim_time), **ev)
        return ok

    def stall_check(self) -> dict[str, Any]:
        if not self.backend.capabilities.torque_feedback:
            return {"available": False, "reason": "backend has no torque/current feedback; stall cannot be observed"}
        out: dict[str, Any] = {"available": True, "sim_time": self.sim_time, "state": self.safety.state.value, "joints": {}}
        static = self._static_torques() or {}
        for n in ALL_SERVOS:
            j = self.joint_state(n)
            out["joints"][n] = {
                "stalled": bool(j.get("stalled")),
                "saturated": bool(j.get("saturated")),
                "requested_torque": j.get("tau_request"),
                "available_torque": j.get("tau_avail"),
                "maximum_torque": j.get("tau_max"),
                "load_ratio": None if not j.get("tau_max") else round(abs(float(j["tau_request"])) / float(j["tau_max"]), 4),
                "static_gravity_torque": static.get(n),
                "stall_time": j.get("stall_time"),
            }
        out["history"] = list(self.safety.stall_history)
        return out

    def status(self) -> dict[str, Any]:
        cur = self.queue.current
        return {
            "sim_time": round(self.sim_time, 4),
            "tick": self.ticks,
            "backend": self.backend.capabilities.name,
            "tool": self.tool,
            "state": self.safety.state.value,
            "reason": self.safety.reason,
            "warnings": list(self.safety.warnings),
            "active_action": cur.label() if cur else None,
            "queue": [r.label() for r in self.queue.pending],
            "joints_deg": {n: round(math.degrees(self.measured(n)), 3) for n in ALL_SERVOS},
            "targets_deg": {n: round(math.degrees(self.cmd[n]), 3) for n in ALL_SERVOS},
            "tcp_cmd_mm": [round(v * 1000, 2) for v in self.cmd_tcp] if self.cmd_tcp else None,
            "tcp_true_mm": [round(v * 1000, 2) for v in self.state["tcp"]["position"]] if "tcp" in self.state else None,
            "power": {n: self.power[n] and n not in self.safety.powered_off_by_safety for n in ALL_SERVOS},
        }

    def telemetry(self) -> dict[str, Any]:
        out: dict[str, Any] = {"sim_time": self.sim_time, "tick": self.ticks, "state": self.safety.state.value, "joints": {}}
        for n in ALL_SERVOS:
            j = self.joint_state(n)
            if not j or "link_pos" not in j:
                out["joints"][n] = {"target_deg": math.degrees(self.cmd[n]), "pulse_us": self.backend.servos[n].pulse_us, "powered": self.backend.servos[n].powered, "feedback": False}
                continue
            out["joints"][n] = {
                "servo": j.get("servo"),
                "pulse_us": j.get("pulse_us"),
                "target_deg": math.degrees(j["target"]),
                "position_deg": math.degrees(j["link_pos"]),
                "servo_shaft_deg": math.degrees(j["servo_pos"]),
                "error_deg": math.degrees(j["target"] - j["link_pos"]),
                "velocity_dps": math.degrees(j.get("link_vel", 0.0)),
                "torque_nm": j["tau_motor"],
                "requested_torque_nm": j["tau_request"],
                "max_torque_nm": j["tau_max"],
                "current_a": j["current"],
                "power_w": j.get("power_w"),
                "powered": j["powered"],
                "stalled": j["stalled"],
                "in_deadband": j["in_deadband"],
                "backlash_gap_deg": math.degrees(j.get("backlash_gap", 0.0)),
            }
        if "gripper" in self.state and "opening" in self.state["gripper"]:
            out["gripper"] = {"opening_mm": self.state["gripper"]["opening"] * 1000, "finger_force_n": self.state["gripper"]["finger_force_each"], "contacts": self.state["gripper"].get("finger_contacts")}
        if "tcp" in self.state:
            out["tcp_mm"] = [v * 1000 for v in self.state["tcp"]["position"]]
        if "objects" in self.state:
            out["objects"] = self.state["objects"]
        if "pen" in self.state:
            out["pen"] = {k: v for k, v in self.state["pen"].items() if k != "ink_new"}
        return out

    def camera_frame(self) -> CameraFrame:
        frame = self.backend.camera().frame()
        self.last_frame = frame
        meta = frame.to_dict()
        meta["frame_sim_time"] = meta.pop("sim_time")
        self.journal.write("camera_frame", self.sim_time, **meta)
        return frame

    def _push_hud(self) -> None:
        cur = self.queue.current
        status = {
            "state": self.safety.state.value,
            "reason": self.safety.reason,
            "active_action": f"▶ {cur.label()}" if cur else "idle",
            "queue": [r.label() for r in self.queue.pending][:6],
            "journal_tail": list(self.journal.tail)[-14:],
            "joint_states": self.safety.joint_states(self.state),
            "tcp_target_mm": "x=%.1f y=%.1f z=%.1f" % tuple(v * 1000 for v in self.cmd_tcp) if self.cmd_tcp else "—",
            "pid_metrics": {k: _metrics_line(v) for k, v in self.pid_metrics.items()},
        }
        if self.view_request:
            status["view"] = self.view_request
            self.view_request = None
        if self.trace_joint:
            status["trace_joint"] = self.trace_joint
        try:
            self.backend.hud(status)
        except BackendError:
            pass


def _metrics_line(m: dict[str, Any]) -> str:
    return "step %+.1f°: overshoot %.1f%%  settle %s  osc %d  ss_err %+.2f°" % (
        m.get("step_deg", 0.0), m.get("overshoot_pct", 0.0),
        "%.3fs" % m["settling_time_s"] if m.get("settling_time_s") is not None else "—",
        int(m.get("oscillations", 0)), m.get("steady_state_error_deg", 0.0),
    )


def step_metrics(samples: list[tuple[float, float, float]], y0: float, target: float, deadband: float) -> dict[str, Any]:
    """Step-response metrics of a joint (classic definitions).

    ``samples`` = (t, position, torque). The final value y_f is the mean of the
    last 15% of the window. Overshoot = (peak - y_f) / (y_f - y0). Settling
    time = first instant after which the response stays within a band of
    max(5% of the step, servo dead band) around y_f. Steady-state error is
    reported separately against the commanded target (hobby servos are
    P/PD controllers: a loaded joint keeps an error).
    """
    step = target - y0
    if not samples or abs(step) < 1e-9:
        return {"overshoot_pct": 0.0, "settling_time_s": None, "oscillations": 0}
    tail = samples[int(len(samples) * 0.85):] or samples[-1:]
    y_f = sum(y for _, y, _ in tail) / len(tail)
    span = y_f - y0
    if abs(span) < 1e-9:
        span = step
    norm = [(t, (y - y0) / span) for (t, y, _) in samples]
    peak = max(v for _, v in norm)
    overshoot = max(0.0, (peak - 1.0) * 100.0)
    rise = next((t for t, v in norm if v >= 0.9), None)
    band = max(0.05, abs(deadband / span))
    settle = None
    for i in range(len(norm)):
        if all(abs(v - 1.0) <= band for _, v in norm[i:]):
            settle = norm[i][0]
            break
    crossings = 0
    prev = None
    for _, v in norm:
        if abs(v - 1.0) <= band * 0.5:
            continue
        s = 1 if v > 1.0 else -1
        if prev is not None and s != prev:
            crossings += 1
        prev = s
    return {
        "overshoot_pct": round(overshoot, 3),
        "peak_normalised": round(peak, 4),
        "rise_time_s": None if rise is None else round(rise, 4),
        "settling_time_s": None if settle is None else round(settle, 4),
        "settling_band_pct": round(band * 100, 2),
        "oscillations": crossings // 2,
        "final_value_deg": round(math.degrees(y_f), 4),
        "steady_state_error_deg": round(math.degrees(target - y_f), 4),
        "peak_torque_nm": round(max(abs(tq) for *_, tq in samples), 5),
        "samples": [[round(t, 4), round(math.degrees(y), 4)] for (t, y, _) in samples],
    }


def _seg_dist(p: tuple[float, float], a: tuple[float, float], b: tuple[float, float]) -> float:
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    L2 = dx * dx + dy * dy
    if L2 == 0:
        return math.dist(p, a)
    t = max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / L2))
    return math.dist(p, (ax + t * dx, ay + t * dy))


def ink_analysis(ideal: list[list[tuple[float, float]]], ink: list[list[list[float]]]) -> dict[str, Any]:
    """Compare drawn ink with the ideal strokes (ground truth, simulation)."""
    segs = [(s[i], s[i + 1]) for s in ideal for i in range(len(s) - 1)]
    pts = [tuple(p) for stroke in ink for p in stroke]
    if not pts or not segs:
        return {"ink_points": len(pts), "coverage_1mm": 0.0}
    dev = [min(_seg_dist(p, a, b) for a, b in segs) for p in pts]   # type: ignore[arg-type]
    # Coverage: ideal samples every 0.5 mm that have ink within 1 mm.
    samples = []
    for a, b in segs:
        n = max(1, int(math.dist(a, b) / 0.0005))
        samples += [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n) for i in range(n)]
    ink_segs = [(tuple(st[i]), tuple(st[i + 1])) for st in ink for i in range(len(st) - 1)] or [(tuple(st[0]), tuple(st[0])) for st in ink if st]
    covered = 0
    for s in samples:
        if any(_seg_dist(s, a, b) <= 0.001 for a, b in ink_segs):  # type: ignore[arg-type]
            covered += 1
    return {
        "ink_strokes": len(ink),
        "ideal_strokes": len(ideal),
        "ink_points": len(pts),
        "deviation_mean_mm": round(sum(dev) / len(dev) * 1000, 3),
        "deviation_rms_mm": round(math.sqrt(sum(d * d for d in dev) / len(dev)) * 1000, 3),
        "deviation_max_mm": round(max(dev) * 1000, 3),
        "coverage_1mm": round(covered / len(samples), 4),
    }
