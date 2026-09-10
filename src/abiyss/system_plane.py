from __future__ import annotations

import json
import socket
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .errors import SystemPlaneError

PROTOCOL_VERSION = 1
MAX_REQUEST_BYTES = 128 * 1024
MAX_RESPONSE_BYTES = 128 * 1024


@dataclass(frozen=True, slots=True)
class SystemResponse:
    request_id: str
    result: Any


class SystemPlaneClient:
    """Strict one-request-per-connection client for the Rust Linux system plane."""

    def __init__(self, socket_path: Path, *, timeout: float = 5.0) -> None:
        self.socket_path = Path(socket_path)
        if not self.socket_path.is_absolute():
            raise ValueError("system-plane socket path must be absolute")
        if not 0.1 <= float(timeout) <= 60.0:
            raise ValueError("system-plane timeout must be between 0.1 and 60 seconds")
        self.timeout = float(timeout)

    @staticmethod
    def _bounded_line(sock: socket.socket, *, limit: int) -> bytes:
        data = bytearray()
        while True:
            chunk = sock.recv(min(8192, limit + 1 - len(data)))
            if not chunk:
                break
            newline = chunk.find(b"\n")
            if newline >= 0:
                data.extend(chunk[:newline])
                if len(data) > limit:
                    raise SystemPlaneError("system-plane response exceeds limit")
                return bytes(data)
            data.extend(chunk)
            if len(data) > limit:
                raise SystemPlaneError("system-plane response exceeds limit")
        if not data:
            raise SystemPlaneError("system-plane closed without a response")
        return bytes(data)

    def call(self, operation: str, args: dict[str, Any] | None = None) -> SystemResponse:
        if not isinstance(operation, str) or not operation or len(operation.encode("utf-8")) > 64 or "\x00" in operation:
            raise ValueError("invalid system-plane operation")
        if args is None:
            args = {}
        if not isinstance(args, dict):
            raise TypeError("system-plane args must be a mapping")
        request_id = f"req_{uuid.uuid4().hex}"
        payload = {
            "version": PROTOCOL_VERSION,
            "id": request_id,
            "op": operation,
            "args": args,
        }
        try:
            encoded = json.dumps(
                payload,
                ensure_ascii=False,
                separators=(",", ":"),
                sort_keys=True,
                allow_nan=False,
            ).encode("utf-8") + b"\n"
        except (TypeError, ValueError) as exc:
            raise SystemPlaneError("system-plane request is not JSON-safe") from exc
        if len(encoded) > MAX_REQUEST_BYTES:
            raise SystemPlaneError("system-plane request exceeds limit")

        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(self.timeout)
        try:
            sock.connect(str(self.socket_path))
            sock.sendall(encoded)
            raw = self._bounded_line(sock, limit=MAX_RESPONSE_BYTES)
        except OSError as exc:
            raise SystemPlaneError(f"system-plane transport failed: {exc}") from exc
        finally:
            sock.close()

        try:
            response = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SystemPlaneError("system-plane returned invalid JSON") from exc
        if not isinstance(response, dict):
            raise SystemPlaneError("system-plane response must be an object")
        if response.get("version") != PROTOCOL_VERSION:
            raise SystemPlaneError("system-plane protocol version mismatch")
        if response.get("id") != request_id:
            raise SystemPlaneError("system-plane response id mismatch")
        if response.get("ok") is True:
            if "result" not in response:
                raise SystemPlaneError("successful system-plane response lacks result")
            return SystemResponse(request_id, response["result"])
        if response.get("ok") is not False:
            raise SystemPlaneError("system-plane response has invalid ok field")
        error = response.get("error")
        if not isinstance(error, dict) or not isinstance(error.get("code"), str) or not isinstance(error.get("message"), str):
            raise SystemPlaneError("malformed system-plane error response")
        raise SystemPlaneError(f"{error['code']}: {error['message']}")

    def info(self) -> Any:
        return self.call("system.info").result

    def process_list(self, limit: int = 32) -> Any:
        if not isinstance(limit, int) or isinstance(limit, bool) or not 1 <= limit <= 128:
            raise ValueError("limit must be between 1 and 128")
        return self.call("process.list", {"limit": limit}).result

    def read_file(self, path: str, *, max_bytes: int = 64 * 1024) -> Any:
        if not isinstance(path, str) or not path or "\x00" in path:
            raise ValueError("invalid system-plane path")
        if not isinstance(max_bytes, int) or isinstance(max_bytes, bool) or not 1 <= max_bytes <= 64 * 1024:
            raise ValueError("max_bytes must be between 1 and 65536")
        return self.call("file.read", {"path": path, "max_bytes": max_bytes}).result
