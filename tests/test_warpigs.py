from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path

import pytest

from abiyss.audit import AuditLog
from abiyss.errors import PersistenceError, SecurityError, ToolDenied, ValidationError
from abiyss.models import Query, QueryState, QueryType, ToolResult, ToolSpec
from abiyss.qq import QueryQueue
from abiyss.qups import QuPsStore
from abiyss.schema import validate
from abiyss.skills import SkillLoader
from abiyss.tools import ProcessTool


def test_warpigs_schema_bool_is_not_integer():
    with pytest.raises(ValidationError):
        validate(True, {"type": "integer"})


def test_warpigs_schema_bool_is_not_number():
    with pytest.raises(ValidationError):
        validate(False, {"type": "number"})


def test_warpigs_qups_rejects_mismatched_filename(tmp_path):
    store = QuPsStore(tmp_path / "qups")
    q = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "x", "arguments": {}})
    store.save(q)
    other = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "x", "arguments": {}})
    path = store.path_for(q.id)
    envelope = json.loads(path.read_text())
    envelope["query"] = json.loads(json.dumps(envelope["query"]))
    envelope["query"]["id"] = other.id
    path.write_text(json.dumps(envelope), encoding="utf-8")
    with pytest.raises(ValidationError):
        store.load(q.id)


def test_warpigs_qups_parent_symlink_is_rejected(tmp_path):
    real = tmp_path / "real"
    real.mkdir()
    link = tmp_path / "link"
    link.symlink_to(real, target_is_directory=True)
    with pytest.raises((PersistenceError, OSError)):
        QuPsStore(link / "qups")


def test_warpigs_audit_log_symlink_is_rejected(tmp_path):
    target = tmp_path / "target"
    target.write_text("do not touch", encoding="utf-8")
    log = tmp_path / "audit.jsonl"
    log.symlink_to(target)
    with pytest.raises(OSError):
        AuditLog(log).append("attack")
    assert target.read_text(encoding="utf-8") == "do not touch"


def test_warpigs_process_shell_metacharacters_are_data(tmp_path):
    spec = ToolSpec("exec", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False}, privileged=True, reversible=False)
    tool = ProcessTool(spec, base_dir=tmp_path, allow_root=True, command_allowlist=("/usr/bin/printf",))
    result = tool.run({"argv": ["/usr/bin/printf", "$(touch /tmp/warpigs-pwned)"]})
    assert result.status == "ok"
    assert not Path("/tmp/warpigs-pwned").exists()


def test_warpigs_process_dangerous_environment_is_denied(tmp_path):
    spec = ToolSpec("exec", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False}, privileged=True, reversible=False)
    tool = ProcessTool(spec, base_dir=tmp_path, allow_root=True, command_allowlist=("/usr/bin/printf",))
    with pytest.raises(ToolDenied):
        tool.run({"argv": ["/usr/bin/printf", "x"], "env": {"LD_PRELOAD": "/tmp/x.so"}})


def test_warpigs_process_cwd_escape_is_denied(tmp_path):
    spec = ToolSpec("exec", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False}, privileged=True, reversible=False)
    tool = ProcessTool(spec, base_dir=tmp_path / "sandbox", allow_root=True, command_allowlist=("/usr/bin/printf",))
    with pytest.raises(ToolDenied):
        tool.run({"argv": ["/usr/bin/printf", "x"], "cwd": "../../"})


def test_warpigs_skill_path_symlink_is_denied(tmp_path):
    root = tmp_path / "skills"
    root.mkdir()
    target = tmp_path / "outside"
    target.mkdir()
    (root / "evil").symlink_to(target, target_is_directory=True)
    with pytest.raises(SecurityError):
        SkillLoader(root).load("evil")


def test_warpigs_skill_entrypoint_hash_is_mandatory(tmp_path):
    root = tmp_path / "skills"
    skill = root / "demo"
    skill.mkdir(parents=True)
    entry = skill / "run"
    entry.write_text("#!/bin/sh\nprintf ok\n", encoding="utf-8")
    entry.chmod(0o700)
    (skill / "skill.json").write_text(json.dumps({"name": "demo", "version": "1", "entrypoint": ["run"], "files": {}}), encoding="utf-8")
    with pytest.raises(SecurityError):
        SkillLoader(root).load("demo")


def test_warpigs_skill_hash_change_is_denied(tmp_path):
    root = tmp_path / "skills"
    skill = root / "demo"
    skill.mkdir(parents=True)
    entry = skill / "run"
    entry.write_text("#!/bin/sh\nprintf ok\n", encoding="utf-8")
    entry.chmod(0o700)
    digest = hashlib.sha256(entry.read_bytes()).hexdigest()
    (skill / "skill.json").write_text(json.dumps({"name": "demo", "version": "1", "entrypoint": ["run"], "files": {"run": digest}}), encoding="utf-8")
    entry.write_text("#!/bin/sh\nprintf pwned\n", encoding="utf-8")
    with pytest.raises(SecurityError):
        SkillLoader(root).load("demo")


def test_warpigs_qups_running_query_is_recovery_not_replay(tmp_path):
    store = QuPsStore(tmp_path / "qups")
    q = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "x", "arguments": {}})
    q.transition(QueryState.RUNNING)
    store.save(q)
    loaded = store.load(q.id)
    assert loaded.state == QueryState.RUNNING


def test_warpigs_queue_does_not_execute_unpersisted_query(tmp_path):
    from abiyss.audit import AuditLog
    from abiyss.tools import ToolRegistry, PythonTool
    events: list[str] = []
    registry = ToolRegistry()
    spec = ToolSpec("x", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False})
    registry.register(PythonTool(spec, lambda _: events.append("ran") or ToolResult("ok")))
    queue = QueryQueue(store=QuPsStore(tmp_path / "qups"), audit=AuditLog(tmp_path / "logs" / "audit.jsonl"), tool_execute=lambda qt, n, a: registry.get(n).run(a), tool_validator=registry.validate_call)
    query = Query.new(query_type=QueryType.AQUERY, priority=1, payload={"tool_name": "x", "arguments": {}})
    original = queue.store.save
    def fail(_query):
        raise PersistenceError("attack")
    queue.store.save = fail  # type: ignore[method-assign]
    queue.submit(query)
    queue.run_once()
    assert events == []
    queue.store.save = original  # type: ignore[method-assign]


def test_warpigs_sensitive_tool_result_is_not_assumed_safe():
    result = ToolResult("ok", {"secret": "value"}, sensitive=True)
    assert result.sensitive is True
    assert result.output == {"secret": "value"}


def test_warpigs_large_argument_is_rejected(tmp_path):
    spec = ToolSpec("exec", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False}, privileged=True, reversible=False)
    tool = ProcessTool(spec, base_dir=tmp_path, allow_root=True, command_allowlist=("/usr/bin/printf",))
    with pytest.raises(ToolDenied):
        tool.run({"argv": ["/usr/bin/printf", "x" * 5000]})


def test_warpigs_process_output_limit_is_enforced(tmp_path):
    spec = ToolSpec("flood", "", QueryType.AQUERY, {"type": "object", "properties": {}, "additionalProperties": False}, privileged=True, reversible=False)
    tool = ProcessTool(spec, base_dir=tmp_path, allow_root=True, command_allowlist=("/usr/bin/yes",), timeout_seconds=2, max_output_bytes=4096)
    result = tool.run({"argv": ["/usr/bin/yes", "x"]})
    assert result.status == "error"
    assert len(result.output) <= 4096 + 8192


def test_warpigs_skill_generic_launcher_is_denied(tmp_path):
    root = tmp_path / "skills"
    skill = root / "demo"
    skill.mkdir(parents=True)
    (skill / "skill.json").write_text(json.dumps({"name": "demo", "version": "1", "entrypoint": ["/bin/sh", "-c", "id"], "files": {}}), encoding="utf-8")
    with pytest.raises(SecurityError):
        SkillLoader(root).load("demo")


def test_warpigs_skill_manifest_size_is_bounded(tmp_path):
    root = tmp_path / "skills"
    skill = root / "demo"
    skill.mkdir(parents=True)
    (skill / "skill.json").write_bytes(b"{" + b"x" * 70000 + b"}")
    with pytest.raises(SecurityError):
        SkillLoader(root).load("demo")


def test_warpigs_config_boolean_coercion_is_not_accepted(tmp_path):
    from abiyss.config import RuntimeConfig
    path = tmp_path / "config.toml"
    path.write_text("allow_exec = \"yes\"\n", encoding="utf-8")
    with pytest.raises(Exception):
        RuntimeConfig.load(path)


def test_warpigs_no_new_privs_api_is_fail_closed():
    from abiyss.security import set_no_new_privs
    assert set_no_new_privs() is True


def test_warpigs_no_shell_calls_in_process_source():
    source = Path(__file__).parents[1] / "src" / "abiyss" / "tools.py"
    text = source.read_text(encoding="utf-8")
    assert "shell=True" not in text
    assert "os.system(" not in text


def test_warpigs_repeated_persistence_roundtrip_is_stable(tmp_path):
    store = QuPsStore(tmp_path / "qups")
    for index in range(100):
        q = Query.new(query_type=QueryType.AQUERY, priority=index % 3, payload={"tool_name": "x", "arguments": {"n": index}})
        store.save(q)
        assert store.load(q.id).id == q.id
