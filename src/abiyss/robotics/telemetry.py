"""Robot telemetry journal (JSONL).

Record format is the one used by :class:`abiyss.audit.AuditLog`: one JSON
object per line, compact separators, sorted keys, ``ts`` (wall-clock unix
time) and ``event``, secret-looking keys redacted. Robotics records add:

* ``source``   = ``"abiyss.robotics"``
* ``run_id``   identifier of the controller run
* ``seq``      monotonically increasing record number within the run
* ``sim_time`` simulated (or hardware) control time in seconds
* ``backend``  ``"simulation"`` or ``"hardware"``

Discrete events (commands, actions, stalls, collisions, state changes,
gripper, camera, errors) are written immediately and can be mirrored into the
runtime AuditLog. Periodic ``joint_state`` samples are written at a reduced
rate (``sample_every`` control ticks) to keep journals reviewable.
"""

from __future__ import annotations

import json
import math
import os
import threading
import time
import uuid
from collections import deque
from pathlib import Path
from typing import Any, Iterable

from ..security import redact

CRITICAL_EVENTS = {"servo_stall", "servo_power_off", "safety_state", "emergency_stop", "fault", "backend_error", "run_end"}


def _clean(value: Any) -> Any:
    """Make values JSON-safe (no NaN/inf, tuples to lists, rounded floats)."""
    if isinstance(value, float):
        if not math.isfinite(value):
            return None
        return round(value, 7)
    if isinstance(value, dict):
        return {str(k): _clean(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_clean(v) for v in value]
    return value


class TelemetryJournal:
    def __init__(self, path: str | Path | None, *, backend: str = "simulation", run_id: str | None = None, mirror: Any = None, tail_size: int = 40, sample_every: int = 5, max_records: int = 200_000) -> None:
        self.path = Path(path) if path is not None else None
        self.backend = backend
        self.run_id = run_id or f"run_{uuid.uuid4().hex[:12]}"
        self.mirror = mirror              # optional abiyss.audit.AuditLog
        self.sample_every = max(1, int(sample_every))
        self.seq = 0
        self._lock = threading.Lock()
        self._fh = None
        self.tail: deque[str] = deque(maxlen=tail_size)
        # Bounded in-memory copy for analysis/tests (a long lab session keeps
        # the most recent records; the file on disk keeps everything).
        self.records: deque[dict[str, Any]] = deque(maxlen=max_records)
        self.keep_records = True
        if self.path is not None:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            if self.path.is_symlink():
                raise OSError(f"telemetry path is a symlink: {self.path}")
            fd = os.open(self.path, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0), 0o600)
            self._fh = os.fdopen(fd, "a", encoding="utf-8")

    def write(self, event: str, sim_time: float | None = None, **fields: Any) -> dict[str, Any]:
        with self._lock:
            self.seq += 1
            record = {
                "ts": time.time(),
                "event": event,
                "source": "abiyss.robotics",
                "run_id": self.run_id,
                "seq": self.seq,
                "backend": self.backend,
                **_clean(redact(fields)),
            }
            if sim_time is not None:
                record["sim_time"] = round(float(sim_time), 6)
            line = json.dumps(record, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
            if self._fh is not None:
                self._fh.write(line + "\n")
                self._fh.flush()
                if event in CRITICAL_EVENTS:
                    os.fsync(self._fh.fileno())
            if event != "joint_state":
                self.tail.append(_short(record))
            if self.keep_records:
                self.records.append(record)
        if self.mirror is not None and event != "joint_state":
            try:
                self.mirror.append(f"robotics.{event}", **{k: v for k, v in record.items() if k not in ("ts", "event")})
            except OSError:
                pass
        return record

    def events(self, name: str | None = None) -> list[dict[str, Any]]:
        return [r for r in self.records if name is None or r["event"] == name]

    def close(self) -> None:
        with self._lock:
            if self._fh is not None:
                self._fh.flush()
                os.fsync(self._fh.fileno())
                self._fh.close()
                self._fh = None


def _short(record: dict[str, Any]) -> str:
    skip = {"ts", "source", "run_id", "backend", "seq"}
    body = {k: v for k, v in record.items() if k not in skip}
    text = json.dumps(body, ensure_ascii=False, separators=(",", ":"), sort_keys=False)
    return text if len(text) <= 150 else text[:147] + "..."


def read_journal(path: str | Path) -> list[dict[str, Any]]:
    out = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                out.append(json.loads(line))
    return out


def joint_sample(state: dict[str, Any], names: Iterable[str]) -> dict[str, Any]:
    """Compact per-joint record for the periodic ``joint_state`` event."""
    out: dict[str, Any] = {}
    joints = dict(state.get("joints", {}))
    if "gripper" in state:
        joints["gripper"] = state["gripper"]
    for n in names:
        j = joints.get(n)
        if j is None:
            continue
        out[n] = {
            "target_deg": math.degrees(j["target"]),
            "position_deg": math.degrees(j["link_pos"]),
            "servo_deg": math.degrees(j["servo_pos"]),
            "error_deg": math.degrees(j["target"] - j["link_pos"]),
            "velocity_dps": math.degrees(j["link_vel"]) if "link_vel" in j else None,
            "torque_nm": j["tau_motor"],
            "requested_nm": j["tau_request"],
            "max_nm": j["tau_max"],
            "current_a": j["current"],
            "powered": j["powered"],
            "stalled": j["stalled"],
        }
    return out
