"""Robotics unit tests: no Godot, no hardware."""

from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any

import pytest

from abiyss.audit import AuditLog
from abiyss.errors import ValidationError
from abiyss.models import QueryType
from abiyss.robotics.action_queue import ActionQueue, ActionStatus
from abiyss.robotics.actions import ACTION_SCHEMAS, validate_action
from abiyss.robotics.backend import BackendCapabilities, CameraFrame, CameraSensor, CapabilityNotSupported, RobotBackend, ServoChannel
from abiyss.robotics.controller import RobotController, ink_analysis, step_metrics
from abiyss.robotics.hardware import HardwareBackend, RecordingPWMDriver
from abiyss.robotics.kinematics import ARM_JOINTS, ArmKinematics, IKError, home_pose
from abiyss.robotics.queue_dsl import load_queue, parse_line
from abiyss.robotics.report import ink_plot, line_plot, scatter_plot
from abiyss.robotics.runtime_tools import register_robot_tools, robot_tool_specs
from abiyss.robotics.safety import SafetyState, SafetySupervisor
from abiyss.robotics.specs import VALID_STATUSES, audit_parameters, load_specs
from abiyss.robotics.telemetry import TelemetryJournal, read_journal
from abiyss.robotics.text import GLYPHS, layout
from abiyss.robotics.trajectory import min_jerk, plan_joint_move, plan_polyline
from abiyss.tools import AST, ToolRegistry

SPECS = load_specs()


# --------------------------------------------------------------------- specs

def test_every_parameter_has_a_verification_status() -> None:
    rows = audit_parameters(SPECS.raw)
    assert len(rows) > 50
    for r in rows:
        assert r["status"] in VALID_STATUSES, r
    sourced = {"datasheet", "manufacturer_web", "datasheet_other_manufacturer", "third_party", "reference"}
    for r in rows:
        if r["status"] in sourced:
            assert r["sources"], f"{r['path']} has status {r['status']} but no sources"
        for s in r["sources"]:
            assert s in SPECS.raw["sources"], f"unknown source id {s} in {r['path']}"


def test_unverified_values_are_explicitly_marked() -> None:
    p = SPECS.raw["servo_models"]["MG90S"]["parameters"]
    for key in ("backlash", "gear_stiffness", "reflected_inertia", "coulomb_friction", "pulse_jitter_sigma", "potentiometer_noise_sigma"):
        assert p[key]["status"] == "estimated", key
    assert SPECS.raw["servo_models"]["SG90"]["parameters"]["stall_current_4v8"]["status"] == "unknown"


def test_datasheet_conversions() -> None:
    mg = SPECS.servos["MG90S"]
    assert mg.stall_torque == pytest.approx(1.8 * 0.0980665, rel=1e-4)       # 1.8 kgf*cm
    assert mg.no_load_speed == pytest.approx(math.radians(60) / 0.10, rel=1e-3)
    assert mg.deadband_us == 5.0 and SPECS.servos["SG90"].deadband_us == 10.0
    assert mg.deadband_rad() == pytest.approx(math.radians(0.9))
    assert SPECS.control_rate_hz == pytest.approx(1.0 / mg.pwm_period)


# ---------------------------------------------------------------- kinematics

@pytest.mark.parametrize("tool", ["gripper", "pen"])
def test_ik_fk_roundtrip(tool: str) -> None:
    k = ArmKinematics(SPECS, tool)
    for x in (0.10, 0.13, 0.16, 0.19):
        for y in (-0.08, 0.0, 0.08):
            for z in (0.0, 0.03, 0.06):
                try:
                    q = k.inverse((x, y, z), -math.pi / 2)
                except IKError:
                    continue
                fk = k.forward(q)
                assert math.dist(fk.tcp, (x, y, z)) < 1e-9
                assert fk.pitch == pytest.approx(-math.pi / 2, abs=1e-9)
                assert not k.limit_violations(q)


def test_ik_rejects_unreachable_and_reports_why() -> None:
    k = ArmKinematics(SPECS, "gripper")
    with pytest.raises(IKError, match="out of reach"):
        k.inverse((0.30, 0.0, 0.0))
    with pytest.raises(IKError):
        k.inverse((0.0, 0.0, 0.05))


def test_gravity_model_signs_and_magnitude() -> None:
    k = ArmKinematics(SPECS, "gripper")
    extended = {"base_yaw": 0.0, "shoulder": 0.0, "elbow": 0.0, "wrist": 0.0}
    tau = k.gravity_torques(extended)
    assert tau["shoulder"] > tau["elbow"] > tau["wrist"] > 0      # positive torque raises the arm
    assert abs(tau["base_yaw"]) < 1e-12
    assert 0.05 < tau["shoulder"] < SPECS.servos["MG90S"].stall_torque     # the arm alone can be held
    heavy = k.gravity_torques(extended, payload_kg=0.123)
    assert heavy["shoulder"] > SPECS.servos["MG90S"].stall_torque       # 123 g at full reach cannot


# -------------------------------------------------------------- trajectory

def test_min_jerk_profile() -> None:
    assert min_jerk(0.0) == 0.0 and min_jerk(1.0) == 1.0
    assert min_jerk(0.5) == pytest.approx(0.5)


def test_joint_move_respects_servo_speed() -> None:
    start = home_pose(SPECS)
    goal = {**start, "shoulder": start["shoulder"] - 1.2}
    vmax = {j: SPECS.servo_for(j).no_load_speed for j in ARM_JOINTS}
    mv = plan_joint_move(start, goal, vmax, speed_scale=0.3)
    dt = 1e-3
    peak = max(abs(mv.at(t + dt)["shoulder"] - mv.at(t)["shoulder"]) / dt for t in [i * dt for i in range(int(mv.duration / dt))])
    assert peak <= vmax["shoulder"] * 0.3 * 1.001


def test_polyline_cruise_speed() -> None:
    pts = [(0.0, 0.0, 0.0), (0.1, 0.0, 0.0)]
    path = plan_polyline(pts, 0.02)
    v = (path.at(path.duration / 2 + 0.01)[0] - path.at(path.duration / 2)[0]) / 0.01
    assert v == pytest.approx(0.02, rel=0.01)


# -------------------------------------------------------------------- text

def test_layout_oi() -> None:
    strokes = layout("OI", origin=(0.15, 0.03), letter_height=0.03, letter_width=0.02, spacing=0.01, up_axis=(1, 0), advance_axis=(0, -1))
    assert len(strokes) == 2
    o, i = strokes
    assert math.dist(o[0], o[-1]) < 1e-9                       # O is closed
    assert i[0][0] == pytest.approx(0.18) and i[-1][0] == pytest.approx(0.15)   # I from cap to baseline
    with pytest.raises(ValidationError):
        layout("O#", origin=(0, 0), letter_height=0.03, letter_width=0.02, spacing=0.01)
    assert {"O", "I"} <= set(GLYPHS)


# ------------------------------------------------------------- actions/queue

def test_action_validation() -> None:
    assert validate_action({"action": "write", "text": "OI"})["text"] == "OI"
    with pytest.raises(ValidationError):
        validate_action({"action": "teleport", "x": 0})
    with pytest.raises(ValidationError):
        validate_action({"action": "move_to", "x": 0.1, "y": 0.0})            # missing z
    with pytest.raises(ValidationError):
        validate_action({"action": "move_to", "x": 0.1, "y": 0.0, "z": 0.0, "set_position": True})
    with pytest.raises(ValidationError):
        validate_action({"action": "move_to", "x": 9.0, "y": 0.0, "z": 0.0})
    assert set(ACTION_SCHEMAS) >= {"move_to", "move_joint", "home", "gripper_open", "gripper_close", "write", "pick", "place"}


def test_queue_lifecycle() -> None:
    q = ActionQueue()
    a = q.submit({"action": "home"})
    b = q.submit({"action": "wait", "seconds": 1})
    assert q.pop_next(0.0) is a and a.status == ActionStatus.RUNNING
    assert q.pop_next(0.0) is None                      # one at a time
    q.finish(ActionStatus.DONE, 1.0, result={"ok": 1})
    assert q.pop_next(1.0) is b
    cancelled = q.cancel_all(2.0, "estop")
    assert [r.id for r in cancelled] == [b.id] and b.status == ActionStatus.CANCELLED
    assert q.idle()


def test_queue_text_dsl_is_data_only() -> None:
    acts = load_queue('queue:\n  write("OI")\n  home()\n')
    assert acts == [{"action": "write", "text": "OI"}, {"action": "home"}]
    assert parse_line("gripper.close(width_mm=20)") == {"action": "gripper_close", "width_mm": 20}
    with pytest.raises(ValidationError):
        parse_line("write(__import__('os').getcwd())")
    with pytest.raises(ValidationError):
        parse_line("os.system('true')")
    assert load_queue('{"queue": [{"action": "home"}]}') == [{"action": "home"}]


# ------------------------------------------------------------------ safety

def _joint(**kw: Any) -> dict[str, Any]:
    base = {"servo": "MG90S", "target": 0.0, "link_pos": 0.0, "link_vel": 0.0, "servo_pos": 0.0, "tau_request": 0.0, "tau_motor": 0.0,
            "tau_avail": 0.17652, "tau_max": 0.17652, "current": 0.006, "powered": True, "stalled": False, "saturated": False, "in_deadband": True, "stall_time": 0.0}
    base.update(kw)
    return base


def _state(**joints: dict[str, Any]) -> dict[str, Any]:
    js = {n: _joint() for n in ARM_JOINTS}
    js.update(joints)
    return {"joints": js, "gripper": _joint(servo="SG90"), "collisions": []}


def test_stall_event_is_auditable_and_latched() -> None:
    s = SafetySupervisor(SPECS)
    stalled = _joint(stalled=True, saturated=True, tau_request=0.45, tau_motor=0.1765, stall_time=0.3, link_pos=0.4, target=0.6, current=0.75)
    s.evaluate(_state(shoulder=stalled), 10.0, static_torques={"shoulder": 0.08}, gripper_holding=True, lever_arms={"shoulder": 0.17})
    ev = [e for e in s.drain_events() if e["event"] == "servo_stall"]
    assert s.state == SafetyState.STALL and len(ev) == 1
    e = ev[0]
    assert e["joint"] == "shoulder" and e["requested_torque"] == 0.45 and e["maximum_torque"] == pytest.approx(0.17652)
    assert e["position_deg"] == pytest.approx(math.degrees(0.4), abs=1e-3) and e["time"] == 10.0
    assert "payload" in e["probable_cause"] and e["payload_lower_bound_kg"] == pytest.approx((0.17652 - 0.08) / (SPECS.gravity * 0.17), abs=1e-4)
    assert not s.accepts_commands()
    assert not s.reset(10.1)                        # still stalled -> refused
    s.evaluate(_state(), 10.2)                      # stall gone
    assert s.state == SafetyState.STALL             # latched
    assert s.reset(10.3) and s.state == SafetyState.SAFE


def test_payload_bound_only_when_lifting_against_gravity() -> None:
    s = SafetySupervisor(SPECS)
    pushing_down = _joint(stalled=True, saturated=True, tau_request=-1.0, stall_time=0.3)
    s.evaluate(_state(shoulder=pushing_down), 1.0, static_torques={"shoulder": 0.067}, lever_arms={"shoulder": 0.15})
    ev = [e for e in s.drain_events() if e["event"] == "servo_stall"][0]
    assert ev["payload_lower_bound_kg"] is None and "obstruction" in ev["probable_cause"]


def test_persistent_stall_escalates_to_fault_and_powers_off() -> None:
    s = SafetySupervisor(SPECS)
    st = _state(elbow=_joint(stalled=True, saturated=True, tau_request=0.3, stall_time=0.3))
    for i in range(0, 130):
        s.evaluate(st, 1.0 + i * 0.02)
    assert s.state == SafetyState.FAULT and "elbow" in s.powered_off_by_safety
    assert any(e["event"] == "servo_power_off" for e in s.drain_events())


def test_gripper_stall_is_a_grip_not_a_fault() -> None:
    s = SafetySupervisor(SPECS)
    st = _state()
    st["gripper"] = _joint(servo="SG90", stalled=True, saturated=True, tau_request=-0.5)
    s.evaluate(st, 1.0)
    assert s.state == SafetyState.SAFE


def test_invalid_physics_is_a_fault() -> None:
    s = SafetySupervisor(SPECS)
    s.evaluate(_state(wrist=_joint(link_pos=float("nan"))), 1.0)
    assert s.state == SafetyState.FAULT


def test_warning_hysteresis_and_estop() -> None:
    s = SafetySupervisor(SPECS)
    hot = _state(shoulder=_joint(tau_request=0.16, in_deadband=False))
    s.evaluate(hot, 0.00)
    assert s.state == SafetyState.SAFE              # one period is not enough
    s.evaluate(hot, 0.12)
    assert s.state == SafetyState.WARNING
    s.evaluate(_state(), 0.14)
    assert s.state == SafetyState.WARNING           # must stay clear for a while
    s.evaluate(_state(), 0.50)
    assert s.state == SafetyState.SAFE
    s.emergency_stop("test", 1.0)
    assert s.state == SafetyState.EMERGENCY_STOP and not s.accepts_commands()
    assert s.reset(2.0) and s.state == SafetyState.SAFE


# --------------------------------------------------------------- telemetry

def test_journal_is_auditlog_compatible(tmp_path: Path) -> None:
    audit = AuditLog(tmp_path / "audit.jsonl")
    j = TelemetryJournal(tmp_path / "robot.jsonl", mirror=audit)
    j.write("servo_stall", 1.25, joint="shoulder", requested_torque=0.4, maximum_torque=0.17652, api_token="secret", nan=float("nan"))
    j.write("joint_state", 1.30, joints={})
    j.close()
    lines = (tmp_path / "robot.jsonl").read_text().splitlines()
    rec = json.loads(lines[0])
    assert list(rec) == sorted(rec)                                 # sorted keys like AuditLog
    assert lines[0] == json.dumps(rec, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    assert rec["event"] == "servo_stall" and isinstance(rec["ts"], float) and rec["sim_time"] == 1.25
    assert rec["api_token"] == "<redacted>" and rec["nan"] is None
    mirrored = read_journal(tmp_path / "audit.jsonl")
    assert [m["event"] for m in mirrored] == ["robotics.servo_stall"]      # periodic samples are not mirrored


# ---------------------------------------------------------- fake backend

class FakeServo(ServoChannel):
    def configure(self, **params: Any) -> None:
        self.params = params


class FakeCamera(CameraSensor):
    def frame(self) -> CameraFrame:
        return CameraFrame(1, 0.0, 2, 2, "png", b"\x89PNG\r\n\x1a\nfake", {"position": [0, 0, 0]}, {"fov_y_deg": 70}, "FakeCamera")


class FakeBackend(RobotBackend):
    """Kinematic stand-in: each joint follows its pulse with a first-order lag."""

    def __init__(self) -> None:
        self.capabilities = BackendCapabilities("simulation", True, True, True, True, True, True, False)
        self._servos = {n: FakeServo(n) for n in [*ARM_JOINTS, "gripper"]}
        self.t = 0.0
        self.q = {n: 0.0 for n in self._servos}
        self.ctl: RobotController | None = None

    def connect(self) -> dict[str, Any]:
        return {"fake": True}

    def close(self) -> None:
        pass

    @property
    def servos(self) -> dict[str, ServoChannel]:
        return self._servos

    def reset(self, *, tool: str, seed: int) -> dict[str, Any]:
        self.t = 0.0
        self.q = home_pose(SPECS)
        self.q["gripper"] = SPECS.gripper.home
        return self._state()

    def _angle(self, name: str) -> float:
        assert self.ctl is not None
        servo = SPECS.servo_for(name)
        center, direction = (SPECS.gripper.center, 1.0) if name == "gripper" else (SPECS.joint(name).center, SPECS.joint(name).direction)
        return center + direction * (self._servos[name].pulse_us - servo.pulse_neutral_us) / servo.pulse_per_90_us * (math.pi / 2)

    def advance(self, dt: float) -> dict[str, Any]:
        for n, ch in self._servos.items():
            if ch.powered:
                self.q[n] += (self._angle(n) - self.q[n]) * min(1.0, dt / 0.05)
        self.t += dt
        return self._state()

    def _state(self) -> dict[str, Any]:
        js = {}
        for n in [*ARM_JOINTS, "gripper"]:
            ch = self._servos[n]
            target = self._angle(n) if self.ctl else self.q[n]
            js[n] = _joint(servo=SPECS.servo_for(n).model, target=target, link_pos=self.q[n], servo_pos=self.q[n], pulse_us=ch.pulse_us, powered=ch.powered, kp=1.0, ki=0.0, kd=0.0)
        g = js.pop("gripper")
        return {"sim_time": self.t, "joints": js, "gripper": g, "collisions": []}

    def camera(self) -> CameraSensor:
        return FakeCamera()


def _controller(tmp_path: Path) -> RobotController:
    be = FakeBackend()
    ctl = RobotController(SPECS, be, journal=TelemetryJournal(tmp_path / "j.jsonl"))
    be.ctl = ctl
    ctl.start()
    return ctl


def test_controller_runs_queue_through_backend(tmp_path: Path) -> None:
    ctl = _controller(tmp_path)
    rec = ctl.submit({"action": "move_to", "x": 0.15, "y": 0.02, "z": 0.05})
    assert ctl.run_until_idle(30)
    assert rec.status == ActionStatus.DONE
    q = ctl.kin.inverse((0.15, 0.02, 0.05))
    for j in ARM_JOINTS:
        assert ctl.measured(j) == pytest.approx(q[j], abs=math.radians(1.0))
    names = [r["event"] for r in ctl.journal.records]
    for ev in ("run_start", "command", "action_start", "move_joint", "joint_state", "action_end"):
        assert ev in names
    ctl.close()


def test_pulses_are_quantised_and_limited(tmp_path: Path) -> None:
    ctl = _controller(tmp_path)
    assert ctl.pulse_for("shoulder", math.radians(90.0)) == 1500.0
    assert ctl.pulse_for("shoulder", math.radians(90.0) + 1e-4) % SPECS.pwm_resolution_us == 0
    with pytest.raises(ValidationError):
        validate_action({"action": "move_joint", "joint": "elbow", "angle_deg": 500})
    rec = ctl.submit({"action": "move_joint", "joint": "elbow", "angle_deg": 60})   # beyond the soft limit (25 deg)
    ctl.run_until_idle(10)
    assert rec.status == ActionStatus.FAILED and "soft limits" in (rec.error or "")
    ctl.close()


def test_stop_estop_and_reset(tmp_path: Path) -> None:
    ctl = _controller(tmp_path)
    ctl.submit({"action": "wait", "seconds": 5})
    ctl.submit({"action": "home"})
    for _ in range(5):
        ctl.tick()
    ctl.stop()
    assert ctl.queue.idle()
    ctl.submit({"action": "wait", "seconds": 5})
    ctl.tick()
    ctl.emergency_stop("unit test")
    assert ctl.status()["state"] == "EMERGENCY_STOP" and ctl.queue.idle()
    with pytest.raises(ValidationError):
        ctl.submit({"action": "home"})
    assert ctl.reset_fault() and ctl.status()["state"] == "SAFE"
    ctl.submit({"action": "home"})
    ctl.close()


def test_controller_is_deterministic(tmp_path: Path) -> None:
    def run(sub: str) -> list[float]:
        ctl = _controller(tmp_path / sub)
        ctl.submit({"action": "move_to", "x": 0.14, "y": -0.03, "z": 0.04})
        ctl.submit({"action": "home"})
        ctl.run_until_idle(30)
        out = [ctl.backend.servos[j].pulse_us for j in ARM_JOINTS] + [ctl.sim_time]
        ctl.close()
        return out
    assert run("a") == run("b")


def test_camera_frame_is_journaled(tmp_path: Path) -> None:
    ctl = _controller(tmp_path)
    f = ctl.camera_frame()
    assert f.data.startswith(b"\x89PNG") and f.to_dict()["bytes"] == len(f.data)
    assert ctl.journal.events("camera_frame")
    ctl.close()


def test_hardware_backend_path_without_hardware(tmp_path: Path) -> None:
    drv = RecordingPWMDriver()
    be = HardwareBackend(SPECS, drv, {"base_yaw": 0, "shoulder": 1, "elbow": 2, "wrist": 3, "gripper": 4}, realtime=False)
    ctl = RobotController(SPECS, be, journal=TelemetryJournal(tmp_path / "hw.jsonl", backend="hardware"))
    ctl.start()
    ctl.submit({"action": "move_joint", "joint": "shoulder", "angle_deg": 100})
    assert ctl.run_until_idle(10)
    shoulder = [p for _, ch, p in drv.log if ch == 1 and p is not None]
    assert shoulder and shoulder[-1] == ctl.pulse_for("shoulder", math.radians(100))
    assert ctl.stall_check()["available"] is False                        # no feedback on a real SG90/MG90S
    with pytest.raises(CapabilityNotSupported):
        be.servos["shoulder"].configure(kp=1.0)
    with pytest.raises(CapabilityNotSupported):
        be.camera().frame()
    ctl.close()
    assert drv.log[-1][2] is None                                          # pulses disabled on close


# ------------------------------------------------------------- metrics/plots

def test_step_metrics_on_known_response() -> None:
    wn, zeta = 30.0, 0.3
    wd = wn * math.sqrt(1 - zeta ** 2)
    samples = []
    for i in range(1, 300):
        t = i * 0.005
        y = 1 - math.exp(-zeta * wn * t) * (math.cos(wd * t) + zeta / math.sqrt(1 - zeta ** 2) * math.sin(wd * t))
        samples.append((t, y, 0.0))
    m = step_metrics(samples, 0.0, 1.0, deadband=0.0)
    expected = 100 * math.exp(-zeta * math.pi / math.sqrt(1 - zeta ** 2))
    assert m["overshoot_pct"] == pytest.approx(expected, rel=0.03)
    assert m["settling_time_s"] == pytest.approx(4 / (zeta * wn), rel=0.35)
    assert m["oscillations"] >= 1


def test_ink_analysis_and_plots() -> None:
    ideal = [[(0.0, 0.0), (0.01, 0.0)]]
    ink = [[[0.0, 0.0005], [0.005, 0.0005], [0.01, 0.0005]]]
    a = ink_analysis(ideal, ink)
    assert a["deviation_mean_mm"] == pytest.approx(0.5) and a["coverage_1mm"] > 0.9
    for svg in (ink_plot(ideal, ink, "t"), line_plot([("a", [(0, 0), (1, 1)])], "t", "x", "y"), scatter_plot([(0, 0), (1, 1)], "t", "x", "y")):
        assert svg.startswith("<svg") and svg.endswith("</svg>")


# ---------------------------------------------------------- runtime tools

class FakeLink:
    def __init__(self) -> None:
        self.queued: list[dict[str, Any]] = []

    def status(self) -> Any:
        return {"state": "SAFE"}

    def telemetry(self) -> Any:
        return {"joints": {}}

    def stall_check(self) -> Any:
        return {"available": True}

    def enqueue(self, actions: list[dict[str, Any]], source: str = "x") -> Any:
        self.queued.extend(actions)
        return [{"id": i + 1} for i in range(len(actions))]

    def emergency_stop(self, reason: str = "x") -> Any:
        return {"state": "EMERGENCY_STOP"}

    def reset(self) -> Any:
        return {"reset": True}

    def frame(self, path: str | None = None) -> Any:
        return {"width": 320}


def test_runtime_tools_validate_intent() -> None:
    reg = ToolRegistry()
    link = FakeLink()
    names = register_robot_tools(reg, link)
    assert "robot.enqueue" in names and all(s.query_type == QueryType.AQUERY for s in robot_tool_specs())
    ast_ = AST(reg)
    res = ast_.execute("robot.enqueue", {"actions": [{"action": "write", "text": "OI"}]})
    assert res.status == "ok" and link.queued == [{"action": "write", "text": "OI"}]
    with pytest.raises(ValidationError):
        ast_.execute("robot.enqueue", {"actions": [{"action": "set_transform", "x": 1}]})
    assert ast_.execute("robot.status", {}).output == {"state": "SAFE"}


def test_workspace_fixtures_are_reachable() -> None:
    """Every object, drop slot and the writing area must be reachable top-down
    within the soft limits (catches fixture layouts that the arm cannot serve)."""
    grip = ArmKinematics(SPECS, "gripper")
    for o in SPECS.workspace["objects"]:
        x, y, z = o["position"]
        grip.inverse((x, y, z))
        grip.inverse((x, y, z + 0.05))
    for x, y, _ in SPECS.workspace["drop_zone"]["slots"]:
        grip.inverse((x, y, 0.0165))
        grip.inverse((x, y, 0.0625))
    pen = ArmKinematics(SPECS, "pen")
    cfg = SPECS.workspace["writing"]
    for stroke in layout("OI", origin=cfg["origin"], letter_height=cfg["letter_height"], letter_width=cfg["letter_width"], spacing=cfg["letter_spacing"], up_axis=cfg["up_axis"], advance_axis=cfg["advance_axis"]):
        for x, y in stroke:
            pen.inverse((x, y, -cfg["pen_press_depth"]))
            pen.inverse((x, y, cfg["pen_up_height"]))
