"""Robot tools for the Abiyss runtime (QQ / AST).

The model never moves the arm. It calls ``robot.enqueue`` with *intent*
(validated actions); the robot service turns intent into physics. All robot
tools are Aquery tools: every model function call that reaches the runtime is
an Aquery (see docs/ARCHITECTURE.md).

    from abiyss.tools import build_default_registry
    from abiyss.robotics.runtime_tools import register_robot_tools
    from abiyss.robotics.server import RobotClient

    registry = build_default_registry(base_dir=...)
    register_robot_tools(registry, RobotClient(port=47012))
"""

from __future__ import annotations

from typing import Any, Protocol

from ..models import QueryType, ToolResult, ToolSpec
from ..tools import PythonTool, ToolRegistry
from .actions import ACTION_DESCRIPTIONS, ACTION_SCHEMAS, validate_action


class RobotLink(Protocol):
    def status(self) -> Any: ...
    def telemetry(self) -> Any: ...
    def stall_check(self) -> Any: ...
    def enqueue(self, actions: list[dict[str, Any]], source: str = ...) -> Any: ...
    def emergency_stop(self, reason: str = ...) -> Any: ...
    def reset(self) -> Any: ...
    def frame(self, path: str | None = None) -> Any: ...


def _actions_schema() -> dict[str, Any]:
    return {
        "type": "object",
        "properties": {
            "actions": {"type": "array", "minItems": 1, "maxItems": 32, "items": {"type": "object"}},
        },
        "required": ["actions"],
        "additionalProperties": False,
    }


def _empty() -> dict[str, Any]:
    return {"type": "object", "properties": {}, "additionalProperties": False}


def robot_tool_specs() -> list[ToolSpec]:
    vocab = "; ".join(f"{k}: {v}" for k, v in ACTION_DESCRIPTIONS.items())
    return [
        ToolSpec("robot.enqueue", "Queue robot actions (intent). Actions: " + vocab, QueryType.AQUERY, _actions_schema(), reversible=False),
        ToolSpec("robot.status", "Robot safety state, active action, queue, joint angles (deg), TCP.", QueryType.AQUERY, _empty()),
        ToolSpec("robot.telemetry", "Per-joint target/position/error/velocity/torque/current and gripper/pen state.", QueryType.AQUERY, _empty()),
        ToolSpec("robot.stall_check", "Requested vs available vs maximum torque per servo and stall history.", QueryType.AQUERY, _empty()),
        ToolSpec("robot.camera_frame", "Capture cam.frame() from the end-effector camera; returns metadata and the saved path.", QueryType.AQUERY,
                 {"type": "object", "properties": {"path": {"type": "string", "maxLength": 4096}}, "additionalProperties": False}),
        ToolSpec("robot.emergency_stop", "Latch EMERGENCY_STOP: clear the queue and hold. Requires robot.reset.", QueryType.AQUERY,
                 {"type": "object", "properties": {"reason": {"type": "string", "maxLength": 200}}, "additionalProperties": False}, reversible=False),
        ToolSpec("robot.reset", "Clear a latched STALL/FAULT/EMERGENCY_STOP (refused while a servo is still stalled).", QueryType.AQUERY, _empty(), reversible=False),
    ]


def register_robot_tools(registry: ToolRegistry, link: RobotLink) -> list[str]:
    specs = {s.name: s for s in robot_tool_specs()}

    def enqueue(args: dict[str, Any]) -> ToolResult:
        actions = [validate_action(a) for a in args["actions"]]
        return ToolResult("ok", link.enqueue(actions, source="abiyss_runtime"))

    table = {
        "robot.enqueue": enqueue,
        "robot.status": lambda _a: ToolResult("ok", link.status()),
        "robot.telemetry": lambda _a: ToolResult("ok", link.telemetry()),
        "robot.stall_check": lambda _a: ToolResult("ok", link.stall_check()),
        "robot.camera_frame": lambda a: ToolResult("ok", link.frame(a.get("path"))),
        "robot.emergency_stop": lambda a: ToolResult("ok", link.emergency_stop(a.get("reason", "abiyss runtime"))),
        "robot.reset": lambda _a: ToolResult("ok", link.reset()),
    }
    for name, fn in table.items():
        registry.register(PythonTool(specs[name], fn))
    return list(table)


__all__ = ["ACTION_SCHEMAS", "RobotLink", "register_robot_tools", "robot_tool_specs"]
