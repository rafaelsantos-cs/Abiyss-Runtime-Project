from __future__ import annotations

import sys
import types

import pytest

from abiyss.errors import ProviderError
from abiyss.gemini import DeterministicProvider, GoogleGeminiProvider
from abiyss.models import QueryType, ToolSpec


def test_google_interactions_function_call_parsing(monkeypatch):
    class Step:
        type = "function_call"
        id = "fc-1"
        name = "system.info"
        arguments = {"x": 1}

    class Interaction:
        id = "interaction-1"
        steps = [Step()]
        output_text = ""

    class Client:
        class interactions:
            @staticmethod
            def create(**kwargs):
                assert kwargs["model"] == "gemini-3.8-flash"
                assert kwargs["previous_interaction_id"] == "prior"
                assert kwargs["generation_config"] == {"thinking_level": "medium"}
                return Interaction()

    google = types.ModuleType("google")
    google_genai = types.ModuleType("google.genai")
    google_genai.Client = lambda **_: Client()
    google.genai = google_genai
    monkeypatch.setitem(sys.modules, "google", google)
    monkeypatch.setitem(sys.modules, "google.genai", google_genai)

    provider = GoogleGeminiProvider()
    turn = provider.turn("hello", [ToolSpec("system.info", "", QueryType.AQUERY, {"type": "object"})], "prior")
    assert turn.interaction_id == "interaction-1"
    assert turn.function_calls[0].id == "fc-1"
    assert turn.function_calls[0].arguments == {"x": 1}


def test_google_rejects_malformed_function_arguments(monkeypatch):
    class Step:
        type = "function_call"
        id = "fc-1"
        name = "system.info"
        arguments = "{bad"

    class Interaction:
        id = "interaction-1"
        steps = [Step()]
        output_text = ""

    class Client:
        class interactions:
            @staticmethod
            def create(**_):
                return Interaction()

    google = types.ModuleType("google")
    google_genai = types.ModuleType("google.genai")
    google_genai.Client = lambda **_: Client()
    google.genai = google_genai
    monkeypatch.setitem(sys.modules, "google", google)
    monkeypatch.setitem(sys.modules, "google.genai", google_genai)

    with pytest.raises(ProviderError):
        GoogleGeminiProvider().turn("hello", [], None)


def test_deterministic_provider_roundtrip():
    provider = DeterministicProvider(
        lambda value, _tools, previous: {
            "interaction_id": previous or "x",
            "output_text": "ok",
            "function_calls": [],
        }
    )
    assert provider.turn("input", []).output_text == "ok"
