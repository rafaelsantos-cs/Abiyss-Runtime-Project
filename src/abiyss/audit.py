from __future__ import annotations

import json
import os
import threading
import time
from pathlib import Path
from typing import Any

from .security import redact


class AuditLog:
    """Append-only JSONL audit stream with process-local serialization."""

    def __init__(self, path: Path) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()

    def append(self, event: str, **fields: Any) -> None:
        record = {"ts": time.time(), "event": event, **redact(fields)}
        line = json.dumps(record, ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n"
        with self._lock:
            with self.path.open("ab") as handle:
                handle.write(line.encode("utf-8"))
                handle.flush()
                os.fsync(handle.fileno())
