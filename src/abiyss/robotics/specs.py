"""ARM_SPECS.json loading.

The specs file is the single source of truth for geometry, servo parameters
and test fixtures. Both the Python Robot API and the Godot plant read it.
Every physical parameter is an object ``{"value": ..., "unit": ..., "status":
..., "sources": [...]}``; :func:`value` unwraps it.
"""

from __future__ import annotations

import json
import math
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from ..errors import ValidationError

VALID_STATUSES = {
    "datasheet",
    "manufacturer_web",
    "datasheet_other_manufacturer",
    "third_party",
    "derived",
    "design",
    "reference",
    "estimated",
    "tuned_in_simulation",
    "unknown",
}


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def default_specs_path() -> Path:
    """$ABIYSS_ARM_SPECS, else the repository copy, else ./ARM_SPECS.json."""
    env = os.environ.get("ABIYSS_ARM_SPECS")
    if env:
        return Path(env)
    repo = repo_root() / "ARM_SPECS.json"
    return repo if repo.exists() else Path.cwd() / "ARM_SPECS.json"


def value(item: Any) -> Any:
    """Unwrap ``{"value": x, ...}`` parameter objects; pass plain values through."""
    if isinstance(item, dict) and "value" in item:
        return item["value"]
    return item


@dataclass(frozen=True, slots=True)
class ServoSpec:
    model: str
    stall_torque: float          # N*m at the operating voltage
    no_load_speed: float         # rad/s
    deadband_us: float
    pulse_neutral_us: float
    pulse_per_90_us: float
    pwm_period: float
    stall_current: float
    mass: float
    kp: float
    ki: float
    kd: float
    raw: dict[str, Any] = field(repr=False, compare=False)

    def deadband_rad(self) -> float:
        return self.deadband_us / self.pulse_per_90_us * (math.pi / 2.0)


@dataclass(frozen=True, slots=True)
class JointSpec:
    name: str
    servo: str
    parent: str
    child: str
    origin: tuple[float, float, float]
    axis: tuple[float, float, float]
    center: float                # rad, joint angle at 1500 us
    direction: float
    soft_limits: tuple[float, float]   # rad
    hard_stops: tuple[float, float]    # rad
    home: float                  # rad


@dataclass(frozen=True, slots=True)
class GripperSpec:
    servo: str
    closed: float                # rad (servo angle at closed jaws)
    center: float                # rad (servo angle at 1500 us)
    soft_limits: tuple[float, float]
    home: float
    pinion_radius: float
    finger_travel: float
    closed_half_gap: float

    def opening_for(self, servo_angle: float) -> float:
        travel = min(max((servo_angle - self.closed) * self.pinion_radius, 0.0), self.finger_travel)
        return 2.0 * (self.closed_half_gap + travel)

    def angle_for_opening(self, opening: float) -> float:
        travel = max(opening / 2.0 - self.closed_half_gap, 0.0)
        return self.closed + travel / self.pinion_radius


@dataclass(slots=True)
class ArmSpecs:
    raw: dict[str, Any]
    path: Path
    servos: dict[str, ServoSpec]
    joints: list[JointSpec]
    gripper: GripperSpec

    @property
    def joint_names(self) -> list[str]:
        return [j.name for j in self.joints]

    def joint(self, name: str) -> JointSpec:
        for j in self.joints:
            if j.name == name:
                return j
        raise ValidationError(f"unknown joint: {name}")

    @property
    def control_rate_hz(self) -> float:
        return float(value(self.raw["simulation"]["control_rate_hz"]))

    @property
    def physics_rate_hz(self) -> float:
        return float(value(self.raw["simulation"]["physics_rate_hz"]))

    @property
    def pwm_resolution_us(self) -> float:
        return float(value(self.raw["simulation"]["pwm_resolution_us"]))

    @property
    def gravity(self) -> float:
        return float(value(self.raw["simulation"]["gravity"]))

    @property
    def default_seed(self) -> int:
        return int(value(self.raw["simulation"]["default_seed"]))

    @property
    def safety(self) -> dict[str, Any]:
        return self.raw["safety"]

    @property
    def workspace(self) -> dict[str, Any]:
        return self.raw["workspace"]

    def tcp_offset(self, tool: str) -> tuple[float, float, float]:
        link = self.raw["arm"]["links"]["gripper"]
        off = link["pen_tcp_offset"] if tool == "pen" else link["tcp_offset"]
        return (float(off[0]), float(off[1]), float(off[2]))

    def servo_for(self, joint_name: str) -> ServoSpec:
        if joint_name == "gripper":
            return self.servos[self.gripper.servo]
        return self.servos[self.joint(joint_name).servo]


def _servo_spec(model: str, d: dict[str, Any]) -> ServoSpec:
    p = d["parameters"]
    c = d["controller"]
    return ServoSpec(
        model=model,
        stall_torque=float(value(p["stall_torque_4v8"])),
        no_load_speed=float(value(p["no_load_speed_4v8"])),
        deadband_us=float(value(p["dead_band_width"])),
        pulse_neutral_us=float(value(p["pulse_neutral"])),
        pulse_per_90_us=float(value(p["pulse_per_90deg"])),
        pwm_period=float(value(p["pwm_period"])),
        stall_current=float(value(p["stall_current_4v8"])),
        mass=float(value(p["mass"])),
        kp=float(value(c["kp"])),
        ki=float(value(c["ki"])),
        kd=float(value(c["kd"])),
        raw=d,
    )


def _deg2(pair: list[Any]) -> tuple[float, float]:
    return (math.radians(float(pair[0])), math.radians(float(pair[1])))


def parse_specs(raw: dict[str, Any], path: Path | None = None) -> ArmSpecs:
    if raw.get("schema") != "abiyss.arm_specs/1":
        raise ValidationError("unsupported ARM_SPECS schema")
    servos = {model: _servo_spec(model, d) for model, d in raw["servo_models"].items()}
    joints: list[JointSpec] = []
    for js in raw["arm"]["joints"]:
        if js["servo"] not in servos:
            raise ValidationError(f"joint {js['name']} uses unknown servo {js['servo']}")
        joints.append(
            JointSpec(
                name=js["name"],
                servo=js["servo"],
                parent=js["parent"],
                child=js["child"],
                origin=tuple(float(v) for v in js["origin"]),  # type: ignore[arg-type]
                axis=tuple(float(v) for v in js["axis"]),  # type: ignore[arg-type]
                center=math.radians(float(js["servo_center_deg"])),
                direction=float(js["direction"]),
                soft_limits=_deg2(js["soft_limits_deg"]),
                hard_stops=_deg2(js["hard_stops_deg"]),
                home=math.radians(float(js["home_deg"])),
            )
        )
    g = raw["arm"]["gripper"]
    gripper = GripperSpec(
        servo=g["servo"],
        closed=math.radians(float(g["servo_closed_deg"])),
        center=math.radians(float(g["servo_center_deg"])),
        soft_limits=_deg2(g["soft_limits_deg"]),
        home=math.radians(float(g["home_deg"])),
        pinion_radius=float(value(g["pinion_radius"])),
        finger_travel=float(value(g["finger_travel"])),
        closed_half_gap=float(value(g["closed_half_gap"])),
    )
    return ArmSpecs(raw=raw, path=path or default_specs_path(), servos=servos, joints=joints, gripper=gripper)


def load_specs(path: str | Path | None = None) -> ArmSpecs:
    p = Path(path) if path is not None else default_specs_path()
    try:
        raw = json.loads(p.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValidationError(f"cannot load arm specs {p}: {exc}") from exc
    return parse_specs(raw, p)


def dumps_specs(raw: dict[str, Any]) -> str:
    """Stable, review-friendly JSON: 2-space indent, short scalar lists inline."""

    def enc(node: Any, indent: int) -> str:
        pad = "  " * indent
        if isinstance(node, dict):
            if not node:
                return "{}"
            flat = json.dumps(node, ensure_ascii=False, separators=(", ", ": "))
            if len(flat) + len(pad) <= 150 and not any(isinstance(v, dict) for v in node.values()):
                return flat
            items = [f'{pad}  {json.dumps(k, ensure_ascii=False)}: {enc(v, indent + 1)}' for k, v in node.items()]
            return "{\n" + ",\n".join(items) + "\n" + pad + "}"
        if isinstance(node, list):
            if all(not isinstance(v, (dict, list)) for v in node) and len(node) <= 8:
                return "[" + ", ".join(json.dumps(v, ensure_ascii=False) for v in node) + "]"
            if all(isinstance(v, list) and all(not isinstance(x, (dict, list)) for x in v) for v in node):
                return "[" + ", ".join(enc(v, indent + 1) for v in node) + "]"
            items = [f"{pad}  {enc(v, indent + 1)}" for v in node]
            return "[\n" + ",\n".join(items) + "\n" + pad + "]"
        return json.dumps(node, ensure_ascii=False)

    return enc(raw, 0) + "\n"


def audit_parameters(raw: dict[str, Any]) -> list[dict[str, Any]]:
    """List every parameter object with its verification status.

    Used by tests and ``abiyss-robotics specs`` to prove that no physical value
    is present without a status (and sources where the status requires them).
    """
    rows: list[dict[str, Any]] = []

    def walk(node: Any, path: str) -> None:
        if isinstance(node, dict):
            if "value" in node and "status" in node:
                rows.append({"path": path, "value": node["value"], "unit": node.get("unit"), "status": node["status"], "sources": node.get("sources", [])})
                return
            for k, v in node.items():
                walk(v, f"{path}.{k}" if path else k)
        elif isinstance(node, list):
            for i, v in enumerate(node):
                walk(v, f"{path}[{i}]")

    walk(raw, "")
    return rows
