from __future__ import annotations

import pytest

from warpigs import PopulationConfig, PopulationSupervisor
from warpigs.engine import SimulationAction, SterileEngine
from warpigs.lifecycle import LifecycleState


def test_engine_advances_without_changing_population() -> None:
    supervisor = PopulationSupervisor(PopulationConfig(8), seed=23)
    pigs = supervisor.create_population()
    engine = SterileEngine(pigs)

    assert engine.population() == 8
    assert engine.tick == 0

    engine.advance(SimulationAction.PREPARE)
    engine.advance(SimulationAction.START)

    assert engine.tick == 2
    assert engine.population() == 8
    assert all(lifecycle.state is LifecycleState.RUNNING for lifecycle in engine.lifecycles)


def test_engine_quarantine_then_terminate() -> None:
    supervisor = PopulationSupervisor(PopulationConfig(3), seed=31)
    engine = SterileEngine(supervisor.create_population())
    engine.advance(SimulationAction.PREPARE)
    engine.advance(SimulationAction.START)
    events = engine.advance(SimulationAction.QUARANTINE)

    assert len(events) == 3
    assert all(event.state is LifecycleState.QUARANTINED for event in events)

    engine.advance(SimulationAction.TERMINATE)
    assert engine.state_counts()[LifecycleState.TERMINATED] == 3


def test_invalid_action_type_is_rejected() -> None:
    supervisor = PopulationSupervisor(PopulationConfig(1), seed=41)
    engine = SterileEngine(supervisor.create_population())

    with pytest.raises(TypeError):
        engine.advance("start")  # type: ignore[arg-type]

    assert engine.tick == 0
    assert engine.population() == 1
