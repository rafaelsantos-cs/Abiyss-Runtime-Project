from pathlib import Path

import pytest

from abiyss.bridge import NativeWorker
from abiyss.errors import ValidationError


def test_native_worker_requires_absolute_executable() -> None:
    with pytest.raises(ValidationError):
        NativeWorker("relative-worker")


def test_native_worker_health_with_python_fixture(tmp_path: Path) -> None:
    worker = tmp_path / "worker.py"
    worker.write_text(
        "import json,sys\n"
        "for line in sys.stdin:\n"
        " r=json.loads(line)\n"
        " print(json.dumps({'version':1,'ok':True,'request_id':r['request_id'],'result':{'healthy':True}}), flush=True)\n",
        encoding="utf-8",
    )
    # The bridge requires an executable path and does not invoke a shell. Use the
    # interpreter explicitly so this test remains portable and deterministic.
    worker.chmod(0o755)
    interpreter = Path("/usr/bin/python3")
    if not interpreter.exists():
        pytest.skip("system Python interpreter unavailable")
    launcher = tmp_path / "launcher"
    launcher.write_text(f"#!{interpreter}\n" + worker.read_text(encoding="utf-8"), encoding="utf-8")
    launcher.chmod(0o755)
    result = NativeWorker(str(launcher)).call("health")
    assert result.ok
    assert result.result == {"healthy": True}
