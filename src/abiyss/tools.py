from __future__ import annotations

import os
import resource
import signal
import subprocess
import threading
from pathlib import Path
from typing import Any, Callable

from .errors import ToolDenied
from .models import QueryType, ToolResult, ToolSpec
from .schema import validate
from .security import ensure_base_dir_safe, set_no_new_privs

_DANGEROUS_ENV = {
    "PATH", "PYTHONPATH", "PYTHONHOME", "PYTHONSTARTUP", "PYTHONINSPECT",
    "BASH_ENV", "ENV", "PERL5OPT", "PERL5LIB", "RUBYOPT", "NODE_OPTIONS",
    "LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT", "LD_DEBUG", "GCONV_PATH",
    "IFS", "HOME", "TMPDIR",
}


class Tool:
    def __init__(self, spec: ToolSpec) -> None:
        self.spec = spec

    def run(self, args: dict[str, Any]) -> ToolResult:
        raise NotImplementedError


class PythonTool(Tool):
    def __init__(self, spec: ToolSpec, fn: Callable[[dict[str, Any]], Any]) -> None:
        super().__init__(spec)
        self._fn = fn

    def run(self, args: dict[str, Any]) -> ToolResult:
        result = self._fn(args)
        return result if isinstance(result, ToolResult) else ToolResult("ok", result)


class ProcessTool(Tool):
    """Bounded process execution. This is a guardrail, not a hostile-code sandbox."""

    def __init__(
        self,
        spec: ToolSpec,
        *,
        base_dir: Path,
        allow_root: bool = False,
        command_allowlist: tuple[str, ...] = (),
        timeout_seconds: float = 20,
        max_output_bytes: int = 64 * 1024,
        max_args: int = 32,
        max_open_files: int = 128,
    ) -> None:
        super().__init__(spec)
        self.base_dir = ensure_base_dir_safe(Path(base_dir)).absolute()
        self.base_dir.mkdir(parents=True, exist_ok=True)
        ensure_base_dir_safe(self.base_dir)
        self.allow_root = bool(allow_root)
        self.allowlist = tuple(command_allowlist)
        self.timeout_seconds = float(timeout_seconds)
        self.max_output_bytes = int(max_output_bytes)
        self.max_args = int(max_args)
        self.max_open_files = int(max_open_files)
        if not 0.1 <= self.timeout_seconds <= 300:
            raise ValueError("invalid process timeout")
        if not 1024 <= self.max_output_bytes <= 4 * 1024 * 1024:
            raise ValueError("invalid process output limit")

    def _validate_cwd(self, cwd: str) -> Path:
        if not isinstance(cwd, str) or "\x00" in cwd or len(cwd.encode("utf-8")) > 4096:
            raise ToolDenied("invalid cwd")
        candidate = (self.base_dir / cwd).absolute()
        ensure_base_dir_safe(candidate)
        try:
            candidate.relative_to(self.base_dir)
        except ValueError as exc:
            raise ToolDenied("cwd escapes tool root") from exc
        if not candidate.is_dir() or candidate.is_symlink():
            raise ToolDenied("cwd is not a safe directory")
        return candidate

    def _validate_env(self, provided: Any, cwd: Path) -> dict[str, str]:
        if not isinstance(provided, dict) or len(provided) > 16:
            raise ToolDenied("invalid environment object")
        env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C", "HOME": str(cwd)}
        for key, value in provided.items():
            if (
                not isinstance(key, str) or not isinstance(value, str) or not key
                or len(key) > 64 or len(value.encode("utf-8")) > 4096
                or "\x00" in key or "\x00" in value
            ):
                raise ToolDenied("invalid environment field")
            upper = key.upper()
            if upper in _DANGEROUS_ENV or upper.startswith("LD_") or upper.startswith("DYLD_"):
                raise ToolDenied(f"dangerous environment variable: {key}")
            env[key] = value
        return env

    def run(self, args: dict[str, Any]) -> ToolResult:
        argv = args.get("argv")
        if not isinstance(argv, list) or not argv or len(argv) > self.max_args or any(not isinstance(x, str) for x in argv):
            raise ToolDenied("invalid argv")
        if any("\x00" in item or len(item.encode("utf-8")) > 4096 for item in argv):
            raise ToolDenied("invalid argv element")
        executable = argv[0]
        if not executable.startswith("/") or executable not in self.allowlist:
            raise ToolDenied("command is not an allowed absolute executable")
        executable_path = Path(executable)
        if not executable_path.is_file() or executable_path.is_symlink():
            raise ToolDenied("executable must be an existing non-symlink file")
        cwd = self._validate_cwd(args.get("cwd", "."))
        env = self._validate_env(args.get("env", {}), cwd)
        if os.geteuid() == 0 and not self.allow_root:
            raise ToolDenied("root execution disabled")

        def child_setup() -> None:
            if not set_no_new_privs() and os.geteuid() == 0:
                raise RuntimeError("could not establish no_new_privs")
            resource.setrlimit(resource.RLIMIT_NOFILE, (self.max_open_files, self.max_open_files))
            resource.setrlimit(resource.RLIMIT_FSIZE, (self.max_output_bytes, self.max_output_bytes))
            resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

        try:
            process = subprocess.Popen(
                argv,
                cwd=cwd,
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                close_fds=True,
                start_new_session=True,
                preexec_fn=child_setup if os.name == "posix" else None,
            )
        except (OSError, subprocess.SubprocessError) as exc:
            raise ToolDenied(f"process launch failed: {exc}") from exc

        output = bytearray()
        exceeded = False
        stop = threading.Event()

        def reader() -> None:
            nonlocal exceeded
            assert process.stdout is not None
            try:
                while True:
                    chunk = process.stdout.read(8192)
                    if not chunk:
                        break
                    remaining = self.max_output_bytes - len(output)
                    if remaining > 0:
                        output.extend(chunk[:remaining])
                    if len(chunk) > max(remaining, 0):
                        exceeded = True
                    if stop.is_set():
                        break
            except OSError:
                pass

        thread = threading.Thread(target=reader, name="abiyss-tool-output", daemon=True)
        thread.start()
        try:
            process.wait(timeout=self.timeout_seconds)
        except subprocess.TimeoutExpired:
            stop.set()
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
            process.wait(timeout=2)
            thread.join(timeout=2)
            return ToolResult("timeout", output.decode("utf-8", errors="replace"), "tool timeout")
        finally:
            thread.join(timeout=2)
            if process.stdout is not None:
                try:
                    process.stdout.close()
                except OSError:
                    pass
        error = "tool output exceeded configured capture limit" if exceeded else (None if process.returncode == 0 else f"exit={process.returncode}")
        return ToolResult("ok" if process.returncode == 0 and not exceeded else "error", output.decode("utf-8", errors="replace"), error)


class ToolRegistry:
    def __init__(self) -> None:
        self._tools: dict[str, Tool] = {}
        self._lock = threading.RLock()

    def register(self, tool: Tool) -> None:
        with self._lock:
            if tool.spec.name in self._tools:
                raise ValueError(f"duplicate tool: {tool.spec.name}")
            self._tools[tool.spec.name] = tool

    def get(self, name: str) -> Tool:
        with self._lock:
            try:
                return self._tools[name]
            except KeyError as exc:
                raise ToolDenied(f"unknown tool: {name}") from exc

    def validate_call(self, name: str, args: dict[str, Any], query_type: QueryType | None = None) -> None:
        tool = self.get(name)
        if query_type is not None and tool.spec.query_type != query_type:
            raise ToolDenied(f"tool {name} is not valid for {query_type.value}")
        validate(args, tool.spec.parameters)

    def specs_for(self, query_type: QueryType) -> list[ToolSpec]:
        with self._lock:
            return [tool.spec for tool in self._tools.values() if tool.spec.query_type == query_type]


class AST:
    def __init__(self, registry: ToolRegistry) -> None:
        self.registry = registry

    def execute(self, name: str, args: dict[str, Any]) -> ToolResult:
        self.registry.validate_call(name, args, QueryType.AQUERY)
        return self.registry.get(name).run(args)


class SST:
    def __init__(self, registry: ToolRegistry) -> None:
        self.registry = registry

    def execute(self, name: str, args: dict[str, Any]) -> ToolResult:
        self.registry.validate_call(name, args, QueryType.SQUERY)
        return self.registry.get(name).run(args)


def _process_list(limit: int) -> dict[str, Any]:
    limit = max(1, min(128, int(limit)))
    rows: list[dict[str, Any]] = []
    proc = Path("/proc")
    if not proc.exists():
        return {"processes": rows}
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        if len(rows) >= limit:
            break
        try:
            rows.append({"pid": int(entry.name), "comm": (entry / "comm").read_text(errors="replace").strip()[:128]})
        except (OSError, ValueError):
            continue
    return {"processes": rows}


def build_default_registry(*, base_dir: Path, allow_exec: bool = False, allow_root_exec: bool = False, command_allowlist: tuple[str, ...] = ()) -> ToolRegistry:
    registry = ToolRegistry()
    registry.register(PythonTool(
        ToolSpec("system.info", "Observe OS identity and ABIYSS process metadata.", QueryType.AQUERY,
                  {"type": "object", "properties": {}, "additionalProperties": False}),
        lambda _: {"platform": os.uname().sysname, "release": os.uname().release, "machine": os.uname().machine, "uid": os.geteuid(), "pid": os.getpid()},
    ))
    registry.register(PythonTool(
        ToolSpec("process.list", "Bounded local process observation.", QueryType.SQUERY,
                  {"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 128}}, "additionalProperties": False}),
        lambda args: _process_list(args.get("limit", 32)),
    ))
    if allow_exec:
        allowlist = tuple(command_allowlist) or ("/usr/bin/printf", "/usr/bin/id", "/usr/bin/uname")
        registry.register(ProcessTool(
            ToolSpec("system.exec", "Execute one explicitly allowlisted absolute executable without a shell.", QueryType.AQUERY,
                     {"type": "object", "properties": {
                         "argv": {"type": "array", "items": {"type": "string", "maxLength": 4096}, "minItems": 1, "maxItems": 32},
                         "cwd": {"type": "string", "maxLength": 4096},
                         "env": {"type": "object", "maxProperties": 16}},
                      "required": ["argv"], "additionalProperties": False},
                     privileged=True, reversible=False, allow_memory_persistence=False),
            base_dir=base_dir, allow_root=allow_root_exec, command_allowlist=allowlist,
        ))
    return registry
