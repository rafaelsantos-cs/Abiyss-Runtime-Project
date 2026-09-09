"""Bounded event engine for sterile WarPig simulation.

The engine is intentionally a pure in-memory simulation. "Running" means
advancing a simulated lifecycle and producing events, never spawning an OS
process or touching the network/filesystem.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from .lifecycle import LifecycleState, WarPigLifecycle
from .model import WarPig


class SimulationAction(str, Enum):
    PREPARE = "prepare"
    START = "start"
    QUARANTINE = "quarantine"
    TERMINATE = "terminate"


@dataclass(frozen=True, slots=True)
class SimulationEvent:
    """Stable event envelope emitted by the simulation engine."""

    tick: int
    identity: str
    action: SimulationAction
    state: LifecycleState


class SterileEngine:
    """Advance a fixed set of WarPig lifecycles under explicit supervision."""

    __slots__ = ("_lifecycles", "_tick")

    def __init__(self, pigs: tuple[WarPig, ...]) -> None:
        self._lifecycles = tuple(WarPigLifecycle(pig) for pig in pigs)
        self._tick = 0

    @property
    def tick(self) -> int:
        return self._tick

    @property
    def lifecycles(self) -> tuple[WarPigLifecycle, ...]:
        return self._lifecycles

    def advance(self, action: SimulationAction) -> tuple[SimulationEvent, ...]:
        """Apply one identical simulated action to all instances.

        No action can increase population. Invalid lifecycle transitions fail
        before an event is emitted for that instance.
        """
        if not isinstance(action, SimulationAction):
            raise TypeError("action must be a SimulationAction")
        self._tick += 1
        events: list[SimulationEvent] = []
        for lifecycle in self._lifecycles:
            if action is SimulationAction.PREPARE:
                lifecycle.prepare()
            elif action is SimulationAction.START:
                lifecycle.start()
            elif action is SimulationAction.QUARANTINE:
                lifecycle.quarantine(reason=f"simulation tick {self._tick}")
            elif action is SimulationAction.TERMINATE:
                lifecycle.terminate(reason=f"simulation tick {self._tick}")
            events.append(
                SimulationEvent(
                    tick=self._tick,
                    identity=lifecycle.pig.identity,
                    action=action,
                    state=lifecycle.state,
                )
            )
        return tuple(events)

    def population(self) -> int:
        """Return the fixed simulated population size."""
        return len(self._lifecycles)

    def state_counts(self) -> dict[LifecycleState, int]:
        """Return lifecycle-state counts without mutating simulation state."""
        counts = {state: 0 for state in LifecycleState}
        for lifecycle in self._lifecycles:
            counts[lifecycle.state] += 1
        return counts
