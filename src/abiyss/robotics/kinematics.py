"""Forward/inverse kinematics and static gravity model of the arm.

Pure Python, no third-party dependencies. Used by the controller for both
backends: kinematics is a property of the arm, not of the simulator.

Conventions (see ARM_SPECS.json "conventions"): robot frame +x forward,
+y left, +z up; pitch joints rotate about -y so positive angles raise the
distal link; tool pitch = shoulder + elbow + wrist.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Iterable, Mapping, Sequence

from ..errors import ValidationError
from .specs import ArmSpecs

Vec3 = tuple[float, float, float]
Mat3 = tuple[Vec3, Vec3, Vec3]   # row-major

ARM_JOINTS = ("base_yaw", "shoulder", "elbow", "wrist")


class IKError(ValidationError):
    """A Cartesian target cannot be reached within the joint limits."""


def _add(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _scale(a: Vec3, s: float) -> Vec3:
    return (a[0] * s, a[1] * s, a[2] * s)


def _dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _cross(a: Vec3, b: Vec3) -> Vec3:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _matvec(m: Mat3, v: Vec3) -> Vec3:
    return (_dot(m[0], v), _dot(m[1], v), _dot(m[2], v))


def _matmul(a: Mat3, b: Mat3) -> Mat3:
    cols = tuple(zip(*b))
    return tuple(tuple(_dot(row, col) for col in cols) for row in a)  # type: ignore[return-value]


IDENTITY: Mat3 = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))


def axis_angle(axis: Vec3, angle: float) -> Mat3:
    n = math.sqrt(_dot(axis, axis))
    x, y, z = (axis[0] / n, axis[1] / n, axis[2] / n)
    c, s = math.cos(angle), math.sin(angle)
    t = 1.0 - c
    return (
        (t * x * x + c, t * x * y - s * z, t * x * z + s * y),
        (t * x * y + s * z, t * y * y + c, t * y * z - s * x),
        (t * x * z - s * y, t * y * z + s * x, t * z * z + c),
    )


def wrap_pi(a: float) -> float:
    a = math.fmod(a + math.pi, 2.0 * math.pi)
    if a < 0:
        a += 2.0 * math.pi
    return a - math.pi


@dataclass(frozen=True, slots=True)
class Frame:
    rotation: Mat3
    origin: Vec3

    def apply(self, p: Vec3) -> Vec3:
        return _add(_matvec(self.rotation, p), self.origin)

    def compose(self, rotation: Mat3, origin: Vec3) -> "Frame":
        return Frame(_matmul(self.rotation, rotation), self.apply(origin))


@dataclass(frozen=True, slots=True)
class FKResult:
    frames: dict[str, Frame]          # link frames (base, turret, upper_arm, forearm, gripper)
    joint_points: dict[str, Vec3]     # joint axis points in the robot frame
    joint_axes: dict[str, Vec3]       # joint axes in the robot frame
    tcp: Vec3
    tool_axis: Vec3
    pitch: float                      # tool pitch (rad) = angle of tool axis above horizontal


@dataclass(frozen=True, slots=True)
class MassProps:
    mass: float
    com: Vec3                         # link frame


def part_mass(specs: ArmSpecs, part: Mapping) -> float:
    if "servo" in part:
        return specs.servos[part["servo"]].mass
    return float(part.get("mass", 0.0))


def mass_properties(specs: ArmSpecs, parts: Iterable[Mapping]) -> MassProps:
    total = 0.0
    acc = (0.0, 0.0, 0.0)
    for part in parts:
        m = part_mass(specs, part)
        total += m
        acc = _add(acc, _scale(tuple(float(v) for v in part["offset"]), m))  # type: ignore[arg-type]
    if total <= 0:
        return MassProps(0.0, (0.0, 0.0, 0.0))
    return MassProps(total, _scale(acc, 1.0 / total))


class ArmKinematics:
    """Kinematic + static model built from :class:`ArmSpecs`."""

    def __init__(self, specs: ArmSpecs, tool: str = "gripper") -> None:
        self.specs = specs
        self.tool = tool
        arm = specs.raw["arm"]
        self.link_parts: dict[str, list[dict]] = {name: list(link["parts"]) for name, link in arm["links"].items()}
        self.link_mass: dict[str, MassProps] = {name: mass_properties(specs, parts) for name, parts in self.link_parts.items()}
        g = arm["gripper"]
        finger_mass = 2.0 * float(g["finger"]["mass"])
        gp = self.link_mass["gripper"]
        # Fingers (+pads) ride on the gripper link: add them at the finger offset.
        fx = float(g["finger"]["offset_x"])
        total = gp.mass + finger_mass
        com = _scale(_add(_scale(gp.com, gp.mass), _scale((fx, 0.0, 0.0), finger_mass)), 1.0 / total)
        self.link_mass["gripper"] = MassProps(total, com)
        self.pen_mass = float(arm["pen"]["mass"])
        tip = arm["pen"]["tip_offset"]
        self.pen_com = (float(tip[0]) - float(arm["pen"]["length"]) / 2.0, 0.0, 0.0)
        self._planar_check()

    # -- configuration ------------------------------------------------------

    def set_tool(self, tool: str) -> None:
        if tool not in ("gripper", "pen"):
            raise ValidationError("tool must be 'gripper' or 'pen'")
        self.tool = tool

    def tcp_offset(self) -> Vec3:
        return self.specs.tcp_offset(self.tool)

    def _planar_check(self) -> None:
        js = {j.name: j for j in self.specs.joints}
        if js["base_yaw"].axis != (0.0, 0.0, 1.0):
            raise ValidationError("IK assumes a vertical base yaw axis")
        for name in ("shoulder", "elbow", "wrist"):
            if js[name].axis != (0.0, -1.0, 0.0):
                raise ValidationError("IK assumes pitch joints about -y")
        for name in ("elbow", "wrist"):
            if abs(js[name].origin[1]) > 1e-9:
                raise ValidationError("IK assumes planar pitch chain (zero y offsets)")
        if abs(js["shoulder"].origin[0]) > 1e-9 or abs(js["shoulder"].origin[1]) > 1e-9:
            raise ValidationError("IK assumes the shoulder axis intersects the yaw axis")

    # -- forward kinematics ---------------------------------------------------

    def forward(self, q: Mapping[str, float]) -> FKResult:
        frames: dict[str, Frame] = {"base": Frame(IDENTITY, (0.0, 0.0, 0.0))}
        points: dict[str, Vec3] = {}
        axes: dict[str, Vec3] = {}
        for j in self.specs.joints:
            parent = frames[j.parent]
            points[j.name] = parent.apply(j.origin)
            axes[j.name] = _matvec(parent.rotation, j.axis)
            frames[j.child] = parent.compose(axis_angle(j.axis, float(q[j.name])), j.origin)
        g = frames["gripper"]
        tcp = g.apply(self.tcp_offset())
        tool_axis = _matvec(g.rotation, (1.0, 0.0, 0.0))
        pitch = math.atan2(tool_axis[2], math.hypot(tool_axis[0], tool_axis[1]))
        return FKResult(frames, points, axes, tcp, tool_axis, pitch)

    # -- inverse kinematics ---------------------------------------------------

    def _planar_vectors(self) -> tuple[Vec3, tuple[float, float], tuple[float, float], tuple[float, float]]:
        js = {j.name: j for j in self.specs.joints}
        shoulder_point = _add(js["base_yaw"].origin, js["shoulder"].origin)
        v1 = (js["elbow"].origin[0], js["elbow"].origin[2])
        v2 = (js["wrist"].origin[0], js["wrist"].origin[2])
        off = self.tcp_offset()
        v3 = (off[0], off[2])
        return shoulder_point, v1, v2, v3

    def inverse(self, target: Sequence[float], pitch: float = -math.pi / 2.0, *, prefer: str = "up", limits: str = "soft") -> dict[str, float]:
        """Joint angles (rad) placing the TCP at ``target`` with tool ``pitch``.

        Raises :class:`IKError` when the target is out of reach or every IK
        branch violates the joint limits.
        """
        x, y, z = (float(target[0]), float(target[1]), float(target[2]))
        r = math.hypot(x, y)
        if r < 1e-6:
            raise IKError("target on the base axis: yaw undefined")
        yaw = math.atan2(y, x)
        sp, v1, v2, v3 = self._planar_vectors()
        s_r, s_z = math.hypot(sp[0], sp[1]), sp[2]
        c, s = math.cos(pitch), math.sin(pitch)
        w_r = r - (v3[0] * c - v3[1] * s)
        w_z = z - (v3[0] * s + v3[1] * c)
        d_r, d_z = w_r - s_r, w_z - s_z
        dist2 = d_r * d_r + d_z * d_z
        l1 = math.hypot(*v1)
        l2 = math.hypot(*v2)
        b1 = math.atan2(v1[1], v1[0])
        b2 = math.atan2(v2[1], v2[0])
        cg = (dist2 - l1 * l1 - l2 * l2) / (2.0 * l1 * l2)
        if cg > 1.0 + 1e-9 or cg < -1.0 - 1e-9:
            raise IKError(f"target out of reach: wrist distance {math.sqrt(dist2):.4f} m, reach {l1 + l2:.4f} m")
        cg = min(1.0, max(-1.0, cg))
        branches = [-math.acos(cg), math.acos(cg)] if prefer == "up" else [math.acos(cg), -math.acos(cg)]
        failures: list[str] = []
        for gamma in branches:
            a = math.atan2(d_z, d_r) - math.atan2(l2 * math.sin(gamma), l1 + l2 * math.cos(gamma))
            q_sh = wrap_pi(a - b1)
            q_el = wrap_pi(gamma - b2 + b1)
            q_wr = wrap_pi(pitch - q_sh - q_el)
            sol = {"base_yaw": wrap_pi(yaw), "shoulder": q_sh, "elbow": q_el, "wrist": q_wr}
            # Shoulder range extends past 180 deg: unwrap into the limit window.
            sol = {k: self._unwrap_into_limits(k, v) for k, v in sol.items()}
            bad = self.limit_violations(sol, limits)
            if not bad:
                return sol
            failures.append(", ".join(bad))
        raise IKError("joint limits violated for every IK branch: " + " | ".join(failures))

    def _unwrap_into_limits(self, joint: str, angle: float) -> float:
        lo, hi = self.specs.joint(joint).soft_limits
        for cand in (angle, angle + 2 * math.pi, angle - 2 * math.pi):
            if lo - 1e-9 <= cand <= hi + 1e-9:
                return cand
        return angle

    def limit_violations(self, q: Mapping[str, float], which: str = "soft") -> list[str]:
        out = []
        for name in ARM_JOINTS:
            j = self.specs.joint(name)
            lo, hi = j.soft_limits if which == "soft" else j.hard_stops
            a = float(q[name])
            if a < lo - 1e-9 or a > hi + 1e-9:
                out.append(f"{name}={math.degrees(a):.1f}deg outside [{math.degrees(lo):.0f}, {math.degrees(hi):.0f}]")
        return out

    # -- statics ----------------------------------------------------------------

    def gravity_torques(self, q: Mapping[str, float], payload_kg: float = 0.0) -> dict[str, float]:
        """Static torque (N*m) each joint must supply to hold ``q`` against gravity.

        Positive = torque in the joint's positive direction. Uses the same part
        masses as the Godot plant (servo masses from datasheets, printed parts
        estimated). ``payload_kg`` is a point mass at the TCP.
        """
        fk = self.forward(q)
        g = self.specs.gravity
        masses: list[tuple[str, float, Vec3]] = []
        for link in ("turret", "upper_arm", "forearm", "gripper"):
            mp = self.link_mass[link]
            masses.append((link, mp.mass, fk.frames[link].apply(mp.com)))
        if self.tool == "pen":
            masses.append(("gripper", self.pen_mass, fk.frames["gripper"].apply(self.pen_com)))
        if payload_kg > 0:
            masses.append(("gripper", payload_kg, fk.tcp))
        distal = {
            "base_yaw": {"turret", "upper_arm", "forearm", "gripper"},
            "shoulder": {"upper_arm", "forearm", "gripper"},
            "elbow": {"forearm", "gripper"},
            "wrist": {"gripper"},
        }
        out: dict[str, float] = {}
        for jn in ARM_JOINTS:
            p = fk.joint_points[jn]
            axis = fk.joint_axes[jn]
            tau = 0.0
            for link, m, com in masses:
                if link in distal[jn]:
                    torque = _cross(_sub(com, p), (0.0, 0.0, -m * g))
                    tau += _dot(torque, axis)
            # Holding torque is the negative of the gravity torque.
            out[jn] = -tau
        return out

    def total_mass(self) -> float:
        return sum(mp.mass for mp in self.link_mass.values()) + (self.pen_mass if self.tool == "pen" else 0.0)


def home_pose(specs: ArmSpecs) -> dict[str, float]:
    return {j.name: j.home for j in specs.joints}


def servo_max_speed(specs: ArmSpecs, joint: str) -> float:
    return specs.servo_for(joint).no_load_speed

