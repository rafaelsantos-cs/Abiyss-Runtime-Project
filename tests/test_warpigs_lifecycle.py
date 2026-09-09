from __future__ import annotations

import pytest

from warpigs import PopulationConfig, PopulationSupervisor
from warpigs.lifecycle import LifecycleError, LifecycleState, WarPigLifecycle


def test_lifecycle_has_explicit_terminal_path() -> None:
    population = PopulationSupervisor(PopulationConfig(1), seed=7)
    pig = population.create_population()[0]
    lifecycle = WarPigLifecycle(pig)

    assert lifecycle.state is LifecycleState.CREATED
    lifecycle.prepare()
    lifecycle.start()
    lifecycle.quarantine(reason="test containment")
    lifecycle.terminate(reason="test shutdown")

    assert lifecycle.state is LifecycleState.TERMINATED
    assert [event.sequence for event in lifecycle.events] == [1, 2, 3, 4]
    assert lifecycle.snapshot()["reproductive"] is False


def test_invalid_transition_is_fail_closed() -> None:
    population = PopulationSupervisor(PopulationConfig(1), seed=11)
    pig = population.create_population()[0]
    lifecycle = WarPigLifecycle(pig)

    with pytest.raises(LifecycleError):
        lifecycle.start()

    assert lifecycle.state is LifecycleState.CREATED
    assert lifecycle.events == ()


def test_terminated_instance_cannot_reactivate() -> None:
    population = PopulationSupervisor(PopulationConfig(1), seed=13)
    pig = population.create_population()[0]
    lifecycle = WarPigLifecycle(pig)
    lifecycle.terminate()

    with pytest.raises(LifecycleError):
        lifecycle.prepare()

    assert lifecycle.state is LifecycleState.TERMINATED
    assert len(lifecycle.events) == 1
