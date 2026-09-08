from __future__ import annotations

import hashlib
import json
import os
import threading
from pathlib import Path

import pytest

from abiyss.audit import AuditLog
from abiyss.cli import main
from abiyss.errors import PersistenceError, SecurityError, ToolDenied, ValidationError
from abiyss.memory import MemoryStore
from abiyss.models import Query, QueryState, QueryType, ToolResult, ToolSpec
from abiyss.qq import QueryQueue
from abiyss.qups import QuPsStore, make_envelope, validate_envelope
from abiyss.runtime import AbiyssRuntime
from abiyss.schema import validate
from abiyss.skills import SkillLoader
from abiyss.tools import ProcessTool, PythonTool, ToolRegistry


def registry_with_events():
    registry = ToolRegistry()
    events: list[str] = []
    empty = {"type": "object", "properties": {}, "additionalProperties": False}
    registry.register(PythonTool(ToolSpec("a", "", QueryType.AQUERY, empty), lambda _: events.append("a") or ToolResult("ok")))
    registry.register(PythonTool(ToolSpec("s", "", QueryType.SQUERY, empty), lambda _: events.append("s") or ToolResult("ok")))
    return registry, events


def make_queue(tmp_path: Path, registry: ToolRegistry) -> QueryQueue:
    return QueryQueue(
        store=QuPsStore(tmp_path / "qups"),
        audit=AuditLog(tmp_path / "logs" / "audit.jsonl"),
        tool_execute=lambda query_type, name, args: registry.get(name).run(args),
        tool_validator=registry.validate_call,
    )


def test_query_transition_contract():
    q = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "a", "arguments": {}})
    q.transition(QueryState.RUNNING)
    q.transition(QueryState.COMPLETED)
    with pytest.raises(ValidationError):
        q.transition(QueryState.RUNNING)


def test_qups_rejects_tampering_and_unknown_fields(tmp_path):
    store = QuPsStore(tmp_path / "qups")
    q = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "a", "arguments": {}})
    store.save(q)
    envelope = make_envelope(q)
    envelope["extra"] = True
    with pytest.raises(ValidationError):
        validate_envelope(envelope)
    path = store.path_for(q.id)
    envelope = json.loads(path.read_text())
    envelope["query"]["priority"] = 99
    path.write_text(json.dumps(envelope), encoding="utf-8")
    with pytest.raises(ValidationError):
        store.load(q.id)


def test_qups_refuses_symlink_and_fifo(tmp_path):
    store = QuPsStore(tmp_path / "qups")
    q = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "a", "arguments": {}})
    store.save(q)
    link = store.query_dir / "link.qups"
    link.symlink_to(store.path_for(q.id))
    with pytest.raises(PersistenceError):
        store.load("link")
    fifo = store.query_dir / "fifo.qups"
    os.mkfifo(fifo)
    with pytest.raises(PersistenceError):
        store.load("fifo")


def test_queue_aquery_class_precedes_squery(tmp_path):
    registry, events = registry_with_events()
    q = make_queue(tmp_path, registry)
    q.submit(Query.new(query_type=QueryType.SQUERY, priority=100, payload={"tool_name": "s", "arguments": {}}))
    q.submit(Query.new(query_type=QueryType.AQUERY, priority=-100, payload={"tool_name": "a", "arguments": {}}))
    q.run_until_idle()
    assert events == ["a", "s"]


def test_queue_preempts_squery_at_tool_boundary(tmp_path):
    registry, events = registry_with_events()
    entered = threading.Event()
    release = threading.Event()

    def slow(_):
        events.append("s")
        entered.set()
        assert release.wait(2)
        return ToolResult("ok")

    registry.register(PythonTool(ToolSpec("slow", "", QueryType.SQUERY, {"type": "object", "properties": {}, "additionalProperties": False}), slow))
    q = make_queue(tmp_path, registry)
    s = Query.new(query_type=QueryType.SQUERY, priority=1, payload={"steps": [{"tool_name": "slow", "arguments": {}}, {"tool_name": "s", "arguments": {}}]})
    q.submit(s)
    runner = threading.Thread(target=lambda: q.run_until_idle(4), daemon=True)
    runner.start()
    assert entered.wait(1)
    q.submit(Query.new(query_type=QueryType.AQUERY, priority=-100, payload={"tool_name": "a", "arguments": {}}))
    release.set()
    runner.join(timeout=3)
    assert not runner.is_alive()
    assert events == ["s", "a", "s"]


def test_queue_durable_running_before_side_effect(tmp_path):
    registry, events = registry_with_events()
    q = make_queue(tmp_path, registry)
    item = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "a", "arguments": {}})
    q.submit(item)
    original = q.store.save
    count = 0
    def fail_first(query):
        nonlocal count
        count += 1
        if count == 1:
            raise PersistenceError("synthetic")
        return original(query)
    q.store.save = fail_first  # type: ignore[method-assign]
    q.run_once()
    assert events == []
    assert q.get(item.id).state == QueryState.QUEUED


def test_queue_post_effect_failure_enters_recovery(tmp_path):
    registry, events = registry_with_events()
    q = make_queue(tmp_path, registry)
    item = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "a", "arguments": {}})
    q.submit(item)
    original = q.store.save
    count = 0
    def fail_after_effect(query):
        nonlocal count
        count += 1
        if count == 2:
            raise PersistenceError("synthetic")
        return original(query)
    q.store.save = fail_after_effect  # type: ignore[method-assign]
    q.run_once()
    assert events == ["a"]
    assert q.get(item.id).state == QueryState.RECOVERY_REQUIRED
    assert q.store.load(item.id).state == QueryState.RUNNING


def test_restore_running_never_replays(tmp_path):
    runtime = AbiyssRuntime(tmp_path)
    q = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "system.info", "arguments": {}})
    q.transition(QueryState.RUNNING)
    runtime.qups.save(q)
    runtime.shutdown()
    restored = AbiyssRuntime(tmp_path)
    assert restored.qq.get(q.id).state == QueryState.RECOVERY_REQUIRED
    restored.qq.resolve_recovery(q.id, "cancel")
    assert restored.qq.get(q.id).state == QueryState.CANCELLED
    restored.shutdown()


def test_ast_sst_role_isolation(tmp_path):
    runtime = AbiyssRuntime(tmp_path)
    with pytest.raises(ToolDenied):
        runtime.submit_query(Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "process.list", "arguments": {"limit": 1}}))
    runtime.shutdown()


def test_memory_daily_key_dedup_and_sensitive_filter(tmp_path):
    memory = MemoryStore(tmp_path / "memory.sqlite3")
    keys = [memory.daily_key(1_700_000_000) for _ in range(16)]
    assert len(set(keys)) == 1
    key = keys[0]
    first = memory.add_memory(key, "same", "source-1")
    second = memory.add_memory(key, "same", "source-1")
    assert first == second
    memory.add_memory(key, "secret", "source-2", sensitive=True)
    assert all(row["content"] != "secret" for row in memory.recent_memories())
    memory.close()


def test_sleep_recap_is_immutable(tmp_path):
    runtime = AbiyssRuntime(tmp_path)
    runtime.memory_event("one", "q1")
    runtime.memory_event("two", "q2")
    first = runtime.tick_sleep(now=1_700_000_000)
    assert first is not None
    assert runtime.tick_sleep(now=1_700_000_100) is None
    assert len(runtime.memory.recaps()) == 1
    runtime.shutdown()


def test_process_tool_is_shell_free_and_allowlisted(tmp_path):
    spec = ToolSpec(
        "exec",
        "",
        QueryType.AQUERY,
        {"type": "object", "properties": {}, "additionalProperties": False},
        privileged=True,
        reversible=False,
        allow_memory_persistence=False,
    )
    tool = ProcessTool(spec, base_dir=tmp_path, allow_root=True, command_allowlist=("/usr/bin/uname",))
    result = tool.run({"argv": ["/usr/bin/uname", "-s"]})
    assert result.status == "ok"
    with pytest.raises(ToolDenied):
        tool.run({"argv": ["/bin/sh", "-c", "id"]})


def test_process_output_flood_is_killed(tmp_path):
    spec = ToolSpec("flood", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False}, privileged=True, reversible=False)
    tool = ProcessTool(spec, base_dir=tmp_path, allow_root=True, command_allowlist=("/usr/bin/yes",), timeout_seconds=3, max_output_bytes=4096)
    result = tool.run({"argv": ["/usr/bin/yes", "x"]})
    assert result.status == "error"
    assert result.error == "tool output exceeded configured capture limit"
    assert len(result.output) <= 4096 + 16


def test_skill_tampering_is_detected(tmp_path):
    root = tmp_path / "skills"
    skill = root / "demo"
    skill.mkdir(parents=True)
    entry = skill / "run.sh"
    entry.write_text("#!/bin/sh\nprintf ok\n", encoding="utf-8")
    entry.chmod(0o700)
    digest = hashlib.sha256(entry.read_bytes()).hexdigest()
    (skill / "skill.json").write_text(json.dumps({"name": "demo", "version": "1", "entrypoint": ["run.sh"], "files": {"run.sh": digest}}), encoding="utf-8")
    entry.write_text("#!/bin/sh\nprintf pwned\n", encoding="utf-8")
    with pytest.raises(SecurityError):
        SkillLoader(root).load("demo")


def test_skill_rejects_unhashed_extra_file(tmp_path):
    root = tmp_path / "skills"
    skill = root / "demo"
    skill.mkdir(parents=True)
    entry = skill / "run.sh"
    entry.write_text("#!/bin/sh\nprintf ok\n", encoding="utf-8")
    entry.chmod(0o700)
    digest = hashlib.sha256(entry.read_bytes()).hexdigest()
    (skill / "extra.sh").write_text("#!/bin/sh\nprintf extra\n", encoding="utf-8")
    (skill / "skill.json").write_text(json.dumps({"name": "demo", "version": "1", "entrypoint": ["run.sh"], "files": {"run.sh": digest}}), encoding="utf-8")
    with pytest.raises(SecurityError):
        SkillLoader(root).load("demo")


def test_schema_rejects_unknown_type_and_extra_property():
    with pytest.raises(ValidationError):
        validate("x", {"type": "made_up"})
    with pytest.raises(ValidationError):
        validate({"x": 1, "y": 2}, {"type": "object", "properties": {"x": {"type": "integer"}}, "additionalProperties": False})


def test_audit_redacts_secrets(tmp_path):
    audit = AuditLog(tmp_path / "audit.jsonl")
    audit.append("event", api_key="SECRET", nested={"password": "PW", "safe": "yes"})
    text = (tmp_path / "audit.jsonl").read_text(encoding="utf-8")
    assert "SECRET" not in text and "PW" not in text and '"safe":"yes"' in text


def test_deterministic_model_cycle_reaches_tool_result(tmp_path):
    from abiyss.gemini import DeterministicProvider
    seen = []
    turn_count = 0

    def planner(input_data, tools, previous):
        nonlocal turn_count
        turn_count += 1
        if turn_count == 1:
            return {"interaction_id": "i1", "function_calls": [{"id": "c1", "name": "system.info", "arguments": {}}]}
        seen.append(input_data)
        return {"interaction_id": "i2", "function_calls": [], "output_text": "done"}

    runtime = AbiyssRuntime(tmp_path, provider=DeterministicProvider(planner))
    result = runtime.execute_agent_cycle("inspect", max_rounds=4)
    assert result.output_text == "done"
    assert seen and seen[0][0]["type"] == "function_result"
    runtime.shutdown()


def test_cli_doctor_and_status(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("ABIYSS_ROOT", str(tmp_path / "state"))
    assert main(["doctor"]) == 0
    doctor = json.loads(capsys.readouterr().out)
    assert doctor["version"] == "0.1.0"
    assert main(["status"]) == 0
    status = json.loads(capsys.readouterr().out)
    assert status["queries"] == 0
