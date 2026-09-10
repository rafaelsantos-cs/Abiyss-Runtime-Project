from __future__ import annotations

import json
import os
import subprocess
import threading
import uuid
from dataclasses import dataclass
from typing import Any

from .errors import ProviderError, ValidationError

PROTOCOL_VERSION = 1
MAX_REQUEST_BYTES = 256 * 1024
MAX_RESPONSE_BYTES = 256 * 1024


@dataclass(frozen=True, slots=True)
class NativeResponse:
    request_id: str
    result: Any = None
    error: str | None = None

    @property
    def ok(self) -> bool:
        return self.error is None


class NativeWorker:
    """Small synchronous JSONL boundary for optional native ABIYSS workers.

    The worker receives bounded, already-authorized operations. It is never given a
    shell command and does not participate in Query authorization.
    """

    def __init__(self, executable: str, *, timeout: float = 5.0) -> None:
        if not isinstance(executable, str) or not executable.startswith("/") or "\x00" in executable:
            raise ValidationError("native executable must be an absolute path")
        if not isinstance(timeout, (int, float)) or timeout <= 0 or timeout > 120:
            raise ValidationError("native worker timeout must be between 0 and 120 seconds")
        self.executable = executable
        self.timeout = float(timeout)
        self._lock = threading.RLock()

    def call(self, op: str, payload: dict[str, Any] | None = None) -> NativeResponse:
        if not isinstance(op, str) or not op or len(op.encode("utf-8")) > 128:
            raise ValidationError("invalid native operation")
        request_id = f"n_{uuid.uuid4().hex}"
        request = {"version": PROTOCOL_VERSION, "op": op, "request_id": request_id, "payload": payload or {}}
        try:
            encoded = json.dumps(request, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")
        except (TypeError, ValueError) as exc:
            raise ValidationError("native payload is not JSON-safe") from exc
        if len(encoded) > MAX_REQUEST_BYTES:
            raise ValidationError("native request too large")

        with self._lock:
            try:
                completed = subprocess.run(
                    [self.executable],
                    input=encoded + b"\n",
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=self.timeout,
                    check=False,
                    shell=False,
                    env={"PATH": "/usr/bin:/bin"},
                    close_fds=True,
                )
            except (OSError, subprocess.TimeoutExpired) as exc:
                raise ProviderError(f"native worker failed: {exc}") from exc

        if len(completed.stdout) > MAX_RESPONSE_BYTES:
            raise ProviderError("native worker response too large")
        try:
            response = json.loads(completed.stdout.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise ProviderError("native worker returned invalid JSON") from exc
        if not isinstance(response, dict) or response.get("version") != PROTOCOL_VERSION:
            raise ProviderError("native worker returned invalid protocol response")
        if response.get("request_id") != request_id:
            raise ProviderError("native worker response identity mismatch")
        if response.get("ok") is True:
            return NativeResponse(request_id=request_id, result=response.get("result"))
        return NativeResponse(request_id=request_id, error=str(response.get("error", "native worker error"))[:4096])
