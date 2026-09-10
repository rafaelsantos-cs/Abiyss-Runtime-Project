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
    native_rust: Path | None = None
    native_cpp: Path | None = None
    native_go: Path | None = None

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
        native = data.get("native", {})
        if not all(isinstance(section, dict) for section in (runtime, security, gemini, native)):
            raise ValidationError("runtime, security, gemini and native must be tables")

        root_raw = runtime.get("root", os.getenv("ABIYSS_ROOT", "~/.local/share/abiyss"))
        if not isinstance(root_raw, str) or not root_raw or len(root_raw.encode("utf-8")) > 4096:
            raise ValidationError("runtime.root must be a bounded path string")
        root = Path(root_raw).expanduser()

        model_raw = gemini.get("model", os.getenv("ABIYSS_GEMINI_MODEL", "gemini-3.8-flash"))
        if not isinstance(model_raw, str) or not model_raw.strip() or len(model_raw.encode("utf-8")) > 256:
            raise ValidationError("gemini.model must be a bounded non-empty string")
        thinking_raw = gemini.get("thinking_level", "medium")
        if not isinstance(thinking_raw, str) or thinking_raw not in {"low", "medium", "high"}:
            raise ValidationError("gemini.thinking_level must be low, medium or high")

        allowlist_raw = security.get("command_allowlist", [])
        if not isinstance(allowlist_raw, list) or len(allowlist_raw) > 64:
            raise ValidationError("command_allowlist must be a list of at most 64 entries")
        if any(not isinstance(item, str) or not item.startswith("/") or "\x00" in item or len(item.encode("utf-8")) > 4096 for item in allowlist_raw):
            raise ValidationError("command_allowlist entries must be bounded absolute paths")
        allowlist = tuple(dict.fromkeys(allowlist_raw))

        max_rounds_raw = runtime.get("max_rounds", 8)
        if not isinstance(max_rounds_raw, int) or isinstance(max_rounds_raw, bool) or not 1 <= max_rounds_raw <= 64:
            raise ValidationError("max_rounds must be an integer between 1 and 64")
        allow_exec = security.get("allow_exec", False)
        allow_root_exec = security.get("allow_root_exec", False)
        if not isinstance(allow_exec, bool) or not isinstance(allow_root_exec, bool):
            raise ValidationError("security execution flags must be booleans")
        if allow_root_exec and not allow_exec:
            raise ValidationError("allow_root_exec requires allow_exec=true")

        def native_path(name: str) -> Path | None:
            raw = native.get(name)
            if raw is None:
                return None
            if not isinstance(raw, str) or not raw.startswith("/") or "\x00" in raw or len(raw.encode("utf-8")) > 4096:
                raise ValidationError(f"native.{name} must be an absolute bounded path")
            return Path(raw)

        return cls(
            root=root,
            model=model_raw,
            thinking_level=thinking_raw,
            allow_exec=allow_exec,
            allow_root_exec=allow_root_exec,
            command_allowlist=allowlist,
            max_rounds=max_rounds_raw,
            native_rust=native_path("rust"),
            native_cpp=native_path("cpp"),
            native_go=native_path("go"),
        )
