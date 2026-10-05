"""Safety state machine of the robot controller.

States (ordered by severity):

* ``SAFE``            normal operation.
* ``WARNING``         operation continues; a soft condition is present
                      (torque near the servo limit, large tracking error,
                      unexpected collision). Clears by itself.
* ``STALL``           a servo demanded more torque than it can produce while
                      not moving. The queue is cancelled and the arm holds
                      the measured position. Latched until ``reset()``.
* ``FAULT``           a stall lasted longer than ``stall_to_fault_time``, the
                      backend stopped answering, or the physics state is
                      invalid. The stalled servo is powered off. Latched.
* ``EMERGENCY_STOP``  commanded stop. Queue cleared, every command rejected,
                      servos hold (``estop_mode = "hold"``) or are powered
                      off (``"power_off"``). Latched until ``reset()``.

The supervisor decides; the controller obeys. No transition happens silently:
every change produces an event for the journal.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Mapping

from .specs import ArmSpecs, value


class SafetyState(str, Enum):
    SAFE = "SAFE"
    WARNING = "WARNING"
    STALL = "STALL"
    FAULT = "FAULT"
    EMERGENCY_STOP = "EMERGENCY_STOP"


LATCHED = {SafetyState.STALL, SafetyState.FAULT, SafetyState.EMERGENCY_STOP}
SEVERITY = {
    SafetyState.SAFE: 0,
    SafetyState.WARNING: 1,
    SafetyState.STALL: 2,
    SafetyState.FAULT: 3,
    SafetyState.EMERGENCY_STOP: 4,
}


@dataclass(slots=True)
class StallRecord:
    joint: str
    started_at: float
    requested_torque: float
    maximum_torque: float
    available_torque: float
    position: float
    target: float
    static_torque_model: float | None
    gripper_holding: bool = False
    lever_arm: float | None = None       # horizontal joint-axis -> TCP distance (m)
    gravity: float = 9.80665
    active: bool = True       # the servo is still stalled
    faulted: bool = False     # the stall escalated to FAULT (servo powered off)


@dataclass(slots=True)
class SafetySupervisor:
    specs: ArmSpecs
    state: SafetyState = SafetyState.SAFE
    reason: str = ""
    since: float = 0.0
    warnings: list[str] = field(default_factory=list)
    stalls: dict[str, StallRecord] = field(default_factory=dict)
    powered_off_by_safety: set[str] = field(default_factory=set)
    events: list[dict[str, Any]] = field(default_factory=list)
    stall_history: list[dict[str, Any]] = field(default_factory=list)
    warning_enter_s: float = 0.1
    warning_clear_s: float = 0.3
    _warn_since: float | None = None
    _clear_since: float | None = None

    @property
    def estop_mode(self) -> str:
        return str(self.specs.safety.get("estop_mode", "hold"))

    def accepts_commands(self) -> bool:
        return self.state not in LATCHED

    def _transition(self, new: SafetyState, reason: str, sim_time: float, **extra: Any) -> None:
        if new == self.state and reason == self.reason:
            return
        old = self.state
        self.state = new
        self.reason = reason
        self.since = sim_time
        self.events.append({"event": "safety_state", "from": old.value, "to": new.value, "reason": reason, "sim_time": sim_time, **extra})

    def drain_events(self) -> list[dict[str, Any]]:
        out, self.events = self.events, []
        return out

    # -- operator actions --------------------------------------------------

    def emergency_stop(self, reason: str, sim_time: float) -> None:
        self._transition(SafetyState.EMERGENCY_STOP, reason, sim_time, estop_mode=self.estop_mode)

    def reset(self, sim_time: float) -> bool:
        """Clear a latched state. Refused while a servo is still stalled."""
        active = [j for j, s in self.stalls.items() if s.active and not s.faulted]
        if self.state == SafetyState.STALL and active:
            self.events.append({"event": "safety_reset_refused", "reason": f"servo still stalled: {', '.join(active)}", "sim_time": sim_time})
            return False
        self.stalls.clear()
        restored = sorted(self.powered_off_by_safety)
        self.powered_off_by_safety.clear()
        self._transition(SafetyState.SAFE, "reset by operator", sim_time, restored_power=restored)
        return True

    # -- evaluation -----------------------------------------------------------

    def evaluate(self, state: Mapping[str, Any], sim_time: float, *, static_torques: Mapping[str, float] | None = None, expected_contacts: set[str] | None = None, gripper_holding: bool = False, lever_arms: Mapping[str, float] | None = None) -> None:
        """Inspect one backend state sample and update the safety state."""
        safety = self.specs.safety
        stall_fault_time = float(value(safety["stall_to_fault_time"]))
        warn_fraction = float(safety["torque_warning_fraction"])
        track_warn = math.radians(float(safety["tracking_error_warning_deg"]))
        warnings: list[str] = []
        joints: dict[str, Any] = dict(state.get("joints", {}))
        if "gripper" in state:
            joints["gripper"] = state["gripper"]

        # Invalid physics state -> FAULT.
        for name, j in joints.items():
            for key in ("link_pos", "link_vel", "tau_motor"):
                if key not in j:
                    continue
                v = j[key]
                if not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v):
                    self._transition(SafetyState.FAULT, f"invalid physics state: {name}.{key}={v!r}", sim_time)
                    return

        for name, j in joints.items():
            is_gripper = name == "gripper"
            stalled = bool(j.get("stalled"))
            if stalled and is_gripper and safety.get("gripper_stall_is_grip", True):
                # A hobby gripper squeezing an object is a stalled servo by design.
                continue
            if stalled:
                rec = self.stalls.get(name)
                if rec is None:
                    rec = StallRecord(
                        joint=name,
                        started_at=sim_time - float(j.get("stall_time", 0.0)),
                        requested_torque=float(j["tau_request"]),
                        maximum_torque=float(j["tau_max"]),
                        available_torque=float(j["tau_avail"]),
                        position=float(j["link_pos"]),
                        target=float(j["target"]),
                        static_torque_model=None if static_torques is None else float(static_torques.get(name, float("nan"))),
                        gripper_holding=gripper_holding,
                        lever_arm=None if lever_arms is None else lever_arms.get(name),
                        gravity=self.specs.gravity,
                    )
                    self.stalls[name] = rec
                    event = {
                        "event": "servo_stall",
                        "joint": name,
                        "servo": j.get("servo"),
                        "requested_torque": round(rec.requested_torque, 5),
                        "maximum_torque": round(rec.maximum_torque, 5),
                        "available_torque": round(rec.available_torque, 5),
                        "position_deg": round(math.degrees(rec.position), 3),
                        "target_deg": round(math.degrees(rec.target), 3),
                        "static_torque_model": None if rec.static_torque_model is None or not math.isfinite(rec.static_torque_model) else round(rec.static_torque_model, 5),
                        "current_a": round(float(j.get("current", 0.0)), 4),
                        "probable_cause": _stall_cause(rec),
                        "payload_lower_bound_kg": _payload_bound(rec),
                        "gripper_holding": rec.gripper_holding,
                        "time": round(sim_time, 4),
                        "sim_time": sim_time,
                    }
                    self.events.append(event)
                    self.stall_history.append(event)
                    if SEVERITY[self.state] < SEVERITY[SafetyState.STALL]:
                        self._transition(SafetyState.STALL, f"servo stall: {name}", sim_time, joint=name)
                else:
                    rec.active = True
                    if abs(float(j["tau_request"])) > abs(rec.requested_torque):
                        rec.requested_torque = float(j["tau_request"])
                    if not rec.faulted and sim_time - rec.started_at >= stall_fault_time:
                        rec.faulted = True
                        self.powered_off_by_safety.add(name)
                        self.events.append({"event": "servo_power_off", "joint": name, "reason": f"stall longer than {stall_fault_time:.1f} s", "sim_time": sim_time})
                        if SEVERITY[self.state] < SEVERITY[SafetyState.FAULT]:
                            self._transition(SafetyState.FAULT, f"persistent stall: {name} powered off", sim_time, joint=name)
            else:
                rec = self.stalls.get(name)
                if rec is not None and rec.active and not rec.faulted:
                    self.events.append({"event": "servo_stall_cleared", "joint": name, "duration": round(sim_time - rec.started_at, 4), "sim_time": sim_time})
                    rec.active = False   # the STALL state stays latched until reset()
            if not is_gripper and bool(j.get("powered", True)):
                if abs(float(j["tau_request"])) >= warn_fraction * float(j["tau_max"]) and not stalled:
                    warnings.append(f"{name} torque {abs(float(j['tau_request'])):.3f}/{float(j['tau_max']):.3f} N*m")
                err = float(j["target"]) - float(j["link_pos"])
                if abs(err) >= track_warn:
                    warnings.append(f"{name} tracking error {math.degrees(err):+.1f} deg")

        for c in state.get("collisions", []):
            if c.get("event") != "collision_begin":
                continue
            pair = {str(c.get("a")), str(c.get("b"))}
            arm_links = {"turret", "upper_arm", "forearm", "gripper", "finger_left", "finger_right"}
            if pair & arm_links and not (expected_contacts and pair & expected_contacts):
                if not (pair <= arm_links | {"paper"} and "paper" in pair):
                    warnings.append(f"collision {c.get('a')}<->{c.get('b')}")

        self.warnings = warnings
        if warnings:
            self._warn_since = sim_time if self._warn_since is None else self._warn_since
            self._clear_since = None
        else:
            self._clear_since = sim_time if self._clear_since is None else self._clear_since
            self._warn_since = None
        if self.state in LATCHED:
            return
        # Hysteresis: a condition must persist to raise WARNING (filters
        # one-period torque spikes while accelerating) and must be absent for
        # a while to clear it (avoids SAFE/WARNING chatter in the journal).
        if self.state == SafetyState.SAFE and warnings and sim_time - self._warn_since >= self.warning_enter_s:
            self._transition(SafetyState.WARNING, warnings[0], sim_time, warnings=warnings[:6])
        elif self.state == SafetyState.WARNING and not warnings and sim_time - self._clear_since >= self.warning_clear_s:
            self._transition(SafetyState.SAFE, "conditions cleared", sim_time)

    def joint_states(self, state: Mapping[str, Any]) -> dict[str, str]:
        out: dict[str, str] = {}
        joints: dict[str, Any] = dict(state.get("joints", {}))
        if "gripper" in state:
            joints["gripper"] = state["gripper"]
        for name, j in joints.items():
            if not j.get("powered", True):
                out[name] = "OFF"
            elif j.get("stalled"):
                out[name] = "GRIP" if name == "gripper" else "STALL"
            elif j.get("saturated"):
                out[name] = "SAT"
            elif j.get("in_deadband"):
                out[name] = "DB"
            else:
                out[name] = "OK"
        return out


def _payload_bound(rec: StallRecord) -> float | None:
    """Lower bound of an unknown payload at the TCP that explains the stall.

    At a stall the servo delivers about its maximum torque and still cannot
    hold the load: tau_max <= tau_arm + m * g * d, so m >= (tau_max - tau_arm) / (g d).
    Only meaningful for pitch joints with a non-zero lever arm.
    """
    m = rec.static_torque_model
    if m is None or not math.isfinite(m) or not rec.lever_arm or rec.lever_arm < 0.01:
        return None
    # Only a servo lifting against gravity can be overloaded by a payload: if
    # it is pushing in the direction gravity already pulls, the extra load is
    # an obstruction and a payload bound would be meaningless.
    if rec.requested_torque * m <= 0.0:
        return None
    return round(max(0.0, (rec.maximum_torque - abs(m)) / (rec.gravity * rec.lever_arm)), 4)


def _stall_cause(rec: StallRecord) -> str:
    """Explain a stall from the arm-only static model and the gripper state.

    The controller does not know the mass of what it holds, so the model
    covers the arm only. The verdict says what the numbers support and no more.
    """
    m = rec.static_torque_model
    if m is None or not math.isfinite(m):
        return "unknown (no static model available)"
    if abs(m) >= 0.9 * rec.maximum_torque:
        return "overload: the arm's own weight needs %.3f N*m of %.3f available" % (abs(m), rec.maximum_torque)
    bound = _payload_bound(rec)
    if rec.gripper_holding:
        extra = "" if bound is None else "; payload >= %.0f g" % (bound * 1000)
        return "overload by payload: gripper is closed on an object; arm alone needs %.3f N*m of %.3f%s" % (abs(m), rec.maximum_torque, extra)
    return "external load or obstruction: arm alone needs %.3f N*m of %.3f; the rest comes from contact, an obstacle or a hard stop" % (abs(m), rec.maximum_torque)
