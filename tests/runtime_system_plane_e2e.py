from __future__ import annotations

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

        provider = DeterministicProvider(
            planner=lambda input_data, tools, previous: {
                "interaction_id": previous or "interaction-1",
                "function_calls": [{"id": "call-1", "name": "system.info", "arguments": {}}]
                if not previous
                else [],
                "output_text": "",
            }
        )
        runtime = AbiyssRuntime(root, provider=provider, system_socket=socket_path, system_timeout=2.0)
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
