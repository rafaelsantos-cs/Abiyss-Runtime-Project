from __future__ import annotations

from pathlib import Path

import pytest

from abiyss.errors import SecurityError
from abiyss.memory import MAX_SOURCES_PER_RECAP, MemoryStore


def test_context_recursion_is_rejected(tmp_path: Path) -> None:
    memory = MemoryStore(tmp_path / "memory.sqlite3")
    context: dict[str, object] = {}
    cursor = context
    for _ in range(64):
        child: dict[str, object] = {}
        cursor["child"] = child
        cursor = child
    with pytest.raises(SecurityError):
        memory.contextual_key("deep", context)
    memory.close()


def test_recap_rejects_source_loss(tmp_path: Path) -> None:
    memory = MemoryStore(tmp_path / "memory.sqlite3")
    sources = [f"q-{index}" for index in range(MAX_SOURCES_PER_RECAP + 1)]
    with pytest.raises(SecurityError):
        memory.create_immutable_recap(sources, "summary")
    memory.close()


def test_recap_sources_are_indexed_and_idempotent(tmp_path: Path) -> None:
    memory = MemoryStore(tmp_path / "memory.sqlite3")
    key_id = memory.daily_key(1_700_000_000)
    memory.add_memory(key_id, "first", "q-1")
    memory.add_memory(key_id, "second", "q-2")

    fingerprint = memory.create_immutable_recap(["q-1", "q-2"], "summary")
    assert fingerprint
    assert memory.unrecapped_memories(limit=10) == []

    fingerprint_again = memory.create_immutable_recap(["q-1", "q-2"], "summary")
    assert fingerprint_again == fingerprint
    assert len(memory.recaps()) == 1
    memory.close()


def test_invalid_key_ids_are_rejected(tmp_path: Path) -> None:
    memory = MemoryStore(tmp_path / "memory.sqlite3")
    with pytest.raises(SecurityError):
        memory.add_memory(0, "x", "q-1")
    with pytest.raises(SecurityError):
        memory.add_relation(0, 1, "related")
    memory.close()
