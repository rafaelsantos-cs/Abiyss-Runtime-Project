from __future__ import annotations

import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from .audit import AuditLog
from .atomic import atomic_write_bytes
from .errors import PersistenceError
from .memory import MemoryStore
from .security import ensure_base_dir_safe


@dataclass(frozen=True, slots=True)
class SleepConfig:
    tick_seconds: int = 300
    review_seconds: int = 1800
    max_pending: int = 256


class SleepManager:
    """Deferred memory consolidation and repetition review."""

    def __init__(
        self,
        *,
        memory: MemoryStore,
        audit: AuditLog,
        config: SleepConfig | None = None,
        obsidian_dir: Path | None = None,
    ) -> None:
        self.memory = memory
        self.audit = audit
        self.config = config or SleepConfig()
        if self.config.tick_seconds < 1 or self.config.review_seconds < 1 or self.config.max_pending < 1:
            raise ValueError("Sleep intervals and max_pending must be positive")
        self.obsidian_dir = Path(obsidian_dir) if obsidian_dir else None
        if self.obsidian_dir:
            ensure_base_dir_safe(self.obsidian_dir)
            self.obsidian_dir.mkdir(parents=True, exist_ok=True)
            ensure_base_dir_safe(self.obsidian_dir)
        self._last_tick: float | None = None
        self._last_review: float | None = None

    def tick(
        self,
        now: float | None = None,
        *,
        summarize: Callable[[list[dict]], str] | None = None,
    ) -> str | None:
        current = now if now is not None else time.time()
        if self._last_tick is not None and current - self._last_tick < self.config.tick_seconds:
            return None
        rows = self.memory.unrecapped_memories(limit=self.config.max_pending)
        if not rows:
            self._last_tick = current
            return None
        data = [{"source_id": row["source_id"], "content": row["content"]} for row in rows]
        source_ids = [item["source_id"] for item in data]
        summary = summarize(data) if summarize else "\n".join(item["content"] for item in data[:16])
        if not isinstance(summary, str) or not summary.strip():
            self._last_tick = current
            self.audit.append("sleep.recap_skipped", reason="empty_summary")
            return None
        fingerprint = self.memory.create_immutable_recap(source_ids, summary)
        self._last_tick = current
        if self.obsidian_dir:
            path = self.obsidian_dir / (
                f"sleep-{time.strftime('%Y%m%d-%H%M%S', time.localtime(current))}-{fingerprint[:12]}.md"
            )
            try:
                if not path.exists():
                    atomic_write_bytes(
                        path,
                        f"# Sleep Recap\n\nFingerprint: `{fingerprint}`\n\n{summary}\n".encode("utf-8"),
                        mode=0o600,
                    )
            except PersistenceError as exc:
                self.audit.append("sleep.obsidian_write_failed", error=str(exc), fingerprint=fingerprint)
        self.audit.append("sleep.recap_created", fingerprint=fingerprint, source_count=len(set(source_ids)))
        return fingerprint

    def review(self, now: float | None = None) -> dict | None:
        current = now if now is not None else time.time()
        if self._last_review is not None and current - self._last_review < self.config.review_seconds:
            return None
        self._last_review = current
        counts: dict[str, int] = {}
        for row in self.memory.recent_memories(limit=256):
            key = row["content"].strip().lower()[:256]
            counts[key] = counts.get(key, 0) + 1
        repeated = sorted(
            ((key, count) for key, count in counts.items() if count >= 2),
            key=lambda item: (-item[1], item[0]),
        )[:32]
        result = {"repeated": repeated, "observed": sum(counts.values())}
        self.audit.append("sleep.review", repeated=len(repeated), observed=result["observed"])
        return result
