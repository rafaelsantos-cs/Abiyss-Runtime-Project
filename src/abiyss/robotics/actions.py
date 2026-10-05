"""Action vocabulary of the agent (what the Abiyss runtime may ask for).

An action is intent, not motion: ``{"action": "move_to", "x": 0.15, ...}``.
The controller turns it into trajectories, IK, servo pulses and physics.
All coordinates are metres in the robot frame; angles are degrees.
"""

from __future__ import annotations

from copy import deepcopy
from typing import Any

from ..errors import ValidationError
from ..schema import validate

JOINT_ENUM = ["base_yaw", "shoulder", "elbow", "wrist", "gripper"]
_coord = {"type": "number", "minimum": -0.4, "maximum": 0.4}
_z = {"type": "number", "minimum": -0.02, "maximum": 0.45}
_speed = {"type": "number", "minimum": 0.02, "maximum": 1.0}
_pitch = {"type": "number", "minimum": -180.0, "maximum": 180.0}


def _obj(props: dict[str, Any], required: list[str] | None = None) -> dict[str, Any]:
    return {
        "type": "object",
        "properties": {"action": {"type": "string", "maxLength": 32}, **props},
        "required": ["action", *(required or [])],
        "additionalProperties": False,
    }


ACTION_SCHEMAS: dict[str, dict[str, Any]] = {
    "move_to": _obj({"x": _coord, "y": _coord, "z": _z, "pitch_deg": _pitch, "speed": _speed, "settle_s": {"type": "number", "minimum": 0.0, "maximum": 5.0}}, ["x", "y", "z"]),
    "move_line": _obj({"x": _coord, "y": _coord, "z": _z, "pitch_deg": _pitch, "speed_mps": {"type": "number", "minimum": 0.002, "maximum": 0.2}}, ["x", "y", "z"]),
    "move_joint": _obj({"joint": {"type": "string", "enum": JOINT_ENUM}, "angle_deg": {"type": "number", "minimum": -200.0, "maximum": 200.0}, "speed": _speed, "settle_s": {"type": "number", "minimum": 0.0, "maximum": 5.0}}, ["joint", "angle_deg"]),
    "move_joints": _obj({"angles_deg": {"type": "object", "properties": {j: {"type": "number", "minimum": -200.0, "maximum": 200.0} for j in JOINT_ENUM}, "additionalProperties": False}, "speed": _speed}, ["angles_deg"]),
    "home": _obj({"speed": _speed}),
    "gripper_open": _obj({"width_mm": {"type": "number", "minimum": 0.0, "maximum": 60.0}}),
    "gripper_close": _obj({"width_mm": {"type": "number", "minimum": 0.0, "maximum": 60.0}, "hold_s": {"type": "number", "minimum": 0.0, "maximum": 5.0}}),
    "write": _obj({
        "text": {"type": "string", "maxLength": 32},
        "origin": {"type": "array", "items": _coord, "minItems": 2, "maxItems": 2},
        "letter_height": {"type": "number", "minimum": 0.008, "maximum": 0.08},
        "letter_width": {"type": "number", "minimum": 0.005, "maximum": 0.08},
        "spacing": {"type": "number", "minimum": 0.0, "maximum": 0.05},
        "speed_mps": {"type": "number", "minimum": 0.002, "maximum": 0.1},
    }, ["text"]),
    "pick": _obj({"x": _coord, "y": _coord, "z": _z, "approach_height": {"type": "number", "minimum": 0.01, "maximum": 0.2}, "label": {"type": "string", "maxLength": 32}}, ["x", "y"]),
    "place": _obj({"x": _coord, "y": _coord, "z": _z, "approach_height": {"type": "number", "minimum": 0.01, "maximum": 0.2}, "open_width_mm": {"type": "number", "minimum": 0.0, "maximum": 60.0}}, ["x", "y"]),
    "wait": _obj({"seconds": {"type": "number", "minimum": 0.0, "maximum": 60.0}}, ["seconds"]),
    "servo_power": _obj({"joint": {"type": "string", "enum": JOINT_ENUM}, "on": {"type": "boolean"}}, ["joint", "on"]),
    "set_pid": _obj({"joint": {"type": "string", "enum": JOINT_ENUM}, "kp": {"type": "number", "minimum": 0.0, "maximum": 50.0}, "ki": {"type": "number", "minimum": 0.0, "maximum": 500.0}, "kd": {"type": "number", "minimum": 0.0, "maximum": 5.0}}, ["joint"]),
    "step_response": _obj({"joint": {"type": "string", "enum": JOINT_ENUM}, "delta_deg": {"type": "number", "minimum": -60.0, "maximum": 60.0}, "duration_s": {"type": "number", "minimum": 0.2, "maximum": 10.0}}, ["joint", "delta_deg"]),
}

ACTION_DESCRIPTIONS = {
    "move_to": "Move the tool centre point to (x, y, z) metres with tool pitch (deg, default -90 = pointing down) through IK and a joint-space min-jerk trajectory.",
    "move_line": "Move the TCP along a straight Cartesian line at speed_mps.",
    "move_joint": "Move one joint to angle_deg.",
    "move_joints": "Move several joints together.",
    "home": "Return to the home pose from ARM_SPECS.json.",
    "gripper_open": "Open the gripper (optionally to width_mm).",
    "gripper_close": "Close the gripper (optionally to width_mm) and hold for hold_s.",
    "write": "Write text with the pen tool on the paper.",
    "pick": "Top-down grasp at (x, y, z): open, approach, descend, close, lift.",
    "place": "Top-down place at (x, y, z): approach, descend, open, lift.",
    "wait": "Do nothing for seconds of simulated time (physics keeps running).",
    "servo_power": "Power a servo on or off. Off = the joint is held only by gear friction.",
    "set_pid": "Change the simulated servo controller gains (simulation backend only).",
    "step_response": "Apply a step of delta_deg to one joint and measure overshoot, settling time and oscillation.",
}


def validate_action(action: Any) -> dict[str, Any]:
    if not isinstance(action, dict):
        raise ValidationError("action must be an object")
    name = action.get("action")
    if name not in ACTION_SCHEMAS:
        raise ValidationError(f"unknown action: {name!r}")
    validate(action, ACTION_SCHEMAS[name])
    return deepcopy(action)


def queue_schema() -> dict[str, Any]:
    """JSON schema of a queue document: ``{"queue": [action, ...]}``."""
    return {"type": "object", "properties": {"queue": {"type": "array", "maxItems": 256}}, "required": ["queue"]}
