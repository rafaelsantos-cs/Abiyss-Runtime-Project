from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path

import pytest

from abiyss.gemini import DeterministicProvider, ModelFunctionCall, ModelTurn
from abiyss.models import QueryState
from abiyss.runtime import AbiyssRuntime


def wait_for_socket(path: Path, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.02)
    raise RuntimeError(f"system-plane socket did not appear: {path}")


def spawn_daemon(binary: Path, root: Path, socket_path: Path) -> subprocess.Popen[str]:
    env = {
        **os.environ,
        "ABIYSS_SYSTEM_SOCKET": str(socket_path),
        "ABIYSS_SYSTEM_ROOT": str(root),
        "ABIYSS_SYSTEM_ALLOWED_UID": str(os.geteuid()),
        "ABIYSS_SYSTEM_ALLOW_EXEC": "0",
        "ABIYSS_SYSTEM_MAX_OUTPUT": "65536",
    }
    return subprocess.Popen(
        [str(binary)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )


def stop_daemon(process: subprocess.Popen[str]) -> None:
    process.terminate()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=2)
    if process.stderr:
        process.stderr.close()


def make_provider() -> DeterministicProvider:
    return DeterministicProvider(
        planner=lambda input_data, tools, previous: {
            "interaction_id": previous or "interaction-1",
            "function_calls": [{"id": "call-1", "name": "system.info", "arguments": {}}]
            if not previous
            else [],
            "output_text": "",
        }
    )


def test_runtime_query_reaches_real_rust_system_plane(tmp_path: Path) -> None:
    binary_raw = os.environ.get("ABIYSS_SYSTEM_BINARY")
    if not binary_raw:
        pytest.skip("ABIYSS_SYSTEM_BINARY not configured")
    binary = Path(binary_raw)
    if not binary.is_file():
        raise AssertionError(f"system-plane binary missing: {binary}")

    root = tmp_path / "runtime"
    socket_path = tmp_path / "system.sock"
    root.mkdir()
    process = spawn_daemon(binary, root, socket_path)
    try:
        wait_for_socket(socket_path)

        runtime = AbiyssRuntime(root, provider=make_provider(), system_socket=socket_path, system_timeout=2.0)
        try:
            turn = runtime.execute_agent_cycle("inspect the local runtime")
            assert turn.interaction_id == "interaction-1"
            query = runtime.qq.find_external_call("interaction-1", "call-1")
            assert query is not None
            assert query.state is QueryState.COMPLETED
            assert query.checkpoint.tool_name == "system.info"
            assert query.checkpoint.result is not None
            assert query.checkpoint.result["status"] == "ok"
            assert query.checkpoint.result["output"]["platform"] == "linux"
        finally:
            runtime.shutdown()
    finally:
        stop_daemon(process)


def test_completed_query_survives_runtime_restart_without_reexecution(tmp_path: Path) -> None:
    binary_raw = os.environ.get("ABIYSS_SYSTEM_BINARY")
    if not binary_raw:
        pytest.skip("ABIYSS_SYSTEM_BINARY not configured")
    binary = Path(binary_raw)
    if not binary.is_file():
        raise AssertionError(f"system-plane binary missing: {binary}")

    root = tmp_path / "runtime"
    socket_path = tmp_path / "system.sock"
    root.mkdir()
    process = spawn_daemon(binary, root, socket_path)
    try:
        wait_for_socket(socket_path)
        runtime = AbiyssRuntime(root, provider=make_provider(), system_socket=socket_path, system_timeout=2.0)
        try:
            runtime.execute_agent_cycle("inspect the local runtime")
            query = runtime.qq.find_external_call("interaction-1", "call-1")
            assert query is not None
            assert query.state is QueryState.COMPLETED
        finally:
            runtime.shutdown()

        restored = AbiyssRuntime(root, system_socket=socket_path, system_timeout=2.0)
        try:
            query = restored.qq.find_external_call("interaction-1", "call-1")
            assert query is not None
            assert query.state is QueryState.COMPLETED
            assert restored.execute_pending(10) == []
        finally:
            restored.shutdown()
    finally:
        stop_daemon(process)


def test_runtime_preserves_function_result_identity_for_provider() -> None:
    class RecordingProvider:
        def __init__(self) -> None:
            self.results: list[dict] | None = None

        def turn(self, input_data, tools, previous_interaction_id=None) -> ModelTurn:
            if previous_interaction_id is not None:
                raise AssertionError("initial turn unexpectedly received continuation id")
            return ModelTurn(
                "interaction-recorded",
                [ModelFunctionCall("call-recorded", "system.info", {})],
                "",
            )

        def send_results(self, previous_interaction_id, results, tools) -> ModelTurn:
            assert previous_interaction_id == "interaction-recorded"
            self.results = results
            return ModelTurn("interaction-recorded", [], "done")

    provider = RecordingProvider()
    from tempfile import TemporaryDirectory

    with TemporaryDirectory() as directory:
        runtime = AbiyssRuntime(Path(directory), provider=provider)
        try:
            turn = runtime.execute_agent_cycle("inspect")
            assert turn.output_text == "done"
            assert provider.results is not None
            assert len(provider.results) == 1
            result = provider.results[0]
            assert result["type"] == "function_result"
            assert result["name"] == "system.info"
            assert result["call_id"] == "call-recorded"
            payload = json.loads(result["result"][0]["text"])
            assert payload["status"] == "completed"
            assert payload["status"] != "ok"
            assert payload["tool_name"] == "system.info"
        finally:
            runtime.shutdown()


def test_runtime_rejects_duplicate_model_call_without_new_query(tmp_path: Path) -> None:
    runtime = AbiyssRuntime(tmp_path)
    try:
        turn = ModelTurn(
            "interaction-duplicate",
            [ModelFunctionCall("call-duplicate", "system.info", {})],
            "",
        )
        first = runtime.ingest_model_turn(turn)
        second = runtime.ingest_model_turn(turn)
        assert len(first) == len(second) == 1
        assert first[0].id == second[0].id
        assert len(runtime.qq.all_queries()) == 1
    finally:
        runtime.shutdown()
