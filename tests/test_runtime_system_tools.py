from __future__ import annotations

from pathlib import Path

from abiyss.models import QueryType
from abiyss.runtime import AbiyssRuntime


def test_system_plane_exposes_model_facing_read_tools(tmp_path: Path) -> None:
    runtime = AbiyssRuntime(tmp_path, system_socket=tmp_path / "system.sock")
    names = {spec.name for spec in runtime.tools.specs_for(QueryType.AQUERY)}
    assert "system.info" in names
    assert "system.process.list" in names
    assert "system.file.read" in names
    runtime.shutdown()


def test_system_file_read_is_not_memory_persistent(tmp_path: Path) -> None:
    runtime = AbiyssRuntime(tmp_path, system_socket=tmp_path / "system.sock")
    spec = runtime.tools.get("system.file.read").spec
    assert spec.allow_memory_persistence is False
    assert spec.query_type is QueryType.AQUERY
    runtime.shutdown()
