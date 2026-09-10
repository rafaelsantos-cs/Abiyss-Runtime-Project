from __future__ import annotations

import copy

import pytest

from abiyss.errors import ValidationError
from abiyss.models import Query, QueryState, QueryType


def make_snapshot() -> dict[str, object]:
    query = Query.new(
        query_type=QueryType.AQUERY,
        priority=10,
        payload={"tool_name": "noop", "arguments": {}},
        source="runtime",
    )
    return query.snapshot()


def test_valid_snapshot_round_trips_without_normalization() -> None:
    snapshot = make_snapshot()
    restored = Query.from_snapshot(snapshot)
    assert restored.snapshot() == snapshot


def test_snapshot_rejects_unknown_top_level_field() -> None:
    snapshot = make_snapshot()
    snapshot["future_field"] = "must not be silently accepted"
    with pytest.raises(ValidationError, match="unknown fields"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_missing_top_level_field() -> None:
    snapshot = make_snapshot()
    del snapshot["source"]
    with pytest.raises(ValidationError, match="missing fields"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_state_mismatch() -> None:
    snapshot = make_snapshot()
    snapshot["state"] = QueryState.COMPLETED.value
    with pytest.raises(ValidationError, match="state does not match"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_checkpoint_state_mismatch() -> None:
    snapshot = make_snapshot()
    checkpoint = snapshot["checkpoint"]
    assert isinstance(checkpoint, dict)
    checkpoint["state"] = QueryState.COMPLETED.value
    with pytest.raises(ValidationError, match="state does not match"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_attempt_mismatch() -> None:
    snapshot = make_snapshot()
    snapshot["attempts"] = 1
    with pytest.raises(ValidationError, match="attempts do not match"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_non_string_source_instead_of_coercing() -> None:
    snapshot = make_snapshot()
    snapshot["source"] = 123
    with pytest.raises(ValidationError, match="invalid query source"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_unknown_checkpoint_field() -> None:
    snapshot = make_snapshot()
    checkpoint = snapshot["checkpoint"]
    assert isinstance(checkpoint, dict)
    checkpoint["unexpected"] = True
    with pytest.raises(ValidationError, match="unknown fields"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_invalid_checkpoint_result_type() -> None:
    snapshot = make_snapshot()
    checkpoint = snapshot["checkpoint"]
    assert isinstance(checkpoint, dict)
    checkpoint["result"] = ["not", "an", "object"]
    with pytest.raises(ValidationError, match="checkpoint result"):
        Query.from_snapshot(snapshot)


def test_snapshot_rejects_non_finite_timestamp() -> None:
    snapshot = make_snapshot()
    snapshot["updated_at"] = float("nan")
    with pytest.raises(ValidationError, match="updated_at"):
        Query.from_snapshot(snapshot)


def test_snapshot_does_not_mutate_input() -> None:
    snapshot = make_snapshot()
    original = copy.deepcopy(snapshot)
    Query.from_snapshot(snapshot)
    assert snapshot == original
