from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from abiyss.system_plane import SystemPlaneClient


def wait_for_socket(path: Path, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.02)
    raise RuntimeError(f"system-plane socket did not appear: {path}")


def test_real_rust_system_plane_round_trip(tmp_path: Path) -> None:
    binary_raw = os.environ.get("ABIYSS_SYSTEM_BINARY")
    if not binary_raw:
        pytest.skip("ABIYSS_SYSTEM_BINARY not configured")
    binary = Path(binary_raw)
    if not binary.is_file():
        raise AssertionError(f"system-plane binary missing: {binary}")

    root = tmp_path / "system-root"
    socket_path = tmp_path / "system.sock"
    root.mkdir()
    (root / "hello.txt").write_text("ABIYSS system plane\n", encoding="utf-8")

    env = {
        **os.environ,
        "ABIYSS_SYSTEM_SOCKET": str(socket_path),
        "ABIYSS_SYSTEM_ROOT": str(root),
        "ABIYSS_SYSTEM_ALLOWED_UID": str(os.geteuid()),
        "ABIYSS_SYSTEM_ALLOW_EXEC": "0",
        "ABIYSS_SYSTEM_MAX_OUTPUT": "65536",
    }
    process = subprocess.Popen(
        [str(binary)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        wait_for_socket(socket_path)
        client = SystemPlaneClient(socket_path, timeout=2.0)
        info = client.info()
        assert info["platform"] == "linux"
        assert info["uid"] == os.geteuid()

        data = client.read_file("hello.txt")
        assert data["data"] == "ABIYSS system plane\n"
        assert data["bytes"] == len("ABIYSS system plane\n".encode())

        processes = client.process_list(4)
        assert isinstance(processes, list)
        assert len(processes) <= 4
    finally:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)
        if process.stderr:
            process.stderr.close()
