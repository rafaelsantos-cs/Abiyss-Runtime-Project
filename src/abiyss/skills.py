from __future__ import annotations

import hashlib
import hmac
import json
import os
import resource
import signal
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path

from .errors import SecurityError, ToolDenied
from .security import ensure_base_dir_safe, ensure_trusted_executable, set_no_new_privs

MAX_MANIFEST_BYTES = 64 * 1024
MAX_FILES = 256
MAX_ARGS = 32
MAX_ARG_BYTES = 4096
MAX_NAME_BYTES = 128
_SYSTEM_EXEC_PREFIXES = (Path("/bin"), Path("/usr/bin"), Path("/usr/local/bin"))
_FORBIDDEN_LAUNCHERS = {
    "env", "bash", "sh", "dash", "zsh", "fish", "python", "python3", "python3.11", "python3.12",
    "python3.13", "python3.14", "node", "nodejs", "ruby", "perl", "php", "lua", "luajit", "java", "tclsh", "awk",
}


@dataclass(frozen=True, slots=True)
class SkillManifest:
    name: str
    version: str
    entrypoint: tuple[str, ...]
    files: dict[str, str]
    allow_root: bool = False
    timeout_seconds: float = 10.0
    max_output_bytes: int = 64 * 1024
    max_args: int = 32

    @classmethod
    def load(cls, path: Path) -> "SkillManifest":
        if path.is_symlink() or not path.is_file():
            raise SecurityError("unsafe skill manifest")
        raw = path.read_bytes()
        if len(raw) > MAX_MANIFEST_BYTES:
            raise SecurityError("skill manifest too large")
        try:
            obj = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SecurityError("invalid skill manifest JSON") from exc
        if not isinstance(obj, dict):
            raise SecurityError("skill manifest must be an object")
        name, version, entrypoint, files = obj.get("name"), obj.get("version"), obj.get("entrypoint"), obj.get("files", {})
        if not isinstance(name, str) or not 1 <= len(name.encode()) <= MAX_NAME_BYTES:
            raise SecurityError("invalid skill name")
        if not isinstance(version, str) or not 1 <= len(version.encode()) <= 128:
            raise SecurityError("invalid skill version")
        if not isinstance(entrypoint, list) or not 1 <= len(entrypoint) <= MAX_ARGS:
            raise SecurityError("invalid skill entrypoint")
        if any(not isinstance(item, str) or not item or "\x00" in item or len(item.encode()) > MAX_ARG_BYTES for item in entrypoint):
            raise SecurityError("invalid skill entrypoint token")
        if not isinstance(files, dict) or len(files) > MAX_FILES:
            raise SecurityError("invalid skill file map")
        allow_root = obj.get("allow_root", False)
        if not isinstance(allow_root, bool):
            raise SecurityError("allow_root must be boolean")
        normalized: dict[str, str] = {}
        for relative, digest in files.items():
            path_obj = Path(relative)
            if (not isinstance(relative, str) or not relative or path_obj.is_absolute() or ".." in path_obj.parts
                    or not isinstance(digest, str) or len(digest) != 64
                    or any(character not in "0123456789abcdef" for character in digest)):
                raise SecurityError(f"invalid skill file hash entry: {relative!r}")
            normalized[relative] = digest
        try:
            timeout = float(obj.get("timeout_seconds", 10.0))
            max_output = int(obj.get("max_output_bytes", 64 * 1024))
            max_args = int(obj.get("max_args", MAX_ARGS))
        except (TypeError, ValueError) as exc:
            raise SecurityError("invalid skill limits") from exc
        if not 0.1 <= timeout <= 300 or not 1024 <= max_output <= 4 * 1024 * 1024 or not 1 <= max_args <= MAX_ARGS:
            raise SecurityError("skill limits outside policy")
        return cls(name, version, tuple(entrypoint), normalized, allow_root, timeout, max_output, max_args)


class SkillLoader:
    def __init__(self, root: Path, *, allow_root_skills: bool = False) -> None:
        self.root = ensure_base_dir_safe(Path(root)).absolute()
        self.root.mkdir(parents=True, exist_ok=True)
        ensure_base_dir_safe(self.root)
        self.allow_root_skills = bool(allow_root_skills)

    def _inside(self, path: Path) -> Path:
        raw = Path(os.path.abspath(path))
        current = Path(raw.anchor)
        for part in raw.parts[1:]:
            current /= part
            if os.path.islink(current):
                raise SecurityError(f"symlink skill path component: {current}")
        resolved = raw.resolve(strict=False)
        try:
            resolved.relative_to(self.root)
        except ValueError as exc:
            raise SecurityError("skill path escapes root") from exc
        return resolved

    def _check_entrypoint(self, directory: Path, manifest: SkillManifest) -> None:
        first = Path(manifest.entrypoint[0])
        if first.is_absolute():
            if first.is_symlink() or not first.is_file():
                raise SecurityError("absolute skill executable is unavailable or a symlink")
            ensure_trusted_executable(first)
            if not any(first.is_relative_to(prefix) for prefix in _SYSTEM_EXEC_PREFIXES):
                raise SecurityError("absolute skill executable is outside the trusted system prefixes")
            if first.name in _FORBIDDEN_LAUNCHERS:
                raise SecurityError("generic command launcher is not an allowed skill entrypoint")
        else:
            resolved = self._inside(directory / first)
            if not resolved.is_file() or resolved.is_symlink():
                raise SecurityError("relative skill entrypoint is unavailable")
            relative = str(resolved.relative_to(directory))
            if relative not in manifest.files:
                raise SecurityError("relative skill entrypoint must be hashed in manifest")

    def _ensure_privileged_tree(self, directory: Path) -> None:
        if os.geteuid() != 0:
            return
        for candidate in [directory, *directory.rglob("*")]:
            if candidate.is_symlink():
                raise SecurityError(f"privileged skill tree contains symlink: {candidate}")
            try:
                stat = candidate.stat()
            except OSError as exc:
                raise SecurityError(f"cannot stat privileged skill path: {candidate}") from exc
            if stat.st_uid != 0 or stat.st_mode & 0o022:
                raise SecurityError(f"privileged skill path is not root-owned and private: {candidate}")

    def verify(self, directory: Path, manifest: SkillManifest) -> None:
        directory = self._inside(directory)
        self._check_entrypoint(directory, manifest)
        expected_files = set(manifest.files)
        actual_files = {str(path.relative_to(directory)) for path in directory.rglob("*") if path.is_file() and not path.is_symlink() and path.name != "skill.json"}
        if actual_files != expected_files:
            missing = sorted(expected_files - actual_files)
            unexpected = sorted(actual_files - expected_files)
            raise SecurityError(f"skill file set mismatch; missing={missing[:8]}, unexpected={unexpected[:8]}")
        for relative, expected in manifest.files.items():
            path = self._inside(directory / relative)
            if not path.is_file() or path.is_symlink():
                raise SecurityError(f"missing or unsafe skill file: {relative}")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            if not hmac.compare_digest(actual, expected):
                raise SecurityError(f"skill hash mismatch: {relative}")
        if manifest.allow_root and not self.allow_root_skills:
            raise SecurityError("root skill execution disabled by policy")
        if manifest.allow_root:
            self._ensure_privileged_tree(directory)

    def load(self, name: str) -> tuple[Path, SkillManifest]:
        directory = self._inside(self.root / name)
        if not directory.is_dir() or directory.is_symlink():
            raise SecurityError("skill directory is unavailable or unsafe")
        manifest = SkillManifest.load(directory / "skill.json")
        self.verify(directory, manifest)
        return directory, manifest

    def execute(self, name: str, args: list[str]) -> dict:
        directory, manifest = self.load(name)
        if not isinstance(args, list) or len(args) > manifest.max_args:
            raise ToolDenied("invalid skill arguments")
        if any(not isinstance(item, str) or "\x00" in item or len(item.encode()) > MAX_ARG_BYTES for item in args):
            raise ToolDenied("invalid skill argument")
        if os.geteuid() == 0 and not manifest.allow_root:
            raise ToolDenied("root skill execution denied")

        def child_setup() -> None:
            if not set_no_new_privs():
                raise RuntimeError("could not establish no_new_privs")
            cpu_limit = max(1, int(manifest.timeout_seconds) + 1)
            resource.setrlimit(resource.RLIMIT_CPU, (cpu_limit, cpu_limit))
            resource.setrlimit(resource.RLIMIT_FSIZE, (manifest.max_output_bytes, manifest.max_output_bytes))
            resource.setrlimit(resource.RLIMIT_NOFILE, (128, 128))
            resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

        environment = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C", "HOME": str(directory)}
        argv = list(manifest.entrypoint) + list(args)
        try:
            process = subprocess.Popen(argv, cwd=directory, env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, close_fds=True, start_new_session=True,
                                       preexec_fn=child_setup if os.name == "posix" else None)
        except (OSError, subprocess.SubprocessError) as exc:
            raise ToolDenied(f"skill launch failed: {exc}") from exc

        output = bytearray()
        stop = threading.Event()
        exceeded = False

        def reader() -> None:
            nonlocal exceeded
            assert process.stdout is not None
            try:
                while True:
                    chunk = process.stdout.read(8192)
                    if not chunk:
                        break
                    remaining = manifest.max_output_bytes - len(output)
                    if remaining > 0:
                        output.extend(chunk[:remaining])
                    if len(chunk) > max(remaining, 0):
                        exceeded = True
                        stop.set()
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except (ProcessLookupError, PermissionError):
                            pass
                        break
                    if stop.is_set():
                        break
            except OSError:
                pass

        thread = threading.Thread(target=reader, daemon=True, name="abiyss-skill-output")
        thread.start()
        deadline = time.monotonic() + manifest.timeout_seconds
        try:
            while True:
                try:
                    process.wait(timeout=min(0.1, max(0.0, deadline - time.monotonic())))
                    break
                except subprocess.TimeoutExpired:
                    if stop.is_set():
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except (ProcessLookupError, PermissionError):
                            try:
                                process.kill()
                            except ProcessLookupError:
                                pass
                        process.wait(timeout=2)
                        break
                    if time.monotonic() >= deadline:
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
                        return {"status": "timeout", "output": output.decode("utf-8", errors="replace"), "error": "skill timeout"}
        finally:
            stop.set()
            thread.join(timeout=2)
            if process.stdout is not None:
                try:
                    process.stdout.close()
                except OSError:
                    pass
        return {"status": "error" if exceeded or process.returncode else "ok", "returncode": process.returncode,
                "output": output.decode("utf-8", errors="replace"), "error": "output limit exceeded" if exceeded else None}


@dataclass(frozen=True, slots=True)
class SkillCandidate:
    signature: str
    observations: int
    competence: float
    qualified: bool


class SkillEmergence:
    def candidate(self, observations: list[dict], min_repetition: int = 3, min_competence: float = 0.8) -> SkillCandidate | None:
        if not observations:
            return None
        counts: dict[str, list[bool]] = {}
        for observation in observations:
            signature = str(observation.get("signature", ""))
            counts.setdefault(signature, []).append(observation.get("success") is True)
        signature, outcomes = max(counts.items(), key=lambda item: (len(item[1]), item[0]))
        repetition = len(outcomes)
        competence = sum(outcomes) / repetition
        return SkillCandidate(signature, repetition, competence, repetition >= min_repetition and competence >= min_competence)
