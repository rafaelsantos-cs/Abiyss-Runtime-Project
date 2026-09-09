from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
NATIVE = ROOT / "native" / "warpigs"


def test_native_engine_contains_no_host_capabilities() -> None:
    text = "\n".join(path.read_text(encoding="utf-8") for path in NATIVE.rglob("*.cpp"))
    forbidden = (
        "fork(", "vfork(", "execve(", "execl(", "system(", "popen(",
        "socket(", "connect(", "bind(", "listen(", "dlopen(",
        "unlink(", "remove(", "rename(",
    )
    for token in forbidden:
        assert token not in text, f"native WarPigs capability leaked into simulator: {token}"


def test_native_public_header_has_no_pointer_returned_buffers() -> None:
    header = (NATIVE / "include" / "warpigs.h").read_text(encoding="utf-8")
    assert "char **" not in header
    assert "void **" not in header
    assert "std::" not in header


def test_warpigs_package_has_no_python_process_or_socket_imports() -> None:
    text = "\n".join(path.read_text(encoding="utf-8") for path in (ROOT / "warpigs").rglob("*.py"))
    assert "import subprocess" not in text
    assert "import socket" not in text
    assert "os.system(" not in text
