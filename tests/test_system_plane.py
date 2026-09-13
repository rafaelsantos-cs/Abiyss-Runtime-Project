from __future__ import annotations

import json
import socket
import threading
from pathlib import Path

import pytest

from abiyss.errors import SystemPlaneError
from abiyss.system_plane import MAX_REQUEST_BYTES, SystemPlaneClient


def serve_once(path: Path, handler) -> threading.Thread:
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(str(path))
    listener.listen(1)

    def run() -> None:
        try:
            connection, _ = listener.accept()
            try:
                handler(connection)
            finally:
                connection.close()
        finally:
            listener.close()
            try:
                path.unlink()
            except FileNotFoundError:
                pass

    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return thread


def test_system_plane_round_trip(tmp_path: Path) -> None:
    path = tmp_path / "system.sock"

    def handler(connection: socket.socket) -> None:
        raw = connection.recv(4096)
        request = json.loads(raw.decode("utf-8"))
        assert request["op"] == "system.info"
        response = {
            "version": 1,
            "id": request["id"],
            "ok": True,
            "result": {"platform": "linux", "uid": 1000},
        }
        connection.sendall(json.dumps(response).encode() + b"\n")

    thread = serve_once(path, handler)
    result = SystemPlaneClient(path).info()
    thread.join(timeout=1)
    assert result["platform"] == "linux"


def test_system_plane_rejects_response_id_mismatch(tmp_path: Path) -> None:
    path = tmp_path / "system.sock"

    def handler(connection: socket.socket) -> None:
        raw = connection.recv(4096)
        request = json.loads(raw.decode("utf-8"))
        response = {"version": 1, "id": request["id"] + "-wrong", "ok": True, "result": {}}
        connection.sendall(json.dumps(response).encode() + b"\n")

    thread = serve_once(path, handler)
    with pytest.raises(SystemPlaneError, match="response id mismatch"):
        SystemPlaneClient(path).call("system.info")
    thread.join(timeout=1)


def test_system_plane_rejects_oversized_request(tmp_path: Path) -> None:
    path = tmp_path / "system.sock"
    client = SystemPlaneClient(path)
    with pytest.raises(SystemPlaneError, match="request exceeds limit"):
        client.call("x", {"payload": "x" * MAX_REQUEST_BYTES})


def test_system_plane_rejects_malformed_error(tmp_path: Path) -> None:
    path = tmp_path / "system.sock"

    def handler(connection: socket.socket) -> None:
        raw = connection.recv(4096)
        request = json.loads(raw.decode("utf-8"))
        response = {"version": 1, "id": request["id"], "ok": False, "error": {"code": 1}}
        connection.sendall(json.dumps(response).encode() + b"\n")

    thread = serve_once(path, handler)
    client = SystemPlaneClient(path)
    with pytest.raises(SystemPlaneError, match="malformed system-plane error"):
        client.call("system.info")
    thread.join(timeout=1)


def test_system_plane_skill_verify_request_contract(tmp_path: Path) -> None:
    path = tmp_path / "system.sock"
    skill = tmp_path / "skills" / "demo"
    skill.mkdir(parents=True)

    def handler(connection: socket.socket) -> None:
        raw = connection.recv(4096)
        request = json.loads(raw.decode("utf-8"))
        assert request["op"] == "skill.verify"
        assert request["args"] == {"directory": str(skill.resolve())}
        response = {
            "version": 1,
            "id": request["id"],
            "ok": True,
            "result": {
                "directory": str(skill.resolve()),
                "name": "demo",
                "version": "1",
                "entrypoint": ["runner"],
                "files": {"runner": "0" * 64},
                "allow_root": False,
                "timeout_seconds": 10.0,
                "max_output_bytes": 65536,
                "max_args": 32,
            },
        }
        connection.sendall(json.dumps(response).encode() + b"\n")

    thread = serve_once(path, handler)
    result = SystemPlaneClient(path).verify_skill(skill)
    thread.join(timeout=1)
    assert result["name"] == "demo"
    assert result["files"]["runner"] == "0" * 64


def test_system_plane_verify_skill_rejects_relative_directory(tmp_path: Path) -> None:
    client = SystemPlaneClient(tmp_path / "missing.sock")
    with pytest.raises(ValueError, match="absolute"):
        client.verify_skill(Path("relative/skill"))
