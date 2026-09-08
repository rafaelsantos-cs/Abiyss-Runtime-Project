from __future__ import annotations

import json
import threading
import time
from pathlib import Path
from typing import Any

from .audit import AuditLog
from .errors import ProviderError, QueueInvariantError, RecoveryRequired, ValidationError
from .gemini import ModelProvider, ModelTurn
from .memory import MemoryStore
from .models import Query, QueryState, QueryType
from .qq import QueryQueue
from .qups import QuPsStore
from .security import ensure_base_dir_safe
from .sleep import SleepManager
from .tools import AST, SST, ToolRegistry, build_default_registry


class AbiyssRuntime:
    """Top-level ABIYSS runtime coordinating model, QQ, tools, memory and Sleep."""

    def __init__(
        self,
        root: Path,
        *,
        provider: ModelProvider | None = None,
        allow_exec: bool = False,
        allow_root_exec: bool = False,
        command_allowlist: tuple[str, ...] = (),
    ) -> None:
        self.root = ensure_base_dir_safe(Path(root)).absolute()
        self.root.mkdir(parents=True, exist_ok=True)
        ensure_base_dir_safe(self.root)
        self.audit = AuditLog(self.root / "logs" / "audit.jsonl")
        self.qups = QuPsStore(self.root / "qups")
        self.memory = MemoryStore(self.root / "memory" / "abiyss.sqlite3")
        self.tools: ToolRegistry = build_default_registry(
            base_dir=self.root / "tool-sandbox",
            allow_exec=allow_exec,
            allow_root_exec=allow_root_exec,
            command_allowlist=command_allowlist,
        )
        self.ast = AST(self.tools)
        self.sst = SST(self.tools)
        self.sleep = SleepManager(
            memory=self.memory,
            audit=self.audit,
            obsidian_dir=self.root / "obsidian",
        )
        self.qq = QueryQueue(
            store=self.qups,
            audit=self.audit,
            tool_execute=self._execute_tool,
            tool_validator=self.tools.validate_call,
        )
        self.qq.restore()
        self.provider = provider
        self.last_interaction_id: str | None = None
        self._model_queries: dict[str, list[str]] = {}
        self._closed = False
        self._lock = threading.RLock()

    def _execute_tool(self, query_type: QueryType, name: str, args: dict[str, Any]):
        return self.ast.execute(name, args) if query_type == QueryType.AQUERY else self.sst.execute(name, args)

    def submit_query(self, query: Query) -> Query:
        if self._closed:
            raise RuntimeError("ABIYSS runtime is closed")
        return self.qq.submit(query)

    def ingest_model_turn(self, turn: ModelTurn, *, priority: int = 100) -> list[Query]:
        queries: list[Query] = []
        for call in turn.function_calls:
            existing = self.qq.find_external_call(turn.interaction_id or "", call.id) if turn.interaction_id else None
            if existing is not None:
                queries.append(existing)
                continue
            query = Query.new(
                query_type=QueryType.AQUERY,
                priority=priority,
                payload={"tool_name": call.name, "arguments": call.arguments},
                source="gemini",
                parent_interaction_id=turn.interaction_id,
                tool_call_id=call.id,
            )
            try:
                submitted = self.submit_query(query)
                query = submitted if submitted.id != query.id else query
            except Exception as exc:
                self.qq.record_failed(query, str(exc))
            queries.append(query)
        if turn.interaction_id:
            self._model_queries[turn.interaction_id] = [query.id for query in queries]
        self.last_interaction_id = turn.interaction_id
        return queries

    def prompt(self, text: str) -> ModelTurn:
        if not isinstance(text, str) or not text.strip():
            raise ValueError("prompt must be non-empty text")
        if self.provider is None:
            raise ProviderError("no model provider configured")
        turn = self.provider.turn(
            text,
            self.tools.specs_for(QueryType.AQUERY),
            self.last_interaction_id,
        )
        self.ingest_model_turn(turn)
        return turn

    def execute_pending(self, max_steps: int = 1000) -> list[Query]:
        return self.qq.run_until_idle(max_steps)

    def _model_results_for_interaction(self, interaction_id: str) -> list[dict[str, Any]]:
        query_ids = self._model_queries.pop(interaction_id, [])
        if len(query_ids) > 64:
            raise QueueInvariantError("interaction produced too many tool calls")
        results: list[dict[str, Any]] = []
        for query_id in query_ids:
            query = self.qq.get(query_id)
            payload = query.checkpoint.result or {}
            status = query.state.value
            if query.state == QueryState.RECOVERY_REQUIRED:
                payload = {
                    "status": "recovery_required",
                    "error": query.checkpoint.error,
                    "step_index": query.checkpoint.step_index,
                }
            elif query.state == QueryState.FAILED:
                payload = {**payload, "status": "failed", "error": query.checkpoint.error}
            model_result = {**payload, "status": status}
            encoded = json.dumps(model_result, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")
            if len(encoded) > 128 * 1024:
                model_result = {"status": status, "error": "tool result omitted because it exceeds model result limit"}
                if query.checkpoint.error:
                    model_result["error"] = query.checkpoint.error[:4096]
            results.append(
                {
                    "type": "function_result",
                    "name": query.checkpoint.tool_name or query.payload.get("tool_name"),
                    "call_id": query.tool_call_id,
                    "result": [
                        {
                            "type": "text",
                            "text": json.dumps(model_result, ensure_ascii=False, separators=(",", ":"), allow_nan=False),
                        }
                    ],
                }
            )
        return results

    def _wait_for_model_queries(self, interaction_id: str, max_steps: int) -> None:
        query_ids = list(self._model_queries.get(interaction_id, []))
        for _ in range(max_steps):
            states = {self.qq.get(query_id).state for query_id in query_ids}
            if states and all(state in {QueryState.COMPLETED, QueryState.FAILED, QueryState.CANCELLED, QueryState.RECOVERY_REQUIRED} for state in states):
                return
            if not self.execute_pending(1):
                states = {self.qq.get(query_id).state for query_id in query_ids}
                if any(state == QueryState.RECOVERY_REQUIRED for state in states):
                    return
                time.sleep(0.005)
        raise QueueInvariantError("model Query set exceeded execution step budget")

    def execute_agent_cycle(
        self,
        text: str,
        *,
        max_rounds: int = 8,
        max_steps_per_round: int = 1000,
    ) -> ModelTurn:
        if self.provider is None:
            raise ProviderError("no model provider configured")
        if not 1 <= max_rounds <= 64:
            raise ValueError("max_rounds must be between 1 and 64")
        turn = self.prompt(text)
        for _ in range(max_rounds):
            interaction_id = turn.interaction_id
            if not interaction_id:
                return turn
            if turn.function_calls:
                self._wait_for_model_queries(interaction_id, max_steps_per_round)
                results = self._model_results_for_interaction(interaction_id)
                if not results:
                    return turn
                turn = self.provider.send_results(interaction_id, results, self.tools.specs_for(QueryType.AQUERY))
                self.ingest_model_turn(turn)
                continue
            return turn
        raise QueueInvariantError("agent cycle exceeded max_rounds")

    def memory_event(self, content: str, source_id: str, *, sensitive: bool = False) -> int:
        return self.memory.add_memory(self.memory.daily_key(), content, source_id, sensitive=sensitive)

    def tick_sleep(self, now: float | None = None) -> str | None:
        return self.sleep.tick(now=now)

    def review_sleep(self, now: float | None = None) -> dict | None:
        return self.sleep.review(now=now)

    def shutdown(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
            self.memory.close()
