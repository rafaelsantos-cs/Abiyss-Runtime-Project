from __future__ import annotations

import pytest

from abiyss.errors import SecurityError
from abiyss.skills import SkillLoader


class FakeSystemPlane:
    def __init__(self, response: dict[str, object]) -> None:
        self.response = response
        self.calls: list[str] = []

    def verify_skill(self, directory):
        self.calls.append(str(directory))
        return self.response


def verified_response(directory: str) -> dict[str, object]:
    return {
        "directory": directory,
        "name": "demo",
        "version": "1",
        "entrypoint": ["runner"],
        "files": {"runner": "0" * 64},
        "allow_root": False,
        "timeout_seconds": 10.0,
        "max_output_bytes": 65536,
        "max_args": 32,
    }


def test_loader_uses_rust_verification_without_reading_local_manifest(tmp_path) -> None:
    skill_dir = tmp_path / "skills" / "demo"
    skill_dir.mkdir(parents=True)
    (skill_dir / "runner").write_text("not actually verified here", encoding="utf-8")
    (skill_dir / "skill.json").write_text("not-json", encoding="utf-8")

    fake = FakeSystemPlane(verified_response(str(skill_dir.resolve())))
    loader = SkillLoader(tmp_path / "skills", system_plane=fake)

    directory, manifest = loader.load("demo")

    assert directory == skill_dir.resolve()
    assert manifest.name == "demo"
    assert manifest.entrypoint == ("runner",)
    assert fake.calls == [str(skill_dir.resolve())]


def test_loader_rejects_untrusted_verifier_response_directory(tmp_path) -> None:
    skill_dir = tmp_path / "skills" / "demo"
    skill_dir.mkdir(parents=True)
    response = verified_response(str(tmp_path / "elsewhere"))
    loader = SkillLoader(tmp_path / "skills", system_plane=FakeSystemPlane(response))

    with pytest.raises(SecurityError, match="unexpected skill directory"):
        loader.load("demo")


def test_verified_manifest_fields_are_strict() -> None:
    response = verified_response("/tmp/demo")
    response["unexpected"] = True
    with pytest.raises(SecurityError, match="response fields"):
        from abiyss.skills import SkillManifest

        SkillManifest.from_verified_response(response)
