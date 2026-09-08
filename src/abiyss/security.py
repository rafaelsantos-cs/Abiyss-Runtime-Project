from __future__ import annotations

import ctypes
import os
import re
import sys
from pathlib import Path
from typing import Any

from .errors import SecurityError

_SECRET_KEY = re.compile(
    r"(?i)(api[_-]?key|token|password|passwd|authorization|secret|private[_-]?key|access[_-]?key|cookie|credential|session[_-]?id)"
)
_SAFE_ID = re.compile(r"^[A-Za-z0-9._-]{1,128}$")


def redact(value: Any, depth: int = 0) -> Any:
    """Recursively redact secret-looking mapping keys and bound log payload size."""
    if depth > 8:
        return "<depth-limit>"
    if isinstance(value, dict):
        result: dict[str, Any] = {}
        for key, item in list(value.items())[:128]:
            key_text = str(key)
            result[key_text] = "<redacted>" if _SECRET_KEY.search(key_text) else redact(item, depth + 1)
        return result
    if isinstance(value, (list, tuple)):
        return [redact(item, depth + 1) for item in list(value)[:128]]
    if isinstance(value, bytes):
        return value[:4096].decode("utf-8", errors="replace") + ("…<truncated>" if len(value) > 4096 else "")
    if isinstance(value, str) and len(value.encode("utf-8")) > 4096:
        return value.encode("utf-8")[:4096].decode("utf-8", errors="ignore") + "…<truncated>"
    return value


def validate_identifier(value: str, *, max_length: int = 128, label: str = "identifier") -> str:
    if not isinstance(value, str) or not value or len(value) > max_length or not _SAFE_ID.fullmatch(value):
        raise SecurityError(f"invalid {label}")
    return value


def ensure_base_dir_safe(path: Path) -> Path:
    """Reject symlinked path components before a runtime-owned directory is used."""
    absolute = Path(os.path.abspath(path))
    current = Path(absolute.anchor)
    for part in absolute.parts[1:]:
        current /= part
        try:
            if os.path.islink(current):
                raise SecurityError(f"symlink path component forbidden: {current}")
        except OSError as exc:
            raise SecurityError(f"cannot inspect path component {current}: {exc}") from exc
    return absolute


def assert_not_symlink(path: Path) -> None:
    try:
        if path.is_symlink():
            raise SecurityError(f"symlink forbidden: {path}")
    except OSError as exc:
        raise SecurityError(f"cannot inspect {path}: {exc}") from exc


def set_no_new_privs() -> bool:
    """Set Linux PR_SET_NO_NEW_PRIVS. Returns False on unsupported platforms."""
    if not sys.platform.startswith("linux"):
        return False
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    libc.prctl.restype = ctypes.c_int
    return libc.prctl(38, 1, 0, 0, 0) == 0


def ensure_trusted_executable(path: Path) -> None:
    """When running privileged, reject user-writable executables from trusted lists."""
    try:
        stat = path.stat()
    except OSError as exc:
        raise SecurityError(f"cannot stat executable {path}: {exc}") from exc
    if os.geteuid() == 0:
        if stat.st_uid != 0:
            raise SecurityError(f"privileged executable must be root-owned: {path}")
        if stat.st_mode & 0o022:
            raise SecurityError(f"privileged executable is group/world writable: {path}")
