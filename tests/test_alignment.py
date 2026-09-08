from __future__ import annotations

from abiyss.alignment import SleepKeyAligner


def test_baseline_groups_normalized_neighbors():
    result = SleepKeyAligner().baseline({"a": "hello world", "b": "hello   world", "c": "other"})
    assert any(set(group) == {"a", "b"} for group in result.groups)


def test_provider_cannot_invent_or_duplicate_keys():
    class Provider:
        def structured(self, _input, _schema):
            return {"groups": [["a", "evil", "a"], ["a", "b"]]}

    result = SleepKeyAligner(Provider()).align({"a": "x", "b": "y"})
    assert sorted(sum(result.groups, [])) == ["a", "b"]
