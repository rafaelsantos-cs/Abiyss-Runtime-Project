"""Queue documents: JSON or a one-call-per-line text form.

JSON::

    {"queue": [{"action": "write", "text": "OI"}, {"action": "home"}]}

Text (each line is a call with literal arguments; ``#`` starts a comment)::

    queue:
      write("OI")
      move_to(0.15, 0.0, 0.05)
      gripper.close()
      home()

Parsing uses :func:`ast.literal_eval` on the arguments only: no code runs.
"""

from __future__ import annotations

import ast
import json
from pathlib import Path
from typing import Any

from ..errors import ValidationError
from .actions import validate_action

# call name -> (action, positional parameter names)
CALLS: dict[str, tuple[str, tuple[str, ...]]] = {
    "write": ("write", ("text",)),
    "move_to": ("move_to", ("x", "y", "z", "pitch_deg")),
    "arm.move_to": ("move_to", ("x", "y", "z", "pitch_deg")),
    "move_line": ("move_line", ("x", "y", "z", "pitch_deg")),
    "move_joint": ("move_joint", ("joint", "angle_deg")),
    "arm.move_joint": ("move_joint", ("joint", "angle_deg")),
    "home": ("home", ()),
    "arm.home": ("home", ()),
    "open": ("gripper_open", ("width_mm",)),
    "gripper.open": ("gripper_open", ("width_mm",)),
    "close": ("gripper_close", ("width_mm",)),
    "gripper.close": ("gripper_close", ("width_mm",)),
    "pick": ("pick", ("x", "y", "z")),
    "place": ("place", ("x", "y", "z")),
    "wait": ("wait", ("seconds",)),
    "servo_off": ("servo_power", ("joint",)),
    "servo_on": ("servo_power", ("joint",)),
    "set_pid": ("set_pid", ("joint", "kp", "ki", "kd")),
    "step_response": ("step_response", ("joint", "delta_deg")),
}


def _call_name(node: ast.AST) -> str:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return f"{_call_name(node.value)}.{node.attr}"
    raise ValidationError("unsupported call target")


def parse_line(line: str) -> dict[str, Any]:
    try:
        tree = ast.parse(line.strip(), mode="eval")
    except SyntaxError as exc:
        raise ValidationError(f"cannot parse queue line {line!r}: {exc.msg}") from exc
    node = tree.body
    if not isinstance(node, ast.Call):
        raise ValidationError(f"queue line must be a call: {line!r}")
    name = _call_name(node.func)
    if name not in CALLS:
        raise ValidationError(f"unknown queue call {name!r}; known: {sorted(CALLS)}")
    action, positional = CALLS[name]
    if len(node.args) > len(positional):
        raise ValidationError(f"too many arguments for {name}")
    out: dict[str, Any] = {"action": action}
    try:
        for pname, arg in zip(positional, node.args):
            out[pname] = ast.literal_eval(arg)
        for kw in node.keywords:
            if kw.arg is None:
                raise ValidationError("**kwargs not allowed")
            out[kw.arg] = ast.literal_eval(kw.value)
    except ValueError as exc:
        raise ValidationError(f"arguments must be literals in {line!r}") from exc
    if name == "servo_off":
        out["on"] = False
    if name == "servo_on":
        out["on"] = True
    return validate_action(out)


def parse_text(text: str) -> list[dict[str, Any]]:
    actions: list[dict[str, Any]] = []
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line or line.rstrip(":") == "queue":
            continue
        if line.startswith("- "):
            line = line[2:].strip()
        actions.append(parse_line(line))
    return actions


def load_queue(source: str | Path) -> list[dict[str, Any]]:
    """Load a queue from a file path or from inline text."""
    p = Path(str(source))
    text = p.read_text(encoding="utf-8") if p.exists() else str(source)
    stripped = text.lstrip()
    if stripped.startswith("{") or stripped.startswith("["):
        doc = json.loads(text)
        items = doc["queue"] if isinstance(doc, dict) else doc
        if not isinstance(items, list):
            raise ValidationError("queue must be a list")
        return [validate_action(a) for a in items]
    return parse_text(text)
