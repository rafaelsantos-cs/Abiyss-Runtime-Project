from __future__ import annotations

import json
import math
import time
import uuid
from dataclasses import asdict, dataclass, field
from enum import Enum
from typing import Any

from .errors import ValidationError
from .security import validate_identifier

MAX_QUERY_PAYLOAD_BYTES = 256 * 1024
MAX_QUERY_STEPS = 256
MAX_TEXT_BYTES = 128 * 1024


class QueryType(str, Enum):
    AQUERY = "Aquery"
    SQUERY = "Squery"


class QueryState(str, Enum):
    QUEUED = "queued"
    RUNNING = "running"
    PAUSED = "paused"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"
    RECOVERY_REQUIRED = "recovery_required"


@dataclass(slots=True)
class Checkpoint:
    schema_version: int = 1
    step_index: int = 0
    state: QueryState = QueryState.QUEUED
    tool_name: str | None = None
    tool_call_id: str | None = None
    attempt: int = 0
    result: dict[str, Any] | None = None
    error: str | None = None
    execution_started_at: float | None = None
    updated_at: float = field(default_factory=time.time)


@dataclass(slots=True)
class Query:
    id: str
    type: QueryType
    priority: int
    payload: dict[str, Any]
    created_at: float = field(default_factory=time.time)
    updated_at: float = field(default_factory=time.time)
    state: QueryState = QueryState.QUEUED
    checkpoint: Checkpoint = field(default_factory=Checkpoint)
    parent_interaction_id: str | None = None
    tool_call_id: str | None = None
    source: str = "runtime"
    attempts: int = 0

    def __post_init__(self) -> None:
        try:
            validate_identifier(self.id, label="query id")
        except Exception as exc:
            raise ValidationError(str(exc)) from exc
        if not isinstance(self.priority, int) or isinstance(self.priority, bool) or not -100_000 <= self.priority <= 100_000:
            raise ValidationError("invalid priority")
        if not isinstance(self.payload, dict):
            raise ValidationError("query payload must be an object")
        try:
            encoded = json.dumps(self.payload, ensure_ascii=False, separators=(",", ":"), sort_keys=True, allow_nan=False).encode("utf-8")
        except (TypeError, ValueError) as exc:
            raise ValidationError("query payload is not JSON-safe") from exc
        if len(encoded) > MAX_QUERY_PAYLOAD_BYTES:
            raise ValidationError("query payload too large")
        if not math.isfinite(float(self.created_at)) or not math.isfinite(float(self.updated_at)):
            raise ValidationError("invalid query timestamp")
        if self.created_at <= 0 or self.updated_at <= 0:
            raise ValidationError("invalid query timestamp")
        self.updated_at = max(self.updated_at, self.created_at)
        self.checkpoint.state = self.state
        self._validate_checkpoint()

    @classmethod
    def new(
        cls,
        *,
        query_type: QueryType,
        priority: int,
        payload: dict[str, Any],
        source: str = "runtime",
        parent_interaction_id: str | None = None,
        tool_call_id: str | None = None,
    ) -> "Query":
        return cls(
            id=f"q_{uuid.uuid4().hex}",
            type=query_type,
            priority=priority,
            payload=payload,
            source=source,
            parent_interaction_id=parent_interaction_id,
            tool_call_id=tool_call_id,
        )

    def _validate_checkpoint(self) -> None:
        if self.checkpoint.schema_version != 1:
            raise ValidationError("unsupported checkpoint schema")
        if not isinstance(self.checkpoint.step_index, int) or not 0 <= self.checkpoint.step_index <= MAX_QUERY_STEPS:
            raise ValidationError("invalid checkpoint step index")
        if not isinstance(self.checkpoint.attempt, int) or self.checkpoint.attempt < 0:
            raise ValidationError("invalid checkpoint attempt")

    def transition(
        self,
        new_state: QueryState,
        *,
        error: Any = None,
        result: dict[str, Any] | None = None,
        tool_name: str | None = None,
        execution_started_at: float | None = None,
    ) -> None:
        allowed = {
            QueryState.QUEUED: {QueryState.RUNNING, QueryState.CANCELLED},
            QueryState.RUNNING: {
                QueryState.PAUSED,
                QueryState.QUEUED,
                QueryState.COMPLETED,
                QueryState.FAILED,
                QueryState.RECOVERY_REQUIRED,
            },
            QueryState.PAUSED: {QueryState.QUEUED, QueryState.CANCELLED},
            QueryState.RECOVERY_REQUIRED: {QueryState.QUEUED, QueryState.CANCELLED},
            QueryState.COMPLETED: set(),
            QueryState.FAILED: set(),
            QueryState.CANCELLED: set(),
        }
        if new_state not in allowed[self.state]:
            raise ValidationError(f"invalid transition {self.state.value}->{new_state.value}")
        self.state = new_state
        self.updated_at = time.time()
        self.checkpoint.state = new_state
        self.checkpoint.updated_at = self.updated_at
        if error is not None:
            self.checkpoint.error = str(error)[:4096]
        if result is not None:
            self.checkpoint.result = result
        if tool_name is not None:
            self.checkpoint.tool_name = tool_name
        if execution_started_at is not None:
            self.checkpoint.execution_started_at = execution_started_at

    def snapshot(self) -> dict[str, Any]:
        data = asdict(self)
        data["type"] = self.type.value
        data["state"] = self.state.value
        data["checkpoint"]["state"] = self.checkpoint.state.value
        return data

    @classmethod
    def from_snapshot(cls, data: dict[str, Any]) -> "Query":
        if not isinstance(data, dict):
            raise ValidationError("query snapshot must be an object")
        checkpoint_data = dict(data.get("checkpoint") or {})
        checkpoint_data["state"] = QueryState(checkpoint_data.get("state", data.get("state", "queued")))
        return cls(
            id=str(data["id"]),
            type=QueryType(data["type"]),
            priority=int(data["priority"]),
            payload=dict(data["payload"]),
            created_at=float(data.get("created_at", time.time())),
            updated_at=float(data.get("updated_at", time.time())),
            state=QueryState(data.get("state", "queued")),
            checkpoint=Checkpoint(**checkpoint_data),
            parent_interaction_id=data.get("parent_interaction_id"),
            tool_call_id=data.get("tool_call_id"),
            source=str(data.get("source", "runtime")),
            attempts=int(data.get("attempts", 0)),
        )


@dataclass(slots=True)
class ToolResult:
    status: str
    output: Any = None
    error: str | None = None
    evidence: list[str] = field(default_factory=list)
    sensitive: bool = False

    def __post_init__(self) -> None:
        if self.status not in {"ok", "error", "timeout", "denied", "recovery_required"}:
            self.status = "error"
        if self.error is not None:
            self.error = str(self.error)[:MAX_TEXT_BYTES]
        if isinstance(self.output, str) and len(self.output.encode("utf-8")) > MAX_TEXT_BYTES:
            self.output = self.output.encode("utf-8")[:MAX_TEXT_BYTES].decode("utf-8", errors="ignore") + "…<truncated>"
        self.evidence = [str(item)[:1024] for item in self.evidence[:32]]

    def as_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True, slots=True)
class ToolSpec:
    name: str
    description: str
    query_type: QueryType
    parameters: dict[str, Any]
    privileged: bool = False
    reversible: bool = True
    allow_memory_persistence: bool = True

    def declaration(self) -> dict[str, Any]:
        return {
            "type": "function",
            "name": self.name,
            "description": self.description,
            "parameters": self.parameters,
        }
