"""Robot API service: lets other processes (the Abiyss runtime, a CLI, a
notebook) talk to a running robot controller.

Transport: one JSON object per line over TCP, bound to 127.0.0.1 only.
Every request is executed on the control thread at a tick boundary
(``RobotController.call_soon``), so external calls never race the physics.

    {"call": "status"}
    {"call": "telemetry"}
    {"call": "stall_check"}
    {"call": "enqueue", "actions": [{"action": "write", "text": "OI"}]}
    {"call": "stop"} / {"call": "emergency_stop", "reason": "..."} / {"call": "reset"}
    {"call": "frame", "path": "/abs/path.png"}      # cam.frame(), saved by the service
    {"call": "queue"}                               # queue snapshot
    {"call": "journal_tail"}
"""

from __future__ import annotations

import json
import socket
import socketserver
import threading
from pathlib import Path
from typing import Any

from ..errors import AbiyssError, ValidationError
from .controller import RobotController

DEFAULT_API_PORT = 47012
MAX_REQUEST_BYTES = 256 * 1024


class _Handler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        ctl: RobotController = self.server.controller  # type: ignore[attr-defined]
        while True:
            line = self.rfile.readline(MAX_REQUEST_BYTES + 1)
            if not line:
                return
            if len(line) > MAX_REQUEST_BYTES:
                self._send({"ok": False, "error": "request too large"})
                return
            try:
                req = json.loads(line)
                if not isinstance(req, dict):
                    raise ValidationError("request must be an object")
                result = ctl.call_soon(lambda r=req: dispatch(ctl, r), wait=True, timeout=120.0)
                self._send({"ok": True, "id": req.get("id"), "result": result})
            except (AbiyssError, ValueError, TimeoutError, OverflowError) as exc:
                self._send({"ok": False, "error": str(exc)})
            except Exception as exc:  # report, never kill the connection silently
                self._send({"ok": False, "error": f"{type(exc).__name__}: {exc}"})

    def _send(self, obj: dict[str, Any]) -> None:
        self.wfile.write((json.dumps(obj, default=str, separators=(",", ":")) + "\n").encode("utf-8"))
        self.wfile.flush()


def dispatch(ctl: RobotController, req: dict[str, Any]) -> Any:
    call = req.get("call")
    if call == "status":
        return ctl.status()
    if call == "telemetry":
        return ctl.telemetry()
    if call == "stall_check":
        return ctl.stall_check()
    if call == "queue":
        return ctl.queue.snapshot()
    if call == "journal_tail":
        return list(ctl.journal.tail)
    if call == "enqueue":
        actions = req.get("actions")
        if not isinstance(actions, list) or not actions:
            raise ValidationError("enqueue needs a non-empty 'actions' list")
        return [ctl.submit(a, source=str(req.get("source", "api_server"))[:64]).summary() for a in actions]
    if call == "stop":
        ctl.stop("api_server stop")
        return ctl.status()
    if call == "emergency_stop":
        ctl.emergency_stop(str(req.get("reason", "api_server"))[:200])
        return ctl.status()
    if call == "reset":
        return {"reset": ctl.reset_fault(), "status": ctl.status()}
    if call == "frame":
        path = req.get("path")
        frame = ctl.camera_frame()
        out = frame.to_dict()
        if path:
            p = Path(str(path))
            if not p.is_absolute():
                raise ValidationError("frame path must be absolute")
            out["path"] = str(frame.save(p))
        return out
    raise ValidationError(f"unknown call: {call!r}")


class RobotAPIServer(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, controller: RobotController, port: int = DEFAULT_API_PORT) -> None:
        super().__init__(("127.0.0.1", port), _Handler)
        self.controller = controller
        self._thread: threading.Thread | None = None

    def start(self) -> "RobotAPIServer":
        self._thread = threading.Thread(target=self.serve_forever, name="robot-api", daemon=True)
        self._thread.start()
        return self

    def stop(self) -> None:
        self.shutdown()
        self.server_close()


class RobotClient:
    """Client of :class:`RobotAPIServer` (used by the CLI and runtime tools)."""

    def __init__(self, port: int = DEFAULT_API_PORT, timeout: float = 130.0) -> None:
        self.port = port
        self.timeout = timeout

    def call(self, call: str, **fields: Any) -> Any:
        with socket.create_connection(("127.0.0.1", self.port), timeout=self.timeout) as s:
            s.sendall((json.dumps({"call": call, **fields}) + "\n").encode("utf-8"))
            buf = b""
            while not buf.endswith(b"\n"):
                chunk = s.recv(65536)
                if not chunk:
                    break
                buf += chunk
        reply = json.loads(buf or b"{}")
        if not reply.get("ok"):
            raise AbiyssError(reply.get("error", "robot API error"))
        return reply.get("result")

    # Convenience methods mirror the Robot API.
    def status(self) -> Any:
        return self.call("status")

    def telemetry(self) -> Any:
        return self.call("telemetry")

    def stall_check(self) -> Any:
        return self.call("stall_check")

    def enqueue(self, actions: list[dict[str, Any]], source: str = "client") -> Any:
        return self.call("enqueue", actions=actions, source=source)

    def emergency_stop(self, reason: str = "client") -> Any:
        return self.call("emergency_stop", reason=reason)

    def stop(self) -> Any:
        return self.call("stop")

    def reset(self) -> Any:
        return self.call("reset")

    def frame(self, path: str | None = None) -> Any:
        return self.call("frame", path=path) if path else self.call("frame")
