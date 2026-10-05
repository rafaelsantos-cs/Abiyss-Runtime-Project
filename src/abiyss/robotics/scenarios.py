"""Reproducible validation scenarios (TEST 1-5 and supporting experiments).

Every scenario:
* starts from ``reset_world`` with a fixed seed (deterministic lockstep),
* drives the arm only through the Robot API (actions/queue),
* reads results from telemetry and, in simulation, from physics ground truth,
* returns a JSON-serialisable report and optional SVG plots,
* can capture screenshots at meaningful moments when rendering is available.

Pass/fail checks verify that the *mechanism* works (a stall is detected and
reported with torques, the arm falls when unpowered, ...). They do not demand
good-looking results: imperfection is the point of the lab.
"""

from __future__ import annotations

import json
import math
import statistics
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from .api import Robot
from .controller import ALL_SERVOS
from .kinematics import ARM_JOINTS, ArmKinematics, home_pose
from .report import ink_plot, line_plot, scatter_plot
from .specs import ArmSpecs, load_specs, value


@dataclass
class ScenarioResult:
    name: str
    title: str
    passed: bool
    checks: list[dict[str, Any]] = field(default_factory=list)
    metrics: dict[str, Any] = field(default_factory=dict)
    artifacts: dict[str, str] = field(default_factory=dict)
    events: list[dict[str, Any]] = field(default_factory=list)
    wall_s: float = 0.0
    sim_s: float = 0.0

    def check(self, name: str, ok: bool, detail: Any = None) -> bool:
        self.checks.append({"check": name, "ok": bool(ok), "detail": detail})
        return bool(ok)

    def finalize(self) -> "ScenarioResult":
        self.passed = all(c["ok"] for c in self.checks)
        return self

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "title": self.title,
            "passed": self.passed,
            "checks": self.checks,
            "metrics": self.metrics,
            "artifacts": self.artifacts,
            "events": self.events,
            "wall_s": round(self.wall_s, 2),
            "sim_s": round(self.sim_s, 3),
        }


class Lab:
    """Shared context: one simulator process reused by several scenarios."""

    def __init__(self, robot: Robot, out_dir: Path, *, screenshots: bool = False) -> None:
        self.robot = robot
        self.ctl = robot.controller
        self.out = out_dir
        self.out.mkdir(parents=True, exist_ok=True)
        self.screenshots = screenshots and self.ctl.backend.capabilities.camera
        self.shots: dict[str, str] = {}

    @property
    def specs(self) -> ArmSpecs:
        return self.ctl.specs

    def reset(self, tool: str, seed: int | None = None) -> None:
        self.ctl.reset_world(tool, self.specs.default_seed if seed is None else seed)
        self.ctl.ticks = 0

    def shot(self, key: str, view: str | None = None, trace: str | None = None) -> str | None:
        """Capture a screenshot of the lab window (only when rendering)."""
        if not self.screenshots:
            return None
        if view:
            self.ctl.view_request = view
        if trace:
            self.ctl.trace_joint = trace
        self.ctl._push_hud()
        path = self.out / "screenshots" / f"{key}.png"
        self.ctl.backend.screenshot(path)
        self.shots[key] = str(path)
        return str(path)

    def write(self, name: str, text: str) -> str:
        p = self.out / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
        return str(p)

    def run_actions(self, actions: list[dict[str, Any]], timeout: float = 120.0, on_tick: Callable[[], None] | None = None) -> list[Any]:
        recs = self.robot.queue(actions, source="scenario")
        end = self.ctl.sim_time + timeout
        while self.ctl.sim_time < end:
            self.ctl.tick()
            if on_tick:
                on_tick()
            if self.ctl.queue.idle() and self.ctl._program is None:
                break
        return recs

    def events(self, *names: str, since_seq: int = 0) -> list[dict[str, Any]]:
        return [
            {k: v for k, v in r.items() if k not in ("ts", "source", "run_id", "backend")}
            for r in self.ctl.journal.records
            if r["event"] in names and r["seq"] > since_seq
        ]

    @property
    def seq(self) -> int:
        return self.ctl.journal.seq


# ---------------------------------------------------------------- TEST 1


def scenario_write_oi(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("test1_write_oi", "TEST 1 - write('OI') on paper", False)
    lab.reset("pen")
    seq0 = lab.seq
    shot_taken = {"mid": False}

    def maybe_shot() -> None:
        pen = lab.ctl.state.get("pen", {})
        if not shot_taken["mid"] and pen.get("strokes", 0) >= 2 and pen.get("down"):
            shot_taken["mid"] = True
            lab.shot("03_writing_OI", view="paper_top", trace="base_yaw")

    recs = lab.run_actions([{"action": "write", "text": "OI"}], timeout=120, on_tick=maybe_shot)
    write = recs[0]
    res = write.result
    analysis = res.get("analysis", {})
    r.metrics.update({"status": write.status.value, "error": write.error, "analysis": analysis, "duration_s": round((write.finished_at or 0) - (write.started_at or 0), 3)})
    r.check("write action completed", write.status.value == "done", write.error)
    r.check("ink was deposited by the physical pen contact", analysis.get("ink_points", 0) > 50, analysis.get("ink_points"))
    r.check("both letters are present (>= 2 ink strokes)", analysis.get("ink_strokes", 0) >= 2, analysis.get("ink_strokes"))
    r.check("ink follows the letters (coverage within 1 mm >= 40%)", analysis.get("coverage_1mm", 0.0) >= 0.4, analysis.get("coverage_1mm"))
    r.check("result is imperfect (mean deviation > 0.2 mm)", analysis.get("deviation_mean_mm", 0.0) > 0.2, analysis.get("deviation_mean_mm"))
    r.check("no FAULT/STALL during writing", lab.ctl.safety.state.value in ("SAFE", "WARNING"), lab.ctl.safety.state.value)
    if "ink" in res:
        cfg = lab.specs.workspace["writing"]
        svg = ink_plot(res["ideal"], res["ink"], "TEST 1 - write('OI'): physical ink vs ideal strokes", up_axis=cfg["up_axis"], advance_axis=cfg["advance_axis"])
        r.artifacts["ink_plot"] = lab.write("test1_write_oi_ink.svg", svg)
        r.artifacts["ink_json"] = lab.write("test1_write_oi_ink.json", json.dumps({"ideal": res["ideal"], "ink": res["ink"]}))
    lab.run_actions([{"action": "move_to", "x": 0.06, "y": 0.0, "z": 0.12, "pitch_deg": -30}], timeout=20)
    shot = lab.shot("03b_OI_result", view="paper_top")
    if shot:
        r.artifacts["screenshot"] = shot
    r.events = lab.events("write_plan", "write_stroke", "servo_stall", "safety_state", "error", since_seq=seq0)
    return r.finalize()


# ---------------------------------------------------------------- TEST 2


def scenario_pick_four(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("test2_pick_four", "TEST 2 - pick and place 4 objects of different mass", False)
    lab.reset("gripper")
    objs = {o["id"]: o for o in lab.specs.workspace["objects"]}
    slots = lab.specs.workspace["drop_zone"]["slots"]
    per: dict[str, Any] = {}
    seq0 = lab.seq
    for i, k in enumerate(["A", "B", "C", "D"]):
        if not lab.ctl.safety.accepts_commands():
            lab.ctl.reset_fault()
        o = objs[k]
        x, y, z = o["position"]
        peak = {"shoulder": 0.0, "elbow": 0.0}
        shot_state = {"done": False}

        def track() -> None:
            for j in peak:
                st = lab.ctl.state["joints"][j]
                peak[j] = max(peak[j], abs(st["tau_request"]) / st["tau_max"])
            if k == "C" and not shot_state["done"] and lab.ctl.state["objects"]["C"]["held"] and lab.ctl.state["objects"]["C"]["position"][2] > 0.04:
                shot_state["done"] = True
                lab.shot("04_pick_and_place", view="objects", trace="shoulder")

        s0 = lab.seq
        recs = lab.run_actions([
            {"action": "pick", "x": x, "y": y, "z": z, "label": k},
            {"action": "place", "x": slots[i][0], "y": slots[i][1], "z": z, "open_width_mm": float(o["size"][1]) * 1000 + 8.0},
            {"action": "home"},
        ], timeout=60, on_tick=track)
        stalled = lab.events("servo_stall", since_seq=s0)
        if stalled and k == "D":
            lab.shot("06_stall", view="objects", trace="shoulder")
        pick, place = recs[0], recs[1]
        final = lab.ctl.state["objects"][k]
        if stalled:
            outcome = "torque_insufficient"
        elif pick.status.value == "done" and not pick.result.get("lifted"):
            outcome = "escaped"
        elif pick.result.get("lifted") and place.status.value == "done" and not final.get("held") and float(final.get("tilt_deg", 0.0)) < 20.0:
            outcome = "held_and_placed"
        elif pick.result.get("lifted"):
            outcome = "held_but_placement_unstable"
        else:
            outcome = "failed"
        per[k] = {
            "name": o["name"],
            "mass_g": round(o["mass"] * 1000, 1),
            "friction": o["friction"],
            "pick_status": pick.status.value,
            "lifted": pick.result.get("lifted"),
            "lift_mm": pick.result.get("lift_mm"),
            "grip_opening_mm": pick.result.get("grip", {}).get("measured_opening_mm"),
            "finger_force_n": pick.result.get("grip", {}).get("finger_force_n"),
            "place_status": place.status.value,
            "place_error": place.error,
            "placement_error_mm": place.result.get("placement_error_mm"),
            "peak_torque_fraction": {j: round(v, 3) for j, v in peak.items()},
            "stalls": stalled,
            "final_position_mm": [round(v * 1000, 1) for v in final["position"]],
            "final_tilt_deg": round(float(final.get("tilt_deg", 0.0)), 2),
            "outcome": outcome,
        }
        if not lab.ctl.safety.accepts_commands():
            # Let the latched state be observed for a moment before the next object.
            lab.ctl.run_for(1.0)
    # Objects placed earlier must still be where they were put (the open
    # jaws of a later placement can knock a neighbour over).
    disturbed = {}
    for k in ("A", "B", "C"):
        if per[k]["outcome"] != "held_and_placed":
            continue
        now = lab.ctl.state["objects"][k]
        disturbed[k] = {
            "moved_mm": round(math.dist([v / 1000 for v in per[k]["final_position_mm"]], now["position"]) * 1000, 2),
            "tilt_deg": round(float(now.get("tilt_deg", 0.0)), 2),
        }
    per["placed_objects_after_run"] = disturbed
    # 2b: light grip -> commanded closing width inside the SG90 dead band
    lab.ctl.reset_fault()
    lab.reset("gripper")
    o = objs["C"]
    x, y, z = o["position"]
    width = float(o["size"][1]) * 1000 - 0.4
    s0 = lab.seq
    recs = lab.run_actions([
        {"action": "gripper_open"},
        {"action": "move_to", "x": x, "y": y, "z": z + 0.05},
        {"action": "move_line", "x": x, "y": y, "z": z, "speed_mps": 0.03},
        {"action": "gripper_close", "width_mm": width, "hold_s": 0.6},
        {"action": "move_line", "x": x, "y": y, "z": z + 0.05, "speed_mps": 0.03},
        {"action": "wait", "seconds": 0.4},
    ], timeout=40)
    grip = recs[3].result
    lifted = lab.ctl.state["objects"]["C"]["position"][2] - z
    per["C_light_grip"] = {
        "commanded_width_mm": round(width, 2),
        "grip": grip,
        "lift_mm": round(lifted * 1000, 2),
        "outcome": "escaped" if lifted < 0.02 else "held",
        "explanation": "A closing command only 0.4 mm narrower than the object asks for ~1 deg of servo travel, inside the SG90 dead band (10 us = 1.8 deg): the amplifier never drives, the jaws exert ~no force and the object stays behind.",
    }
    r.metrics["objects"] = per
    outcomes = {k: v["outcome"] for k, v in per.items() if "outcome" in v}
    r.metrics["outcomes"] = outcomes
    r.check("objects of 4 different masses were attempted", len({per[k]["mass_g"] for k in "ABCD"}) == 4)
    r.check("a light object is held, lifted and placed", any(per[k]["outcome"] == "held_and_placed" for k in "ABC"), outcomes)
    r.check("insufficient torque is detected for the heavy object", per["D"]["outcome"] == "torque_insufficient", per["D"]["stalls"][:1])
    r.check("an object escapes when the grip force is too low", per["C_light_grip"]["outcome"] == "escaped", per["C_light_grip"]["lift_mm"])
    r.check("placed objects are not disturbed by later placements (< 5 mm, < 10 deg)", all(v["moved_mm"] < 5.0 and v["tilt_deg"] < 10.0 for v in disturbed.values()), disturbed)
    r.events = lab.events("pick_result", "place_result", "gripper", "servo_stall", "safety_state", since_seq=seq0)
    return r.finalize()


# ---------------------------------------------------------------- TEST 3


def scenario_shoulder_stall(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("test3_shoulder_stall", "TEST 3 - force a shoulder stall", False)
    # 3a: payload overload (steel cube D, 123 g)
    lab.reset("gripper")
    d = next(o for o in lab.specs.workspace["objects"] if o["id"] == "D")
    x, y, z = d["position"]
    s0 = lab.seq
    lab.run_actions([{"action": "pick", "x": x, "y": y, "z": z, "label": "D"}], timeout=40)
    lab.ctl.run_for(0.5)
    stalls_a = lab.events("servo_stall", since_seq=s0)
    status_a = lab.ctl.status()
    check = lab.ctl.stall_check()
    rejected = False
    try:
        lab.ctl.submit({"action": "home"})
    except Exception:
        rejected = True
    lab.ctl.reset_fault()
    # 3b: obstruction - drive the shoulder down until the tool hits the bench
    lab.reset("gripper")
    s1 = lab.seq
    lab.run_actions([
        {"action": "move_joints", "angles_deg": {"base_yaw": 0, "shoulder": 70, "elbow": -80, "wrist": -60}},
        {"action": "move_joint", "joint": "shoulder", "angle_deg": 10, "speed": 0.15},
    ], timeout=40)
    lab.ctl.run_for(0.5)
    stalls_b = lab.events("servo_stall", since_seq=s1)
    r.metrics.update({
        "payload_case": {"stall_events": stalls_a, "status": {k: status_a[k] for k in ("state", "reason")}, "new_command_rejected_while_stalled": rejected, "stall_check": {k: check["joints"]["shoulder"] for k in ["shoulder"]}},
        "obstruction_case": {"stall_events": stalls_b},
    })
    sh_a = [e for e in stalls_a if e["joint"] == "shoulder"]
    r.check("shoulder stall detected under payload", bool(sh_a), [e["joint"] for e in stalls_a])
    if sh_a:
        e = sh_a[0]
        r.check("requested torque exceeds the servo maximum", abs(e["requested_torque"]) > e["maximum_torque"], {"requested": e["requested_torque"], "maximum": e["maximum_torque"]})
        r.check("stall event is auditable (joint, torques, position, time)", all(k in e for k in ("joint", "requested_torque", "maximum_torque", "position_deg", "time")))
    r.check("state machine entered STALL", status_a["state"] in ("STALL", "FAULT"), status_a["state"])
    r.check("commands are rejected while STALL is latched", rejected)
    r.check("obstruction stall detected (tool pressed into the bench)", bool(stalls_b), [e["joint"] for e in stalls_b])
    r.events = stalls_a + stalls_b
    return r.finalize()


# ---------------------------------------------------------------- TEST 4


def _repeat(lab: Lab, n: int, target: tuple[float, float, float], via: list[tuple[float, float, float]], settle: float) -> list[dict[str, Any]]:
    out = []
    for i in range(n):
        start = via[i % len(via)]
        lab.run_actions([{"action": "move_to", "x": start[0], "y": start[1], "z": start[2]}], timeout=20)
        lab.run_actions([{"action": "move_to", "x": target[0], "y": target[1], "z": target[2], "settle_s": settle}], timeout=20)
        st = lab.ctl.state
        out.append({
            "trial": i + 1,
            "approach_from_mm": [round(v * 1000, 1) for v in start],
            "tcp_mm": [round(v * 1000, 4) for v in st["tcp"]["position"]],
            "joints_deg": {j: round(math.degrees(st["joints"][j]["link_pos"]), 4) for j in ARM_JOINTS},
            "servo_shaft_deg": {j: round(math.degrees(st["joints"][j]["servo_pos"]), 4) for j in ARM_JOINTS},
        })
    return out


def _dispersion(trials: list[dict[str, Any]], target_mm: list[float]) -> dict[str, Any]:
    pts = [t["tcp_mm"] for t in trials]
    mean = [statistics.fmean(p[i] for p in pts) for i in range(3)]
    std = [statistics.pstdev([p[i] for p in pts]) for i in range(3)]
    radii = [math.dist(p, mean) for p in pts]
    errs = [math.dist(p, target_mm) for p in pts]
    return {
        "n": len(pts),
        "mean_mm": [round(v, 3) for v in mean],
        "std_mm": [round(v, 4) for v in std],
        "dispersion_rms_mm": round(math.sqrt(statistics.fmean(r_ * r_ for r_ in radii)), 4),
        "dispersion_max_mm": round(max(radii), 4),
        "range_mm": [round(max(p[i] for p in pts) - min(p[i] for p in pts), 4) for i in range(3)],
        "error_to_target_mean_mm": round(statistics.fmean(errs), 3),
        "error_to_target_max_mm": round(max(errs), 3),
    }


def scenario_repeatability(lab: Lab, n: int = 20) -> ScenarioResult:
    r = ScenarioResult("test4_repeatability", f"TEST 4 - repeatability ({n} identical moves)", False)
    target = (0.16, 0.0, 0.04)
    via_same = [(0.12, 0.09, 0.08)]                  # always approach from the same side
    via_alt = [(0.12, 0.09, 0.08), (0.12, -0.09, 0.08)]   # alternate sides -> backlash hysteresis
    tmm = [v * 1000 for v in target]
    lab.reset("gripper")
    same = _repeat(lab, n, target, via_same, 0.5)
    if lab.screenshots:
        lab.ctl.trace_joint = "wrist"
        lab.ctl._push_hud()
        lab.ctl.run_for(3.0)          # 3 s hold: the trace shows jitter and dead-band hunting
        lab.shot("07_backlash_jitter", view="gripper", trace="wrist")
    lab.reset("gripper")
    alt = _repeat(lab, n, target, via_alt, 0.5)
    left = [t for i, t in enumerate(alt) if i % 2 == 0]
    right = [t for i, t in enumerate(alt) if i % 2 == 1]
    # Ablation: same experiment with dead band, backlash and jitter disabled.
    lab.reset("gripper")
    for s in ALL_SERVOS:
        lab.ctl.backend.servos[s].configure(enable_deadband=False, enable_backlash=False, enable_jitter=False)
    ideal = _repeat(lab, n, target, via_alt, 0.5)
    lab.reset("gripper")   # restores the defaults (all effects ON)
    d_same, d_alt, d_ideal = _dispersion(same, tmm), _dispersion(alt, tmm), _dispersion(ideal, tmm)
    d_left, d_right = _dispersion(left, tmm), _dispersion(right, tmm)
    hysteresis = math.dist(d_left["mean_mm"], d_right["mean_mm"])
    r.metrics.update({
        "target_mm": tmm,
        "same_side": {"dispersion": d_same, "trials": same},
        "alternating_sides": {"dispersion": d_alt, "left": d_left, "right": d_right, "approach_hysteresis_mm": round(hysteresis, 4), "trials": alt},
        "ablation_no_deadband_backlash_jitter": {"dispersion": d_ideal, "trials": ideal},
    })
    r.check(f"{n} trials recorded for the same move", len(same) == n)
    r.check("final positions disperse (effects are physical, not cosmetic)", d_same["dispersion_rms_mm"] > 0.01, d_same["dispersion_rms_mm"])
    r.check("approach direction changes the final position (backlash/dead band hysteresis)", hysteresis > d_same["dispersion_rms_mm"], round(hysteresis, 4))
    r.check("disabling dead band/backlash/jitter collapses the dispersion", d_ideal["dispersion_rms_mm"] < 0.25 * max(d_alt["dispersion_rms_mm"], 1e-9), {"with": d_alt["dispersion_rms_mm"], "without": d_ideal["dispersion_rms_mm"]})
    pts_same = [(t["tcp_mm"][1], t["tcp_mm"][0]) for t in same]
    r.artifacts["scatter_same"] = lab.write("test4_repeatability_same_side.svg", scatter_plot(pts_same, f"TEST 4 - {n} moves from the same side: final TCP (y, x) mm", "y (mm)", "x (mm)", center=(tmm[1], tmm[0])))
    pts_alt = [(t["tcp_mm"][1], t["tcp_mm"][0]) for t in alt]
    r.artifacts["scatter_alt"] = lab.write("test4_repeatability_alternating.svg", scatter_plot(pts_alt, f"TEST 4 - {n} moves, alternating approach side (odd = left, even = right)", "y (mm)", "x (mm)", center=(tmm[1], tmm[0])))
    return r.finalize()


# ---------------------------------------------------------------- TEST 5


def predicted_initial_acceleration(specs: ArmSpecs, kin: ArmKinematics, q: dict[str, float], joint: str = "shoulder") -> dict[str, float]:
    """Gravity torque / (composite inertia + reflected servo inertia) at ``q``.

    Composite inertia of the distal links about the joint axis computed from
    the same part list the plant uses (boxes/cylinders + parallel axis),
    projected on the axis. Coulomb friction of the gear train is subtracted.
    """
    fk = kin.forward(q)
    p = fk.joint_points[joint]
    a = fk.joint_axes[joint]
    links = {"shoulder": ["upper_arm", "forearm", "gripper"], "elbow": ["forearm", "gripper"], "wrist": ["gripper"]}[joint]
    inertia = 0.0
    for link in links:
        frame = fk.frames[link]
        for part in kin.link_parts[link]:
            m = specs.servos[part["servo"]].mass if "servo" in part else float(part.get("mass", 0.0))
            c = frame.apply(tuple(part["offset"]))
            d = [c[i] - p[i] for i in range(3)]
            cross = (d[1] * a[2] - d[2] * a[1], d[2] * a[0] - d[0] * a[2], d[0] * a[1] - d[1] * a[0])
            inertia += m * sum(v * v for v in cross)
            if part["shape"] == "box":
                sx, sy, sz = part["size"]
                inertia += m * (sx * sx + sz * sz) / 12.0   # about the link y axis (pitch axis)
    g = specs.raw["arm"]["gripper"]
    for side in (1, -1):
        c = fk.frames["gripper"].apply((float(g["finger"]["offset_x"]), 0.0, 0.0))
        d = [c[i] - p[i] for i in range(3)]
        cross = (d[1] * a[2] - d[2] * a[1], d[2] * a[0] - d[0] * a[2], d[0] * a[1] - d[1] * a[0])
        inertia += float(g["finger"]["mass"]) * sum(v * v for v in cross)
    servo = specs.servos[specs.joint(joint).servo].raw["parameters"]
    j_ref = float(value(servo["reflected_inertia"]))
    tau_c = float(value(servo["coulomb_friction"]))
    tau_g = -kin.gravity_torques(q)[joint]          # gravity torque (holding torque negated)
    net = math.copysign(max(abs(tau_g) - tau_c, 0.0), tau_g)
    return {"gravity_torque_nm": tau_g, "link_inertia": inertia, "reflected_inertia": j_ref, "alpha0_rad_s2": net / (inertia + j_ref)}


def _pendulum_prediction(lab: Lab, q0: dict[str, float], omega0: float, inertia: float, horizon: float, dt: float = 0.0005) -> list[tuple[float, float]]:
    """Integrate the shoulder as a 1-DOF pendulum (distal joints held rigid):
    I_total * theta'' = tau_gravity(theta) - Coulomb friction. Returns degrees."""
    servo = lab.specs.servos[lab.specs.joint("shoulder").servo].raw["parameters"]
    j_ref = float(value(servo["reflected_inertia"]))
    tau_c = float(value(servo["coulomb_friction"]))
    q = dict(q0)
    th, om, t = q0["shoulder"], omega0, 0.0
    out = [(0.0, math.degrees(th))]
    while t < horizon - 1e-12:
        q["shoulder"] = th
        tau_g = -lab.ctl.kin.gravity_torques(q)["shoulder"]
        fric = -math.copysign(tau_c, om) if abs(om) > 1e-6 else -math.copysign(min(tau_c, abs(tau_g)), tau_g)
        acc = (tau_g + fric) / (inertia + j_ref)
        om += acc * dt
        th += om * dt
        t += dt
        if abs(round(t / 0.02) * 0.02 - t) < dt / 2:
            out.append((round(t, 4), math.degrees(th)))
    return out


def scenario_gravity(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("test5_gravity", "TEST 5 - gravity: power off the shoulder servo", False)
    lab.reset("gripper")
    pose = {"base_yaw": 0.0, "shoulder": 60.0, "elbow": -50.0, "wrist": -40.0}
    lab.run_actions([{"action": "move_joints", "angles_deg": pose}], timeout=20)
    lab.ctl.run_for(1.0)
    # Baseline: powered hold for 1 s
    hold = []
    t0 = lab.ctl.sim_time
    for _ in range(50):
        lab.ctl.tick()
        hold.append((lab.ctl.sim_time - t0, math.degrees(lab.ctl.state["joints"]["shoulder"]["link_pos"])))
    st0 = lab.ctl.state
    q_meas = {j: st0["joints"][j]["link_pos"] for j in ARM_JOINTS}
    omega0 = float(st0["joints"]["shoulder"]["link_vel"])
    plant_inertia = float(st0["joints"]["shoulder"].get("axis_inertia", float("nan")))
    pred = predicted_initial_acceleration(lab.specs, lab.ctl.kin, q_meas)
    s0 = lab.seq
    # t = 0 is the state sampled just before the power-off; the power-off is
    # applied at the start of the next 20 ms PWM period.
    t_off = lab.ctl.sim_time
    fall = [(0.0, math.degrees(q_meas["shoulder"]))]
    tcp_z = [(0.0, st0["tcp"]["position"][2] * 1000)]
    elbow0 = q_meas["elbow"]
    elbow_dev = [(0.0, 0.0)]
    lab.robot.queue([{"action": "servo_power", "joint": "shoulder", "on": False}], source="scenario")
    shot_done = False
    for _ in range(100):
        lab.ctl.tick()
        t = lab.ctl.sim_time - t_off
        fall.append((t, math.degrees(lab.ctl.state["joints"]["shoulder"]["link_pos"])))
        tcp_z.append((t, lab.ctl.state["tcp"]["position"][2] * 1000))
        elbow_dev.append((t, math.degrees(lab.ctl.state["joints"]["elbow"]["link_pos"] - elbow0)))
        if not shot_done and t >= 0.12:
            shot_done = True
            lab.shot("08_gravity_fall", view="side", trace="shoulder")
    impact = next((t for t, z in tcp_z if z < 12.0), None)
    # The 1-DOF model assumes the powered elbow/wrist stay rigid. Compare only
    # while that holds: until the elbow deflects more than 2 deg (measured).
    rigid_until = next((t for t, d in elbow_dev if abs(d) > 2.0), 0.2)
    horizon = max(0.04, min(round((rigid_until - 1e-9) // 0.02 * 0.02, 2), (impact or 0.2) - 0.04))
    model = _pendulum_prediction(lab, q_meas, omega0, pred["link_inertia"], horizon)
    paired = [(t, a, next(b for tt, b in fall if abs(tt - t) < 1e-6)) for t, a in model if t > 0 and any(abs(tt - t) < 1e-6 for tt, _ in fall)]
    drop_model = model[-1][1] - model[0][1]
    drop_meas = paired[-1][2] - fall[0][1] if paired else float("nan")
    drop = fall[0][1] - min(a for _, a in fall)
    hold_range = max(a for _, a in hold) - min(a for _, a in hold)
    early = [(t, math.radians(a - fall[0][1])) for t, a in fall[1:4]]
    alpha_meas = statistics.fmean(2 * d / (t * t) for t, d in early if t > 0)
    r.metrics.update({
        "pose_deg": pose,
        "holding_range_powered_deg": round(hold_range, 3),
        "drop_deg": round(drop, 2),
        "time_to_bench_s": impact,
        "alpha_measured_first_60ms_rad_s2": round(alpha_meas, 2),
        "alpha_model_rad_s2": round(pred["alpha0_rad_s2"], 2),
        "inertia_model_kgm2": round(pred["link_inertia"], 7),
        "inertia_plant_kgm2": round(plant_inertia, 7),
        "model_vs_measured": [[t, round(a, 3), round(b, 3)] for t, a, b in paired],
        "rigid_assumption_valid_until_s": rigid_until,
        "comparison_horizon_s": horizon,
        "elbow_deflection_deg": [[round(t, 3), round(d, 3)] for t, d in elbow_dev],
        "drop_model_deg": round(drop_model, 3),
        "drop_measured_deg": round(drop_meas, 3),
        "fall_deg": [[round(t, 3), round(a, 3)] for t, a in fall],
        "tcp_z_mm": [[round(t, 3), round(z, 2)] for t, z in tcp_z],
    })
    r.check("powered servo holds the pose (range < 3 deg over 1 s)", hold_range < 3.0, round(hold_range, 3))
    r.check("unpowered shoulder falls under gravity (> 20 deg)", drop > 20.0, round(drop, 2))
    r.check("Python mass model and Godot plant agree on the shoulder inertia (5%)", abs(plant_inertia / pred["link_inertia"] - 1.0) < 0.05, {"python": round(pred["link_inertia"], 7), "godot": round(plant_inertia, 7)})
    ratio = drop_meas / drop_model if drop_model else float("nan")
    r.check(f"fall over the first {horizon:.2f} s matches a rigid pendulum model within 25%", 0.75 <= ratio <= 1.25, {"measured_deg": round(drop_meas, 3), "model_deg": round(drop_model, 3), "ratio": round(ratio, 3)})
    r.artifacts["fall_plot"] = lab.write("test5_gravity_fall.svg", line_plot(
        [("shoulder angle after power-off (physics)", fall), ("rigid pendulum model", model), ("powered hold, previous 1 s", hold)],
        "TEST 5 - shoulder servo powered off at t = 0", "time (s)", "shoulder angle (deg)"))
    r.events = lab.events("servo_power", "collision", since_seq=s0)[:20]
    return r.finalize()


# ---------------------------------------------------------------- extras


def scenario_emergency_stop(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("extra_emergency_stop", "E-STOP during writing", False)
    lab.reset("pen")
    lab.robot.queue([{"action": "write", "text": "OI"}, {"action": "home"}], source="scenario")
    while lab.ctl.sim_time < 7.0:
        lab.ctl.tick()
    before = lab.ctl.status()
    lab.robot.emergency_stop("scenario: operator pressed E-STOP")
    pose0 = {j: lab.ctl.measured(j) for j in ARM_JOINTS}
    lab.ctl.run_for(1.0)
    drift = max(abs(math.degrees(lab.ctl.measured(j) - pose0[j])) for j in ARM_JOINTS)
    lab.shot("10_emergency_stop", view="overview", trace="shoulder")
    rejected = False
    try:
        lab.ctl.submit({"action": "home"})
    except Exception:
        rejected = True
    after = lab.ctl.status()
    ok_reset = lab.robot.reset()
    lab.ctl.run_for(0.2)
    r.metrics.update({"active_before": before["active_action"], "queue_before": before["queue"], "state_after": after["state"], "queue_after": after["queue"], "pose_drift_deg_1s": round(drift, 3), "rejected_while_estop": rejected, "reset_ok": ok_reset, "state_after_reset": lab.ctl.safety.state.value})
    r.check("state is EMERGENCY_STOP", after["state"] == "EMERGENCY_STOP", after["state"])
    r.check("queue cleared", not after["queue"] and after["active_action"] is None)
    r.check("new commands rejected", rejected)
    r.check("arm holds position (drift < 5 deg in 1 s)", drift < 5.0, round(drift, 3))
    r.check("explicit reset returns to SAFE", ok_reset and lab.ctl.safety.state.value == "SAFE", lab.ctl.safety.state.value)
    return r.finalize()


def scenario_pid_step(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("extra_pid_step", "PID step response (elbow), gains changed at runtime", False)
    lab.reset("gripper")
    lab.run_actions([{"action": "move_joints", "angles_deg": {"shoulder": 90, "elbow": -90, "wrist": -45}}], timeout=20)
    lab.ctl.run_for(0.5)
    runs = []
    base = lab.specs.servos["MG90S"]
    for label, gains in [("default", {"kp": base.kp, "ki": base.ki, "kd": base.kd}), ("Kp x2, Kd x0.3", {"kp": base.kp * 2, "ki": 0.0, "kd": base.kd * 0.3}), ("with Ki", {"kp": base.kp, "ki": 3.0, "kd": base.kd})]:
        lab.run_actions([{"action": "set_pid", "joint": "elbow", **gains}], timeout=5)
        lab.run_actions([{"action": "move_joint", "joint": "elbow", "angle_deg": -90}], timeout=10)
        lab.ctl.run_for(0.5)
        m = lab.robot.arm.step_response("elbow", 20.0, duration_s=1.5)
        runs.append({"label": label, "gains": gains, **{k: v for k, v in m.items() if k != "samples"}, "samples": m["samples"]})
    lab.run_actions([{"action": "set_pid", "joint": "elbow", "kp": base.kp, "ki": base.ki, "kd": base.kd}], timeout=5)
    r.metrics["runs"] = [{k: v for k, v in run.items() if k != "samples"} for run in runs]
    r.artifacts["step_plot"] = lab.write("pid_step_elbow.svg", line_plot([(run["label"], [(t, a) for t, a in run["samples"]]) for run in runs], "Elbow step response +20 deg (servo PID, simulated)", "time (s)", "elbow angle (deg)"))
    r.check("metrics computed for each gain set", all("overshoot_pct" in run for run in runs))
    r.check("changing gains changes the response", len({round(run["overshoot_pct"], 2) for run in runs}) > 1 or len({run["settling_time_s"] for run in runs}) > 1)
    return r.finalize()


def scenario_camera(lab: Lab) -> ScenarioResult:
    r = ScenarioResult("extra_camera", "cam.frame() from the end effector", False)
    if not lab.ctl.backend.capabilities.camera:
        r.check("camera available (needs a renderer; headless runs skip this)", False, "headless")
        return r.finalize()
    lab.reset("gripper")
    b = next(o for o in lab.specs.workspace["objects"] if o["id"] == "B")
    recs = lab.run_actions([{"action": "gripper_open"}, {"action": "move_to", "x": b["position"][0], "y": b["position"][1], "z": 0.07, "pitch_deg": -90}], timeout=20)
    lab.ctl.run_for(0.3)
    frame = lab.robot.cam.frame()
    r.check("arm moved above object B before capturing", recs[1].status.value == "done", recs[1].error)
    cam_xy = frame.pose.get("position", [0, 0, 0])[:2]
    r.check("camera is above object B (< 30 mm horizontally)", math.dist(cam_xy, b["position"][:2]) < 0.03, [round(v, 4) for v in cam_xy])
    path = frame.save(lab.out / "screenshots" / "09_cam_frame.png")
    lab.shot("09b_camera_hud", view="objects")
    r.artifacts["frame"] = str(path)
    r.metrics["frame"] = frame.to_dict()
    r.check("frame has the configured resolution", (frame.width, frame.height) == tuple(lab.specs.raw["arm"]["links"]["gripper"]["camera"]["resolution"]), (frame.width, frame.height))
    r.check("frame is a PNG", frame.data[:8] == b"\x89PNG\r\n\x1a\n")
    r.check("frame carries pose and intrinsics", bool(frame.pose) and bool(frame.intrinsics))
    return r.finalize()


def scenario_stability(lab: Lab) -> ScenarioResult:
    """The same reach-out move with the base clamped (design) and free-standing."""
    r = ScenarioResult("extra_stability", "Stability: reach out with a clamped vs a free-standing base", False)
    runs: dict[str, Any] = {}
    for mount in ("clamped", "free"):
        lab.ctl.reset_world("gripper", lab.specs.default_seed, base_mount=mount)
        tilt = [(0.0, float(lab.ctl.state.get("base", {}).get("tilt_deg", 0.0) or 0.0))]
        t0 = lab.ctl.sim_time
        shot = {"done": False}

        def track() -> None:
            b = lab.ctl.state.get("base", {})
            tilt.append((lab.ctl.sim_time - t0, float(b.get("tilt_deg", 0.0) or 0.0)))
            if mount == "free" and not shot["done"] and tilt[-1][1] > 15.0:
                shot["done"] = True
                lab.shot("11_unstable_free_base", view="overview", trace="shoulder")

        recs = lab.run_actions([{"action": "move_joints", "angles_deg": {"base_yaw": 0, "shoulder": 10, "elbow": 0, "wrist": -20}, "speed": 0.5}], timeout=10, on_tick=track)
        for _ in range(75):
            lab.ctl.tick()
            track()
        runs[mount] = {"move_status": recs[0].status.value, "max_base_tilt_deg": round(max(v for _, v in tilt), 2), "final_state": lab.ctl.safety.state.value, "tilt_deg": [[round(t, 3), round(v, 2)] for t, v in tilt[::5]]}
    lab.ctl.reset_world("gripper", lab.specs.default_seed, base_mount="clamped")
    if lab.ctl.safety.state.value != "SAFE":
        lab.ctl.reset_fault()
    # Static check: the combined centre of mass (base + arm) must stay above
    # the base footprint (radius of the base plate) for a free-standing robot.
    kin = lab.ctl.kin
    reach = {"base_yaw": 0.0, "shoulder": math.radians(10), "elbow": 0.0, "wrist": math.radians(-20)}
    fk = kin.forward(reach)
    base_plate = next(p for p in lab.specs.raw["arm"]["base"]["parts"] if p["name"] == "base_plate")
    m_base = float(base_plate["mass"]) + lab.specs.servos["MG90S"].mass
    total_m, moment_x = m_base, 0.0
    for link in ("turret", "upper_arm", "forearm", "gripper"):
        mp = kin.link_mass[link]
        c = fk.frames[link].apply(mp.com)
        total_m += mp.mass
        moment_x += mp.mass * c[0]
    com_x = moment_x / total_m
    r.metrics.update({"runs": runs, "combined_com_x_m": round(com_x, 4), "base_radius_m": float(base_plate["radius"]), "total_mass_kg": round(total_m, 4)})
    r.check("clamped base stays put (design default)", runs["clamped"]["max_base_tilt_deg"] < 1.0, runs["clamped"]["max_base_tilt_deg"])
    r.check("static model predicts tipping (combined COM beyond the base edge)", com_x > float(base_plate["radius"]), {"com_x_m": round(com_x, 4), "base_radius_m": base_plate["radius"]})
    r.check("free-standing base lifts off and tips (> 5 deg): the arm loses stability", runs["free"]["max_base_tilt_deg"] > 5.0, runs["free"]["max_base_tilt_deg"])
    return r.finalize()


SCENARIOS: dict[str, Callable[..., ScenarioResult]] = {
    "write_oi": scenario_write_oi,
    "pick_four": scenario_pick_four,
    "shoulder_stall": scenario_shoulder_stall,
    "repeatability": scenario_repeatability,
    "gravity": scenario_gravity,
    "emergency_stop": scenario_emergency_stop,
    "pid_step": scenario_pid_step,
    "camera": scenario_camera,
    "stability": scenario_stability,
}


def run_scenarios(names: list[str], out_dir: str | Path, *, headless: bool = True, screenshots: bool = False, seed: int | None = None, journal: str | Path | None = None, specs_path: str | Path | None = None) -> dict[str, Any]:
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    specs = load_specs(specs_path)
    robot = Robot.simulation(tool="gripper", seed=seed, headless=headless, specs=specs, journal_path=journal or out / "telemetry.jsonl", godot_log=out / "godot.log")
    lab = Lab(robot, out, screenshots=screenshots)
    results = []
    try:
        if screenshots:
            lab.reset("gripper")
            lab.ctl.run_for(1.0)
            lab.shot("01_lab_overview", view="overview", trace="shoulder")
            lab.shot("02_arm_at_rest", view="gripper", trace="elbow")
        for name in names:
            t0 = time.time()
            k0 = lab.ctl.total_ticks
            fn = SCENARIOS[name]
            res = fn(lab)
            res.wall_s = time.time() - t0
            res.sim_s = (lab.ctl.total_ticks - k0) * lab.ctl.dt
            results.append(res)
            (out / f"{res.name}.json").write_text(json.dumps(res.to_dict(), indent=2, default=str), encoding="utf-8")
        if screenshots:
            lab.reset("pen")
            lab.robot.queue([{"action": "write", "text": "OI"}], source="scenario")
            while lab.ctl.sim_time < 9.0:
                lab.ctl.tick()
            lab.shot("05_telemetry_panel", view="behind", trace="shoulder")
    finally:
        robot.close()
    summary = {
        "engine": robot.controller.backend.info if hasattr(robot.controller.backend, "info") else {},
        "seed": seed if seed is not None else specs.default_seed,
        "results": [{"name": x.name, "title": x.title, "passed": x.passed, "wall_s": round(x.wall_s, 2), "sim_s": round(x.sim_s, 2), "failed_checks": [c for c in x.checks if not c["ok"]]} for x in results],
        "screenshots": lab.shots,
        "all_passed": all(x.passed for x in results),
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=2, default=str), encoding="utf-8")
    return summary
