from __future__ import annotations

import os
import tempfile
from pathlib import Path

from .errors import PersistenceError


def atomic_write_bytes(path: Path, data: bytes, *, mode: int = 0o600) -> None:
    """Atomically replace a file and fsync both file and parent directory."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = -1
    tmp: Path | None = None
    try:
        fd, tmp_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
        tmp = Path(tmp_name)
        os.fchmod(fd, mode)
        with os.fdopen(fd, "wb", closefd=True) as handle:
            fd = -1
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp, path)
        tmp = None
        dir_fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        try:
            os.fsync(dir_fd)
        finally:
            os.close(dir_fd)
    except OSError as exc:
        raise PersistenceError(f"atomic write failed for {path}: {exc}") from exc
    finally:
        if fd >= 0:
            try:
                os.close(fd)
            except OSError:
                pass
        if tmp is not None:
            try:
                tmp.unlink()
            except FileNotFoundError:
                pass
            except OSError:
                pass
