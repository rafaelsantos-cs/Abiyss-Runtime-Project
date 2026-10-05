"""Action queue executed by the robot controller.

Actions are validated JSON objects (see :mod:`abiyss.robotics.actions`).
Nothing is executed on ``submit``: the controller pulls one action at a time
and runs it through the physical simulation (or the hardware) tick by tick.
"""

from __future__ import annotations

import itertools
from collections import deque
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from .actions import validate_action


class ActionStatus(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    DONE = "done"
    FAILED = "failed"
    CANCELLED = "cancelled"


@dataclass(slots=True)
class ActionRecord:
    id: int
    action: str
    params: dict[str, Any]
    status: ActionStatus = ActionStatus.PENDING
    submitted_at: float = 0.0
    started_at: float | None = None
    finished_at: float | None = None
    result: dict[str, Any] = field(default_factory=dict)
    error: str | None = None

    def summary(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "action": self.action,
            "params": self.params,
            "status": self.status.value,
            "submitted_at": self.submitted_at,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "result": self.result,
            "error": self.error,
        }

    def label(self) -> str:
        args = ", ".join(f"{k}={v!r}" for k, v in self.params.items() if k != "action")
        return f"#{self.id} {self.action}({args})"


class ActionQueue:
    MAX_PENDING = 256

    def __init__(self) -> None:
        self._pending: deque[ActionRecord] = deque()
        self._ids = itertools.count(1)
        self.current: ActionRecord | None = None
        self.history: list[ActionRecord] = []

    def submit(self, action: dict[str, Any], sim_time: float = 0.0) -> ActionRecord:
        clean = validate_action(action)
        if len(self._pending) >= self.MAX_PENDING:
            raise OverflowError("action queue is full")
        rec = ActionRecord(next(self._ids), clean["action"], {k: v for k, v in clean.items() if k != "action"}, submitted_at=sim_time)
        self._pending.append(rec)
        return rec

    def pop_next(self, sim_time: float) -> ActionRecord | None:
        if self.current is not None or not self._pending:
            return None
        rec = self._pending.popleft()
        rec.status = ActionStatus.RUNNING
        rec.started_at = sim_time
        self.current = rec
        return rec

    def finish(self, status: ActionStatus, sim_time: float, *, result: dict[str, Any] | None = None, error: str | None = None) -> ActionRecord | None:
        rec = self.current
        if rec is None:
            return None
        rec.status = status
        rec.finished_at = sim_time
        rec.result = result or {}
        rec.error = error
        self.history.append(rec)
        self.current = None
        return rec

    def cancel_all(self, sim_time: float, reason: str) -> list[ActionRecord]:
        cancelled: list[ActionRecord] = []
        if self.current is not None:
            cancelled.append(self.finish(ActionStatus.CANCELLED, sim_time, error=reason))  # type: ignore[arg-type]
        while self._pending:
            rec = self._pending.popleft()
            rec.status = ActionStatus.CANCELLED
            rec.finished_at = sim_time
            rec.error = reason
            self.history.append(rec)
            cancelled.append(rec)
        return cancelled

    @property
    def pending(self) -> list[ActionRecord]:
        return list(self._pending)

    def idle(self) -> bool:
        return self.current is None and not self._pending

    def snapshot(self) -> dict[str, Any]:
        return {
            "current": self.current.summary() if self.current else None,
            "pending": [r.summary() for r in self._pending],
            "history": [r.summary() for r in self.history[-20:]],
        }
