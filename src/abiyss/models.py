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
            encoded = json.dumps(
                self.payload,
                ensure_ascii=False,
                separators=(",", ":"),
                sort_keys=True,
                allow_nan=False,
            ).encode("utf-8")
        except (TypeError, ValueError) as exc:
            raise ValidationError("query payload is not JSON-safe") from exc
        if len(encoded) > MAX_QUERY_PAYLOAD_BYTES:
            raise ValidationError("query payload too large")
        if self.created_at <= 0 or not math.isfinite(self.created_at):
            raise ValidationError("invalid created_at")
        if self.updated_at <= 0 or not math.isfinite(self.updated_at):
            raise ValidationError("invalid updated_at")
        self.updated_at = max(self.updated_at, self.created_at)
        for label, value, limit in (
            ("parent interaction id", self.parent_interaction_id, 256),
            ("tool call id", self.tool_call_id, 256),
        ):
            if value is not None and (
                not isinstance(value, str) or not value or len(value.encode("utf-8")) > limit or "\x00" in value
            ):
                raise ValidationError(f"invalid {label}")
        if not isinstance(self.source, str) or not 1 <= len(self.source.encode("utf-8")) <= 128:
            raise ValidationError("invalid query source")
        if not isinstance(self.attempts, int) or isinstance(self.attempts, bool) or self.attempts < 0:
            raise ValidationError("invalid query attempts")
        if self.checkpoint.state != self.state:
            raise ValidationError("query state does not match checkpoint state")
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
        checkpoint = Checkpoint()
        return cls(
            id=f"q_{uuid.uuid4().hex}",
            type=query_type,
            priority=priority,
            payload=payload,
            source=source,
            parent_interaction_id=parent_interaction_id,
            tool_call_id=tool_call_id,
            checkpoint=checkpoint,
        )

    def _validate_checkpoint(self) -> None:
        if self.checkpoint.schema_version != 1:
            raise ValidationError("unsupported checkpoint schema")
        if not isinstance(self.checkpoint.step_index, int) or isinstance(self.checkpoint.step_index, bool) or not 0 <= self.checkpoint.step_index <= MAX_QUERY_STEPS:
            raise ValidationError("invalid checkpoint step index")
        if not isinstance(self.checkpoint.attempt, int) or isinstance(self.checkpoint.attempt, bool) or self.checkpoint.attempt < 0:
            raise ValidationError("invalid checkpoint attempt")
        if self.checkpoint.attempt != self.attempts:
            raise ValidationError("query attempts do not match checkpoint attempt")
        if self.checkpoint.tool_name is not None and (
            not isinstance(self.checkpoint.tool_name, str) or not 1 <= len(self.checkpoint.tool_name.encode("utf-8")) <= 256
        ):
            raise ValidationError("invalid checkpoint tool name")
        if self.checkpoint.tool_call_id is not None and (
            not isinstance(self.checkpoint.tool_call_id, str) or not 1 <= len(self.checkpoint.tool_call_id.encode("utf-8")) <= 256
        ):
            raise ValidationError("invalid checkpoint tool call id")
        if self.checkpoint.result is not None and not isinstance(self.checkpoint.result, dict):
            raise ValidationError("invalid checkpoint result")
        if self.checkpoint.error is not None and not isinstance(self.checkpoint.error, str):
            raise ValidationError("invalid checkpoint error")
        if self.checkpoint.execution_started_at is not None and (
            not isinstance(self.checkpoint.execution_started_at, (int, float))
            or isinstance(self.checkpoint.execution_started_at, bool)
            or not math.isfinite(self.checkpoint.execution_started_at)
            or self.checkpoint.execution_started_at <= 0
        ):
            raise ValidationError("invalid checkpoint execution timestamp")
        if not isinstance(self.checkpoint.updated_at, (int, float)) or isinstance(self.checkpoint.updated_at, bool):
            raise ValidationError("invalid checkpoint updated_at")
        if not math.isfinite(self.checkpoint.updated_at) or self.checkpoint.updated_at <= 0:
            raise ValidationError("invalid checkpoint updated_at")

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
        allowed_fields = {
            "id",
            "type",
            "priority",
            "payload",
            "created_at",
            "updated_at",
            "state",
            "checkpoint",
            "parent_interaction_id",
            "tool_call_id",
            "source",
            "attempts",
        }
        if set(data) != allowed_fields:
            unknown = sorted(set(data) - allowed_fields)
            missing = sorted(allowed_fields - set(data))
            details: list[str] = []
            if unknown:
                details.append(f"unknown fields: {unknown}")
            if missing:
                details.append(f"missing fields: {missing}")
            raise ValidationError("invalid query snapshot fields; " + "; ".join(details))
        if not isinstance(data["id"], str) or not isinstance(data["type"], str) or not isinstance(data["state"], str):
            raise ValidationError("invalid query snapshot scalar field")
        if not isinstance(data["priority"], int) or isinstance(data["priority"], bool):
            raise ValidationError("invalid query priority")
        if not isinstance(data["payload"], dict):
            raise ValidationError("query payload must be an object")
        checkpoint_raw = data["checkpoint"]
        if not isinstance(checkpoint_raw, dict):
            raise ValidationError("checkpoint must be an object")
        checkpoint_fields = {
            "schema_version",
            "step_index",
            "state",
            "tool_name",
            "tool_call_id",
            "attempt",
            "result",
            "error",
            "execution_started_at",
            "updated_at",
        }
        if set(checkpoint_raw) != checkpoint_fields:
            unknown = sorted(set(checkpoint_raw) - checkpoint_fields)
            missing = sorted(checkpoint_fields - set(checkpoint_raw))
            details: list[str] = []
            if unknown:
                details.append(f"unknown fields: {unknown}")
            if missing:
                details.append(f"missing fields: {missing}")
            raise ValidationError("invalid checkpoint fields; " + "; ".join(details))
        try:
            checkpoint_state = QueryState(checkpoint_raw["state"])
            state = QueryState(data["state"])
            query_type = QueryType(data["type"])
        except (ValueError, TypeError) as exc:
            raise ValidationError("invalid query enum value") from exc
        if checkpoint_state is not state:
            raise ValidationError("query state does not match checkpoint state")
        for key in ("created_at", "updated_at"):
            if not isinstance(data[key], (int, float)) or isinstance(data[key], bool):
                raise ValidationError(f"invalid {key}")
            if not math.isfinite(data[key]) or data[key] <= 0:
                raise ValidationError(f"invalid {key}")
        attempts = data["attempts"]
        if not isinstance(attempts, int) or isinstance(attempts, bool) or attempts < 0:
            raise ValidationError("invalid query attempts")
        checkpoint_attempt = checkpoint_raw["attempt"]
        if not isinstance(checkpoint_attempt, int) or isinstance(checkpoint_attempt, bool) or checkpoint_attempt < 0:
            raise ValidationError("invalid checkpoint attempt")
        if checkpoint_attempt != attempts:
            raise ValidationError("query attempts do not match checkpoint attempt")
        for optional in ("parent_interaction_id", "tool_call_id"):
            value = data[optional]
            if value is not None and not isinstance(value, str):
                raise ValidationError(f"invalid {optional}")
        if not isinstance(data["source"], str):
            raise ValidationError("invalid query source")
        try:
            checkpoint = Checkpoint(**checkpoint_raw)
            return cls(
                id=data["id"],
                type=query_type,
                priority=data["priority"],
                payload=data["payload"],
                created_at=data["created_at"],
                updated_at=data["updated_at"],
                state=state,
                checkpoint=checkpoint,
                parent_interaction_id=data["parent_interaction_id"],
                tool_call_id=data["tool_call_id"],
                source=data["source"],
                attempts=attempts,
            )
        except (KeyError, TypeError, ValueError) as exc:
            raise ValidationError(f"invalid query snapshot: {exc}") from exc


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
