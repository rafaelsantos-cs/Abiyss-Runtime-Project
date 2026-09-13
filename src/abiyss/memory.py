from __future__ import annotations

import hashlib
import json
import math
import sqlite3
import threading
import time
from pathlib import Path
from typing import Any

from .errors import PersistenceError, SecurityError
from .security import assert_not_symlink, ensure_base_dir_safe

MAX_CONTEXT_BYTES = 16 * 1024
MAX_CONTEXT_DEPTH = 32
MAX_CONTENT_BYTES = 64 * 1024
MAX_SOURCE_BYTES = 256
MAX_SOURCES_PER_RECAP = 128


class MemoryStore:
    """SQLite-backed memory with provenance, sensitivity and idempotence."""

    def __init__(self, path: Path) -> None:
        self.path = Path(path)
        ensure_base_dir_safe(self.path.parent)
        if self.path.exists():
            assert_not_symlink(self.path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.RLock()
        try:
            self.db = sqlite3.connect(self.path, isolation_level=None, check_same_thread=False, timeout=10)
        except sqlite3.Error as exc:
            raise PersistenceError(f"cannot open memory database: {exc}") from exc
        self.db.row_factory = sqlite3.Row
        with self._lock:
            self.db.execute("PRAGMA journal_mode=WAL")
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.execute("PRAGMA foreign_keys=ON")
            self.db.execute("PRAGMA busy_timeout=10000")
            self.db.executescript(
                """
                CREATE TABLE IF NOT EXISTS keys (
                    id INTEGER PRIMARY KEY,
                    key_kind TEXT NOT NULL,
                    key_name TEXT NOT NULL,
                    context_json TEXT NOT NULL,
                    created_at REAL NOT NULL,
                    UNIQUE(key_kind, key_name)
                );
                CREATE TABLE IF NOT EXISTS memories (
                    id INTEGER PRIMARY KEY,
                    key_id INTEGER NOT NULL REFERENCES keys(id) ON DELETE CASCADE,
                    content TEXT NOT NULL,
                    fingerprint TEXT NOT NULL,
                    source_id TEXT NOT NULL,
                    created_at REAL NOT NULL,
                    sensitive INTEGER NOT NULL DEFAULT 0,
                    UNIQUE(key_id, fingerprint, source_id)
                );
                CREATE TABLE IF NOT EXISTS relations (
                    left_id INTEGER NOT NULL REFERENCES keys(id) ON DELETE CASCADE,
                    right_id INTEGER NOT NULL REFERENCES keys(id) ON DELETE CASCADE,
                    relation TEXT NOT NULL,
                    created_at REAL NOT NULL,
                    UNIQUE(left_id, right_id, relation)
                );
                CREATE TABLE IF NOT EXISTS recaps (
                    fingerprint TEXT PRIMARY KEY,
                    source_ids_json TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at REAL NOT NULL
                );
                CREATE TABLE IF NOT EXISTS recap_sources (
                    source_id TEXT PRIMARY KEY,
                    recap_fingerprint TEXT NOT NULL REFERENCES recaps(fingerprint) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_memories_source_id ON memories(source_id);
                CREATE INDEX IF NOT EXISTS idx_memories_key_created ON memories(key_id, id DESC);
                """
            )
            self._backfill_recap_sources()

    def close(self) -> None:
        with self._lock:
            self.db.close()

    @staticmethod
    def _text(value: Any, limit: int, label: str) -> str:
        if not isinstance(value, str) or len(value.encode("utf-8")) > limit:
            raise SecurityError(f"invalid or oversized {label}")
        return value

    @staticmethod
    def _finite_timestamp(value: float, label: str) -> float:
        if not isinstance(value, (int, float)) or isinstance(value, bool) or not math.isfinite(float(value)) or value <= 0:
            raise SecurityError(f"invalid {label}")
        return float(value)

    @staticmethod
    def _validate_context_depth(value: Any, *, max_depth: int = MAX_CONTEXT_DEPTH) -> None:
        """Reject deeply nested JSON-like contexts before invoking recursive encoders."""
        pending: list[tuple[Any, int]] = [(value, 0)]
        while pending:
            current, depth = pending.pop()
            if depth > max_depth:
                raise SecurityError("key context nesting too deep")
            if isinstance(current, dict):
                pending.extend((child, depth + 1) for child in current.values())
            elif isinstance(current, list):
                pending.extend((child, depth + 1) for child in current)
            elif isinstance(current, tuple):
                pending.extend((child, depth + 1) for child in current)

    def get_or_create_key(self, kind: str, name: str, context: dict[str, Any] | None = None) -> int:
        kind = self._text(kind, 64, "key kind")
        name = self._text(name, 256, "key name")
        context = context or {}
        self._validate_context_depth(context)
        try:
            context_json = json.dumps(
                context, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False
            ).encode("utf-8")
        except (TypeError, ValueError, RecursionError) as exc:
            raise SecurityError("key context is not JSON-safe") from exc
        if len(context_json) > MAX_CONTEXT_BYTES:
            raise SecurityError("invalid or oversized key context")
        context_text = context_json.decode("utf-8")
        with self._lock:
            row = self.db.execute("SELECT id FROM keys WHERE key_kind=? AND key_name=?", (kind, name)).fetchone()
            if row:
                return int(row[0])
            try:
                cursor = self.db.execute(
                    "INSERT INTO keys(key_kind,key_name,context_json,created_at) VALUES(?,?,?,?)",
                    (kind, name, context_text, time.time()),
                )
                return int(cursor.lastrowid)
            except sqlite3.IntegrityError:
                row = self.db.execute("SELECT id FROM keys WHERE key_kind=? AND key_name=?", (kind, name)).fetchone()
                if not row:
                    raise PersistenceError("key creation race lost without durable row")
                return int(row[0])

    def daily_key(self, now: float | None = None) -> int:
        timestamp = self._finite_timestamp(now if now is not None else time.time(), "daily-key timestamp")
        tm = time.localtime(timestamp)
        return self.get_or_create_key("daily", time.strftime("%Y-%m-%d", tm))

    def contextual_key(self, name: str, context: dict[str, Any] | None = None) -> int:
        return self.get_or_create_key("context", name, context)

    def add_memory(self, key_id: int, content: str, source_id: str, *, sensitive: bool = False) -> int:
        if not isinstance(key_id, int) or isinstance(key_id, bool) or key_id <= 0:
            raise SecurityError("invalid memory key id")
        self._text(content, MAX_CONTENT_BYTES, "memory content")
        self._text(source_id, MAX_SOURCE_BYTES, "source id")
        fingerprint = hashlib.sha256(content.encode("utf-8")).hexdigest()
        with self._lock:
            self.db.execute(
                "INSERT OR IGNORE INTO memories(key_id,content,fingerprint,source_id,created_at,sensitive) VALUES(?,?,?,?,?,?)",
                (key_id, content, fingerprint, source_id, time.time(), int(bool(sensitive))),
            )
            row = self.db.execute(
                "SELECT id FROM memories WHERE key_id=? AND fingerprint=? AND source_id=?",
                (key_id, fingerprint, source_id),
            ).fetchone()
            if not row:
                raise PersistenceError("memory write did not produce durable row")
            return int(row[0])

    def add_tool_result(self, key_id: int, *, content: str, source_id: str, sensitive: bool) -> int | None:
        if sensitive:
            return None
        return self.add_memory(key_id, content, source_id, sensitive=False)

    def add_relation(self, left_id: int, right_id: int, relation: str) -> None:
        if left_id == right_id:
            return
        if not isinstance(left_id, int) or isinstance(left_id, bool) or left_id <= 0:
            raise SecurityError("invalid left key id")
        if not isinstance(right_id, int) or isinstance(right_id, bool) or right_id <= 0:
            raise SecurityError("invalid right key id")
        self._text(relation, 128, "relation")
        with self._lock:
            self.db.execute(
                "INSERT OR IGNORE INTO relations(left_id,right_id,relation,created_at) VALUES(?,?,?,?)",
                (left_id, right_id, relation, time.time()),
            )

    def recent_memories(self, *, limit: int = 100, include_sensitive: bool = False) -> list[sqlite3.Row]:
        if not isinstance(limit, int) or isinstance(limit, bool):
            raise SecurityError("invalid memory limit")
        limit = max(1, min(1000, limit))
        with self._lock:
            return list(self.db.execute(
                "SELECT * FROM memories WHERE sensitive=0 OR ? ORDER BY id DESC LIMIT ?",
                (int(include_sensitive), limit),
            ).fetchall())

    def _backfill_recap_sources(self) -> None:
        """One-time migration from the original JSON source list to an indexed table."""
        rows = self.db.execute("SELECT fingerprint, source_ids_json FROM recaps").fetchall()
        for row in rows:
            try:
                values = json.loads(row["source_ids_json"])
            except (TypeError, ValueError, json.JSONDecodeError):
                continue
            if not isinstance(values, list):
                continue
            for source_id in values[:MAX_SOURCES_PER_RECAP]:
                if isinstance(source_id, str) and 0 < len(source_id.encode("utf-8")) <= MAX_SOURCE_BYTES:
                    self.db.execute(
                        "INSERT OR IGNORE INTO recap_sources(source_id,recap_fingerprint) VALUES(?,?)",
                        (source_id, row["fingerprint"]),
                    )

    def unrecapped_memories(self, *, limit: int = 100) -> list[sqlite3.Row]:
        if not isinstance(limit, int) or isinstance(limit, bool):
            raise SecurityError("invalid unrecapped-memory limit")
        limit = max(1, min(1000, limit))
        with self._lock:
            return list(self.db.execute(
                """
                SELECT m.*
                FROM memories AS m
                LEFT JOIN recap_sources AS rs ON rs.source_id = m.source_id
                WHERE m.sensitive=0 AND rs.source_id IS NULL
                ORDER BY m.id DESC
                LIMIT ?
                """,
                (limit,),
            ).fetchall())

    def create_immutable_recap(self, source_ids: list[str], content: str) -> str:
        if not isinstance(source_ids, list):
            raise SecurityError("recap source ids must be a list")
        normalized = sorted({self._text(item, MAX_SOURCE_BYTES, "source id") for item in source_ids})
        if not normalized:
            raise SecurityError("recap requires at least one source")
        if len(normalized) > MAX_SOURCES_PER_RECAP:
            raise SecurityError("recap contains too many source ids")
        self._text(content, MAX_CONTENT_BYTES, "recap content")
        fingerprint = hashlib.sha256(("|".join(normalized) + "\n" + content).encode("utf-8")).hexdigest()
        source_json = json.dumps(normalized, ensure_ascii=False, separators=(",", ":"))
        with self._lock:
            try:
                self.db.execute("BEGIN IMMEDIATE")
                self.db.execute(
                    "INSERT OR IGNORE INTO recaps(fingerprint,source_ids_json,content,created_at) VALUES(?,?,?,?)",
                    (fingerprint, source_json, content, time.time()),
                )
                self.db.executemany(
                    "INSERT OR IGNORE INTO recap_sources(source_id,recap_fingerprint) VALUES(?,?)",
                    [(source_id, fingerprint) for source_id in normalized],
                )
                self.db.execute("COMMIT")
            except sqlite3.Error as exc:
                try:
                    self.db.execute("ROLLBACK")
                except sqlite3.Error:
                    pass
                raise PersistenceError(f"failed to persist immutable recap: {exc}") from exc
        return fingerprint

    def recaps(self, limit: int = 100) -> list[sqlite3.Row]:
        if not isinstance(limit, int) or isinstance(limit, bool):
            raise SecurityError("invalid recap limit")
        limit = max(1, min(1000, limit))
        with self._lock:
            return list(self.db.execute("SELECT * FROM recaps ORDER BY created_at DESC LIMIT ?", (limit,)).fetchall())
