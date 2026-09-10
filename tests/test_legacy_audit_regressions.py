from __future__ import annotations

import json
import os
import threading
import time
from pathlib import Path

import pytest

from abiyss.audit import AuditLog
from abiyss.errors import PersistenceError, ValidationError
from abiyss.memory import MemoryStore
from abiyss.models import Query, QueryType, ToolResult, ToolSpec
from abiyss.qups import QuPsStore, canonical
from abiyss.sleep import SleepConfig, SleepManager
from abiyss.tools import ProcessTool
from abiyss.schema import validate
from abiyss.qq import QueryQueue


def test_schema_unknown_type_fails_closed() -> None:
    with pytest.raises(ValidationError):
        validate("value", {"type": "sting"})


def test_qups_fifo_is_rejected_without_blocking(tmp_path: Path) -> None:
    store = QuPsStore(tmp_path / "qups")
    fifo = store.query_dir / "trap.qups"
    os.mkfifo(fifo)

    result: list[BaseException] = []

    def worker() -> None:
        try:
            store.load("trap")
        except BaseException as exc:
            result.append(exc)

    thread = threading.Thread(target=worker, daemon=True)
    thread.start()
    thread.join(timeout=1)
    assert not thread.is_alive(), "QuPs FIFO load blocked"
    assert result and isinstance(result[0], PersistenceError)


def test_qups_nested_json_is_bounded() -> None:
    value: object = {"x": "leaf"}
    for _ in range(40):
        value = {"x": value}
    with pytest.raises(ValidationError, match="nesting"):
        canonical(value)


def test_qups_container_cardinality_is_bounded() -> None:
    with pytest.raises(ValidationError, match="too many fields"):
        canonical({str(index): index for index in range(1025)})


def test_process_output_overflow_is_terminated_early(tmp_path: Path) -> None:
    spec = ToolSpec(
        "flood",
        "",
        QueryType.AQUERY,
        {"type": "object", "properties": {}, "additionalProperties": False},
        privileged=True,
        reversible=False,
    )
    tool = ProcessTool(
        spec,
        base_dir=tmp_path,
        allow_root=True,
        command_allowlist=("/usr/bin/yes",),
        timeout_seconds=5,
        max_output_bytes=4096,
    )
    started = time.monotonic()
    result = tool.run({"argv": ["/usr/bin/yes", "x"]})
    elapsed = time.monotonic() - started
    assert result.status == "error"
    assert result.error == "tool output exceeded configured capture limit"
    assert elapsed < 2
    assert len(result.output.encode("utf-8")) <= 4096 + 16


def test_query_becomes_recovery_required_when_post_effect_persistence_fails(tmp_path: Path) -> None:
    class FailingStore(QuPsStore):
        def __init__(self, root: Path) -> None:
            super().__init__(root)
            self.fail_after_running = False

        def save(self, query: Query) -> None:
            if self.fail_after_running and query.state.value in {"completed", "failed", "paused", "queued"}:
                raise PersistenceError("simulated post-effect persistence failure")
            super().save(query)

    store = FailingStore(tmp_path / "qups")
    audit = AuditLog(tmp_path / "audit.jsonl")
    side_effects: list[str] = []

    queue = QueryQueue(
        store=store,
        audit=audit,
        tool_execute=lambda _query_type, _name, _args: (side_effects.append("ran") or ToolResult("ok", {"done": True})),
    )
    query = Query.new(
        query_type=QueryType.AQUERY,
        priority=1,
        payload={"tool_name": "noop", "arguments": {}},
    )
    queue.submit(query)
    store.fail_after_running = True
    result = queue.run_once()

    assert result is query
    assert side_effects == ["ran"]
    assert query.state.value == "recovery_required"
    assert query.checkpoint.state.value == "recovery_required"
    assert "post-side-effect" in (query.checkpoint.error or "")

    # The failed terminal save must not be mistaken for durable recovery. The
    # on-disk envelope is still RUNNING, so a fresh queue must convert that
    # ambiguous state to RECOVERY_REQUIRED before it can execute anything.
    store.fail_after_running = False
    restored_audit = AuditLog(tmp_path / "restored-audit.jsonl")
    restored = QueryQueue(
        store=store,
        audit=restored_audit,
        tool_execute=lambda *_: (_ for _ in ()).throw(AssertionError("recovery path re-executed side effect")),
    )
    restored.restore()
    recovered = restored.get(query.id)
    assert recovered.state is QueryState.RECOVERY_REQUIRED
    assert restored.run_once() is None
    assert side_effects == ["ran"]


def test_sleep_failed_summary_is_throttled(tmp_path: Path) -> None:
    memory = MemoryStore(tmp_path / "memory.sqlite3")
    audit = AuditLog(tmp_path / "audit.jsonl")
    sleep = SleepManager(memory=memory, audit=audit, config=SleepConfig(tick_seconds=300, review_seconds=1800))
    memory.add_memory(memory.daily_key(1_700_000_000), "observation", "q1")

    calls = 0

    def empty_summary(_):
        nonlocal calls
        calls += 1
        return ""

    assert sleep.tick(now=1_700_000_000, summarize=empty_summary) is None
    assert sleep.tick(now=1_700_000_100, summarize=empty_summary) is None
    assert calls == 1
    memory.close()
