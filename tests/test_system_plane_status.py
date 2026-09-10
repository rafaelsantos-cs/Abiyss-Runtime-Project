from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

from abiyss.models import QueryType
from abiyss.runtime import AbiyssRuntime


def test_system_exec_remote_error_is_not_marked_success(tmp_path: Path) -> None:
    runtime = AbiyssRuntime(tmp_path, system_socket=tmp_path / "system.sock")

    class FakePlane:
        def call(self, operation: str, args: dict[str, object]):
            assert operation == "process.exec"
            return SimpleNamespace(result={"status": "error", "error": "exit=17"})

    runtime.system_plane = FakePlane()  # type: ignore[assignment]
    result = runtime._execute_tool(QueryType.AQUERY, "system.exec", {"argv": ["/bin/example"]})

    assert result.status == "error"
    assert result.error == "exit=17"
    runtime.shutdown()


def test_system_exec_output_limit_is_normalized_to_error(tmp_path: Path) -> None:
    runtime = AbiyssRuntime(tmp_path, system_socket=tmp_path / "system.sock")

    class FakePlane:
        def call(self, _operation: str, _args: dict[str, object]):
            return SimpleNamespace(result={"status": "output_limit", "output": "x"})

    runtime.system_plane = FakePlane()  # type: ignore[assignment]
    result = runtime._execute_tool(QueryType.AQUERY, "system.exec", {"argv": ["/bin/example"]})

    assert result.status == "error"
    assert result.error == "tool output exceeded configured capture limit"
    runtime.shutdown()
