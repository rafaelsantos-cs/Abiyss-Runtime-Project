from __future__ import annotations

import hashlib
import hmac
import json
import math
import os
import stat
from pathlib import Path
from typing import Any

from .atomic import atomic_write_bytes
from .errors import PersistenceError, SecurityError, ValidationError
from .models import Query
from .security import assert_not_symlink, ensure_base_dir_safe

QUPS_VERSION = 1
MAX_QUPS_BYTES = 512 * 1024


def _validate_json_tree(value: Any, *, path: str = "$") -> None:
    if isinstance(value, float) and not math.isfinite(value):
        raise ValidationError(f"non-finite value at {path}")
    if isinstance(value, dict):
        if any(not isinstance(key, str) for key in value):
            raise ValidationError(f"non-string key at {path}")
        for key, item in value.items():
            _validate_json_tree(item, path=f"{path}.{key}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            _validate_json_tree(item, path=f"{path}[{index}]")


def canonical(value: Any) -> bytes:
    _validate_json_tree(value)
    try:
        return json.dumps(
            value,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
            allow_nan=False,
        ).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise ValidationError("value is not canonical JSON") from exc


def make_envelope(query: Query) -> dict[str, Any]:
    body = {"qups_version": QUPS_VERSION, "kind": "query", "query": query.snapshot()}
    body["sha256"] = hashlib.sha256(canonical(body)).hexdigest()
    return body


def validate_envelope(envelope: Any) -> Query:
    if not isinstance(envelope, dict):
        raise ValidationError("QuPs envelope must be an object")
    required = {"qups_version", "kind", "query", "sha256"}
    if set(envelope) != required:
        raise ValidationError("invalid QuPs envelope fields")
    if envelope["qups_version"] != QUPS_VERSION or envelope["kind"] != "query":
        raise ValidationError("unsupported QuPs envelope")
    digest = envelope["sha256"]
    if not isinstance(digest, str) or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
        raise ValidationError("invalid sha256")
    body = {"qups_version": envelope["qups_version"], "kind": envelope["kind"], "query": envelope["query"]}
    expected = hashlib.sha256(canonical(body)).hexdigest()
    if not hmac.compare_digest(digest, expected):
        raise ValidationError("QuPs hash mismatch")
    _validate_json_tree(body)
    return Query.from_snapshot(body["query"])


class QuPsStore:
    """Durable Query store. QuPs integrity is explicit, not an authentication mechanism."""

    def __init__(self, root: Path) -> None:
        root = ensure_base_dir_safe(root)
        self.root = Path(root).absolute()
        self.root.mkdir(parents=True, exist_ok=True)
        ensure_base_dir_safe(self.root)
        self.query_dir = self.root / "queries"
        self.query_dir.mkdir(parents=True, exist_ok=True)
        ensure_base_dir_safe(self.query_dir)

    def path_for(self, query_id: str) -> Path:
        if not isinstance(query_id, str) or not query_id or any(
            char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-" for char in query_id
        ):
            raise ValidationError("unsafe query id")
        return self.query_dir / f"{query_id}.qups"

    def save(self, query: Query) -> None:
        data = canonical(make_envelope(query))
        if len(data) > MAX_QUPS_BYTES:
            raise PersistenceError("QuPs payload too large")
        atomic_write_bytes(self.path_for(query.id), data, mode=0o600)

    def _read_nofollow(self, path: Path) -> bytes:
        """Read only regular files without blocking on attacker-controlled FIFOs."""
        try:
            assert_not_symlink(path)
        except SecurityError as exc:
            raise PersistenceError(str(exc)) from exc
        flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NONBLOCK
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            fd = os.open(path, flags)
        except OSError as exc:
            raise PersistenceError(f"cannot open QuPs: {path}: {exc}") from exc
        try:
            metadata = os.fstat(fd)
            if not stat.S_ISREG(metadata.st_mode):
                raise PersistenceError("QuPs path is not a regular file")
            chunks: list[bytes] = []
            total = 0
            while total <= MAX_QUPS_BYTES:
                chunk = os.read(fd, min(64 * 1024, MAX_QUPS_BYTES + 1 - total))
                if not chunk:
                    break
                chunks.append(chunk)
                total += len(chunk)
                if total > MAX_QUPS_BYTES:
                    break
            return b"".join(chunks)
        except OSError as exc:
            raise PersistenceError(f"cannot read QuPs: {path}: {exc}") from exc
        finally:
            os.close(fd)

    def load(self, query_id: str) -> Query:
        path = self.path_for(query_id)
        raw = self._read_nofollow(path)
        if len(raw) > MAX_QUPS_BYTES:
            raise PersistenceError("QuPs payload too large")
        try:
            envelope = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise PersistenceError("invalid QuPs JSON") from exc
        try:
            return validate_envelope(envelope)
        except ValidationError:
            raise
        except Exception as exc:
            raise PersistenceError(f"invalid QuPs envelope: {exc}") from exc
