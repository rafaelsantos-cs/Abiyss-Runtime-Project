from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path

from .errors import ValidationError


@dataclass(frozen=True, slots=True)
class Config:
    root: Path
    model: str = "gemini-3.8-flash"
    thinking_level: str = "medium"
    allow_exec: bool = False
    allow_root_exec: bool = False
    command_allowlist: tuple[str, ...] = ()
    max_rounds: int = 8

    @classmethod
    def load(cls, path: Path | None = None) -> "Config":
        config_path = Path(path or os.getenv("ABIYSS_CONFIG", "~/.config/abiyss/config.toml")).expanduser()
        data: dict = {}
        if config_path.exists():
            try:
                with config_path.open("rb") as handle:
                    data = tomllib.load(handle)
            except (OSError, tomllib.TOMLDecodeError) as exc:
                raise ValidationError(f"invalid ABIYSS config: {exc}") from exc
        if not isinstance(data, dict):
            raise ValidationError("ABIYSS config must be a table")
        runtime = data.get("runtime", {})
        security = data.get("security", {})
        gemini = data.get("gemini", {})
        if not all(isinstance(section, dict) for section in (runtime, security, gemini)):
            raise ValidationError("runtime, security and gemini must be tables")
        root_raw = runtime.get("root", os.getenv("ABIYSS_ROOT", "~/.local/share/abiyss"))
        if not isinstance(root_raw, str) or not root_raw or len(root_raw.encode("utf-8")) > 4096:
            raise ValidationError("runtime.root must be a bounded path string")
        root = Path(root_raw).expanduser()
        model_raw = gemini.get("model", os.getenv("ABIYSS_GEMINI_MODEL", "gemini-3.8-flash"))
        if not isinstance(model_raw, str) or not model_raw.strip() or len(model_raw.encode("utf-8")) > 256:
            raise ValidationError("gemini.model must be a bounded non-empty string")
        model = model_raw
        thinking_raw = gemini.get("thinking_level", "medium")
        if not isinstance(thinking_raw, str) or thinking_raw not in {"low", "medium", "high"}:
            raise ValidationError("gemini.thinking_level must be low, medium or high")
        thinking_level = thinking_raw
        allowlist_raw = security.get("command_allowlist", [])
        if not isinstance(allowlist_raw, list) or len(allowlist_raw) > 64:
            raise ValidationError("command_allowlist must be a list of at most 64 entries")
        if any(
            not isinstance(item, str)
            or not item.startswith("/")
            or "\x00" in item
            or len(item.encode("utf-8")) > 4096
            for item in allowlist_raw
        ):
            raise ValidationError("command_allowlist entries must be bounded absolute paths")
        allowlist = tuple(dict.fromkeys(allowlist_raw))
        max_rounds_raw = runtime.get("max_rounds", 8)
        if not isinstance(max_rounds_raw, int) or isinstance(max_rounds_raw, bool):
            raise ValidationError("max_rounds must be an integer")
        max_rounds = max_rounds_raw
        allow_exec = security.get("allow_exec", False)
        allow_root_exec = security.get("allow_root_exec", False)
        if not isinstance(allow_exec, bool) or not isinstance(allow_root_exec, bool):
            raise ValidationError("security execution flags must be booleans")
        if not 1 <= max_rounds <= 64:
            raise ValidationError("max_rounds must be between 1 and 64")
        if allow_root_exec and not allow_exec:
            raise ValidationError("allow_root_exec requires allow_exec=true")
        return cls(
            root=root,
            model=model,
            thinking_level=thinking_level,
            allow_exec=allow_exec,
            allow_root_exec=allow_root_exec,
            command_allowlist=allowlist,
            max_rounds=max_rounds,
        )
