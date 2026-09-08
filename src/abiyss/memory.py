from __future__ import annotations

import hashlib
import json
import sqlite3
import threading
import time
from pathlib import Path
from typing import Any

from .errors import PersistenceError, SecurityError

MAX_CONTEXT_BYTES = 16 * 1024
MAX_CONTENT_BYTES = 64 * 1024
MAX_SOURCE_BYTES = 256
MAX_SOURCES_PER_RECAP = 128


class MemoryStore:
    """SQLite-backed memory with provenance, sensitivity and idempotence."""

    def __init__(self, path: Path) -> None:
        self.path = Path(path)
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
                """
            )

    def close(self) -> None:
        with self._lock:
            self.db.close()

    @staticmethod
    def _text(value: Any, limit: int, label: str) -> str:
        if not isinstance(value, str) or len(value.encode("utf-8")) > limit:
            raise SecurityError(f"invalid or oversized {label}")
        return value

    def get_or_create_key(self, kind: str, name: str, context: dict[str, Any] | None = None) -> int:
        kind = self._text(kind, 64, "key kind")
        name = self._text(name, 256, "key name")
        try:
            context_json = json.dumps(context or {}, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)
        except (TypeError, ValueError) as exc:
            raise SecurityError("key context is not JSON-safe") from exc
        self._text(context_json, MAX_CONTEXT_BYTES, "key context")
        with self._lock:
            row = self.db.execute("SELECT id FROM keys WHERE key_kind=? AND key_name=?", (kind, name)).fetchone()
            if row:
                return int(row[0])
            try:
                cursor = self.db.execute(
                    "INSERT INTO keys(key_kind,key_name,context_json,created_at) VALUES(?,?,?,?)",
                    (kind, name, context_json, time.time()),
                )
                return int(cursor.lastrowid)
            except sqlite3.IntegrityError:
                row = self.db.execute("SELECT id FROM keys WHERE key_kind=? AND key_name=?", (kind, name)).fetchone()
                if not row:
                    raise PersistenceError("key creation race lost without durable row")
                return int(row[0])

    def daily_key(self, now: float | None = None) -> int:
        tm = time.localtime(now if now is not None else time.time())
        return self.get_or_create_key("daily", time.strftime("%Y-%m-%d", tm))

    def contextual_key(self, name: str, context: dict[str, Any] | None = None) -> int:
        return self.get_or_create_key("context", name, context)

    def add_memory(self, key_id: int, content: str, source_id: str, *, sensitive: bool = False) -> int:
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
        self._text(relation, 128, "relation")
        with self._lock:
            self.db.execute(
                "INSERT OR IGNORE INTO relations(left_id,right_id,relation,created_at) VALUES(?,?,?,?)",
                (left_id, right_id, relation, time.time()),
            )

    def recent_memories(self, *, limit: int = 100, include_sensitive: bool = False) -> list[sqlite3.Row]:
        limit = max(1, min(1000, int(limit)))
        with self._lock:
            return list(self.db.execute(
                "SELECT * FROM memories WHERE sensitive=0 OR ? ORDER BY id DESC LIMIT ?",
                (int(include_sensitive), limit),
            ).fetchall())

    def _processed_source_ids(self) -> set[str]:
        processed: set[str] = set()
        for row in self.db.execute("SELECT source_ids_json FROM recaps").fetchall():
            try:
                values = json.loads(row[0])
            except (TypeError, ValueError, json.JSONDecodeError):
                continue
            if isinstance(values, list):
                processed.update(str(value) for value in values)
        return processed

    def unrecapped_memories(self, *, limit: int = 100) -> list[sqlite3.Row]:
        limit = max(1, min(1000, int(limit)))
        with self._lock:
            processed = self._processed_source_ids()
            rows = self.db.execute(
                "SELECT * FROM memories WHERE sensitive=0 ORDER BY id DESC LIMIT ?",
                (min(5000, limit * 20),),
            ).fetchall()
            return [row for row in rows if row["source_id"] not in processed][:limit]

    def create_immutable_recap(self, source_ids: list[str], content: str) -> str:
        normalized = sorted({self._text(item, MAX_SOURCE_BYTES, "source id") for item in source_ids})
        if not normalized:
            raise SecurityError("recap requires at least one source")
        if len(normalized) > MAX_SOURCES_PER_RECAP:
            normalized = normalized[:MAX_SOURCES_PER_RECAP]
        self._text(content, MAX_CONTENT_BYTES, "recap content")
        fingerprint = hashlib.sha256(("|".join(normalized) + "\n" + content).encode("utf-8")).hexdigest()
        with self._lock:
            self.db.execute(
                "INSERT OR IGNORE INTO recaps(fingerprint,source_ids_json,content,created_at) VALUES(?,?,?,?)",
                (fingerprint, json.dumps(normalized, ensure_ascii=False), content, time.time()),
            )
        return fingerprint

    def recaps(self, limit: int = 100) -> list[sqlite3.Row]:
        limit = max(1, min(1000, int(limit)))
        with self._lock:
            return list(self.db.execute("SELECT * FROM recaps ORDER BY created_at DESC LIMIT ?", (limit,)).fetchall())
EOF
