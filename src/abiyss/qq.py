from __future__ import annotations

import heapq
import threading
import time
from dataclasses import dataclass
from typing import Callable

from .audit import AuditLog
from .errors import PersistenceError, QueueInvariantError, ToolDenied
from .models import Checkpoint, Query, QueryState, QueryType, ToolResult
from .qups import QuPsStore


@dataclass(frozen=True, slots=True)
class ExecutionOutcome:
    query_id: str
    state: QueryState
    step_index: int | None
    tool_result: ToolResult | None
    recovery_required: bool = False


class QueryQueue:
    """Single logical execution lane with Aquery priority and boundary-only Squery preemption."""

    def __init__(
        self,
        *,
        store: QuPsStore,
        audit: AuditLog,
        tool_execute: Callable[[QueryType, str, dict], ToolResult],
        tool_validator: Callable[[str, dict, QueryType], None] | None = None,
        max_queries: int = 10_000,
    ) -> None:
        self.store = store
        self.audit = audit
        self.tool_execute = tool_execute
        self.tool_validator = tool_validator
        self.max_queries = max_queries
        self._lock = threading.RLock()
        self._execution_lock = threading.Lock()
        self._heap: list[tuple[int, int, int, str]] = []
        self._sequence = 0
        self._queries: dict[str, Query] = {}
        self._external_calls: dict[tuple[str, str], str] = {}
        self._active: str | None = None
        self._preempt = threading.Event()

    @property
    def active_query_id(self) -> str | None:
        with self._lock:
            return self._active

    def _steps(self, query: Query) -> list[dict]:
        value = query.payload.get("steps")
        if value is None:
            name = query.payload.get("tool_name")
            args = query.payload.get("arguments", {})
            if not isinstance(name, str) or not isinstance(args, dict):
                raise QueueInvariantError("single-step Query has invalid tool call")
            if self.tool_validator:
                self.tool_validator(name, args, query.type)
            return [{"tool_name": name, "arguments": args}]
        if not isinstance(value, list) or not 1 <= len(value) <= 256:
            raise QueueInvariantError("invalid Query step count")
        steps: list[dict] = []
        for index, step in enumerate(value):
            if not isinstance(step, dict):
                raise QueueInvariantError(f"step {index} is not an object")
            name = step.get("tool_name")
            args = step.get("arguments", {})
            if not isinstance(name, str) or not isinstance(args, dict):
                raise QueueInvariantError(f"step {index} has invalid tool call")
            if self.tool_validator:
                self.tool_validator(name, args, query.type)
            steps.append({"tool_name": name, "arguments": args})
        return steps

    def _validate(self, query: Query) -> None:
        self._steps(query)

    def submit(self, query: Query) -> Query:
        with self._lock:
            if query.id in self._queries:
                raise QueueInvariantError("duplicate query id")
            if query.parent_interaction_id and query.tool_call_id:
                key = (query.parent_interaction_id, query.tool_call_id)
                existing_id = self._external_calls.get(key)
                if existing_id is not None:
                    return self._queries[existing_id]
            active_count = sum(item.state in {QueryState.QUEUED, QueryState.RUNNING, QueryState.PAUSED, QueryState.RECOVERY_REQUIRED} for item in self._queries.values())
            if active_count >= self.max_queries:
                raise QueueInvariantError("active Query capacity reached")
            self._validate(query)
            try:
                self.store.save(query)
            except Exception as exc:
                raise PersistenceError("cannot persist QUEUED Query before admission") from exc
            self._queries[query.id] = query
            if query.parent_interaction_id and query.tool_call_id:
                self._external_calls[(query.parent_interaction_id, query.tool_call_id)] = query.id
            self._sequence += 1
            heapq.heappush(self._heap, (self._class_key(query), -query.priority, self._sequence, query.id))
            active = self._queries.get(self._active) if self._active else None
            if query.type == QueryType.AQUERY and active and active.type == QueryType.SQUERY:
                self._preempt.set()
            self.audit.append(
                "query.enqueued",
                query_id=query.id,
                query_type=query.type.value,
                priority=query.priority,
            )
            return query

    def find_external_call(self, interaction_id: str, call_id: str) -> Query | None:
        if not interaction_id or not call_id:
            return None
        with self._lock:
            query_id = self._external_calls.get((interaction_id, call_id))
            return self._queries.get(query_id) if query_id else None

    @staticmethod
    def _class_key(query: Query) -> int:
        return 0 if query.type == QueryType.AQUERY else 1

    def record_failed(self, query: Query, error: str) -> None:
        with self._lock:
            query.state = QueryState.FAILED
            query.checkpoint.state = QueryState.FAILED
            query.checkpoint.error = str(error)[:4096]
            self.store.save(query)
            self._queries[query.id] = query
            self.audit.append("query.rejected", query_id=query.id, error=str(error))

    def _next_id(self) -> str | None:
        while self._heap:
            _, _, _, query_id = heapq.heappop(self._heap)
            query = self._queries.get(query_id)
            if query is not None and query.state == QueryState.QUEUED:
                return query_id
        return None

    def _persist_running(self, query: Query) -> bool:
        query.attempts += 1
        query.checkpoint.attempt = query.attempts
        query.checkpoint.execution_started_at = time.time()
        started = query.checkpoint.execution_started_at
        try:
            query.transition(QueryState.RUNNING, execution_started_at=started)
            self.store.save(query)
            return True
        except Exception as exc:
            query.attempts = max(0, query.attempts - 1)
            query.state = QueryState.QUEUED
            query.checkpoint.state = QueryState.QUEUED
            self.audit.append("query.running_persist_failed", query_id=query.id, error=str(exc))
            return False

    def _step_index(self, query: Query) -> int:
        if not isinstance(query.checkpoint.result, dict):
            return query.checkpoint.step_index
        value = query.checkpoint.result.get("next_step", query.checkpoint.step_index)
        if not isinstance(value, int) or isinstance(value, bool):
            raise QueueInvariantError("durable checkpoint next_step is not an integer")
        return value

    def _requeue(self, query: Query) -> None:
        self._sequence += 1
        heapq.heappush(self._heap, (self._class_key(query), -query.priority, self._sequence, query.id))

    def _save_after_side_effect(self, query: Query, state: QueryState, *, result: dict, tool_name: str) -> bool:
        query.checkpoint.result = result
        query.checkpoint.step_index = int(result["next_step"])
        try:
            query.transition(state, result=result, tool_name=tool_name)
            self.store.save(query)
            return True
        except Exception as exc:
            query.state = QueryState.RECOVERY_REQUIRED
            query.checkpoint.state = QueryState.RECOVERY_REQUIRED
            query.checkpoint.error = f"post-side-effect persistence failed: {exc}"[:4096]
            self.audit.append("query.recovery_required", query_id=query.id, error=str(exc), step=result["step_index"])
            return False

    def run_once(self) -> Query | None:
        with self._execution_lock:
            with self._lock:
                query_id = self._next_id()
                if query_id is None:
                    return None
                query = self._queries[query_id]
                self._active = query_id
                self._preempt.clear()
            try:
                if not self._persist_running(query):
                    with self._lock:
                        self._requeue(query)
                    return query

                try:
                    steps = self._steps(query)
                except Exception as exc:
                    query.state = QueryState.RECOVERY_REQUIRED
                    query.checkpoint.state = QueryState.RECOVERY_REQUIRED
                    query.checkpoint.error = f"durable Query validation failed: {exc}"[:4096]
                    try:
                        self.store.save(query)
                    except Exception:
                        pass
                    return query

                index = self._step_index(query)
                if not 0 <= index < len(steps):
                    query.state = QueryState.RECOVERY_REQUIRED
                    query.checkpoint.state = QueryState.RECOVERY_REQUIRED
                    query.checkpoint.error = "invalid durable checkpoint step index"
                    try:
                        self.store.save(query)
                    except Exception:
                        pass
                    return query

                step = steps[index]
                name = str(step["tool_name"])
                args = step["arguments"]
                assert isinstance(args, dict)
                query.checkpoint.tool_name = name
                query.checkpoint.tool_call_id = query.tool_call_id

                try:
                    if self.tool_validator:
                        self.tool_validator(name, args, query.type)
                    result = self.tool_execute(query.type, name, args)
                    if not isinstance(result, ToolResult):
                        result = ToolResult("ok", result)
                except ToolDenied as exc:
                    result = ToolResult("denied", error=str(exc))
                except Exception as exc:
                    result = ToolResult("error", error=f"{type(exc).__name__}: {exc}")

                checkpoint_result = result.as_dict()
                checkpoint_result.update({
                    "step_index": index,
                    "next_step": index + 1,
                    "tool_name": name,
                    "recorded_at": time.time(),
                })

                if result.status != "ok":
                    if self._save_after_side_effect(query, QueryState.FAILED, result=checkpoint_result, tool_name=name):
                        self.audit.append("query.step_failed", query_id=query.id, step=index, status=result.status)
                    return query

                next_index = index + 1
                preempted = query.type == QueryType.SQUERY and self._preempt.is_set() and next_index < len(steps)
                if next_index < len(steps):
                    if preempted:
                        if not self._save_after_side_effect(query, QueryState.PAUSED, result=checkpoint_result, tool_name=name):
                            return query
                        try:
                            query.transition(QueryState.QUEUED, result=checkpoint_result, tool_name=name)
                            self.store.save(query)
                        except Exception as exc:
                            query.state = QueryState.PAUSED
                            query.checkpoint.state = QueryState.PAUSED
                            query.checkpoint.error = f"resume persistence failed: {exc}"[:4096]
                            self.audit.append("query.resume_persist_failed", query_id=query.id, error=str(exc))
                            return query
                    else:
                        if not self._save_after_side_effect(query, QueryState.QUEUED, result=checkpoint_result, tool_name=name):
                            return query
                    with self._lock:
                        self._requeue(query)
                else:
                    if not self._save_after_side_effect(query, QueryState.COMPLETED, result=checkpoint_result, tool_name=name):
                        return query
                self.audit.append("query.step_completed", query_id=query.id, step=index, status=query.state.value, preempted=preempted)
                return query
            finally:
                with self._lock:
                    self._active = None
                    self._preempt.clear()

    def run_until_idle(self, max_steps: int = 1000) -> list[Query]:
        if not isinstance(max_steps, int) or not 1 <= max_steps <= 100_000:
            raise ValueError("max_steps must be between 1 and 100000")
        results: list[Query] = []
        for _ in range(max_steps):
            query = self.run_once()
            if query is None:
                break
            results.append(query)
        return results

    def restore(self) -> None:
        with self._lock:
            self._heap.clear()
            self._queries.clear()
            self._external_calls.clear()
            self._sequence = 0
            for path in sorted(self.store.query_dir.glob("*.qups")):
                try:
                    query = self.store.load(path.stem)
                except Exception as exc:
                    self.audit.append("queue.restore_rejected", path=str(path), error=str(exc))
                    continue
                if query.id in self._queries:
                    self.audit.append("queue.restore_duplicate", query_id=query.id)
                    continue
                if query.state in {QueryState.RUNNING, QueryState.RECOVERY_REQUIRED}:
                    query.state = QueryState.RECOVERY_REQUIRED
                    query.checkpoint.state = QueryState.RECOVERY_REQUIRED
                    query.checkpoint.error = "durable execution state is ambiguous after restart"
                    try:
                        self.store.save(query)
                    except Exception as exc:
                        self.audit.append("queue.restore_recovery_persist_failed", query_id=query.id, error=str(exc))
                elif query.state in {QueryState.QUEUED, QueryState.PAUSED}:
                    query.state = QueryState.QUEUED
                    query.checkpoint.state = QueryState.QUEUED
                    self._sequence += 1
                    heapq.heappush(self._heap, (self._class_key(query), -query.priority, self._sequence, query.id))

                if query.parent_interaction_id and query.tool_call_id:
                    key = (query.parent_interaction_id, query.tool_call_id)
                    existing = self._external_calls.get(key)
                    if existing is not None and existing != query.id:
                        query.state = QueryState.RECOVERY_REQUIRED
                        query.checkpoint.state = QueryState.RECOVERY_REQUIRED
                        query.checkpoint.error = "duplicate durable model tool_call identity; automatic execution disabled"
                        try:
                            self.store.save(query)
                        except Exception as exc:
                            self.audit.append(
                                "queue.restore_duplicate_external_persist_failed",
                                query_id=query.id,
                                error=str(exc),
                            )
                        self.audit.append(
                            "queue.restore_duplicate_external",
                            query_id=query.id,
                            duplicate_of=existing,
                            interaction_id=query.parent_interaction_id,
                            tool_call_id=query.tool_call_id,
                        )
                    else:
                        self._external_calls[key] = query.id

                self._queries[query.id] = query

    def resolve_recovery(self, query_id: str, action: str) -> None:
        with self._lock:
            try:
                query = self._queries[query_id]
            except KeyError as exc:
                raise QueueInvariantError(f"unknown Query: {query_id}") from exc
            if query.state != QueryState.RECOVERY_REQUIRED:
                raise QueueInvariantError("Query is not awaiting recovery")
            if action == "cancel":
                query.transition(QueryState.CANCELLED)
                self.store.save(query)
                self.audit.append("query.recovery_cancelled", query_id=query.id)
                return
            if action == "requeue":
                query.state = QueryState.QUEUED
                query.checkpoint.state = QueryState.QUEUED
                query.checkpoint.error = "explicit recovery requeue; external side effect may duplicate"
                self.store.save(query)
                self._requeue(query)
                self.audit.append(
                    "query.recovery_requeued",
                    query_id=query.id,
                    warning="external side effect may have already happened",
                )
                return
            raise QueueInvariantError("recovery action must be cancel or requeue")

    def get(self, query_id: str) -> Query:
        with self._lock:
            return self._queries[query_id]

    def all_queries(self) -> list[Query]:
        with self._lock:
            return list(self._queries.values())
