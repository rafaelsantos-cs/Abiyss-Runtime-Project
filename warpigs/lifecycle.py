"""Host-free lifecycle state machine for sterile WarPig simulations.

This module models hostile-looking lifecycle transitions as data only. It does
not create OS processes, execute commands, access the network, persist itself,
or mutate ABIYSS runtime state.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from .model import WarPig


class LifecycleState(str, Enum):
    """States of one simulated WarPig instance."""

    CREATED = "created"
    READY = "ready"
    RUNNING = "running"
    QUARANTINED = "quarantined"
    TERMINATED = "terminated"


class LifecycleError(RuntimeError):
    """Raised when a lifecycle transition violates the state machine."""


_ALLOWED: dict[LifecycleState, frozenset[LifecycleState]] = {
    LifecycleState.CREATED: frozenset({LifecycleState.READY, LifecycleState.TERMINATED}),
    LifecycleState.READY: frozenset({LifecycleState.RUNNING, LifecycleState.QUARANTINED, LifecycleState.TERMINATED}),
    LifecycleState.RUNNING: frozenset({LifecycleState.QUARANTINED, LifecycleState.TERMINATED}),
    LifecycleState.QUARANTINED: frozenset({LifecycleState.TERMINATED}),
    LifecycleState.TERMINATED: frozenset(),
}


@dataclass(frozen=True, slots=True)
class LifecycleEvent:
    """Immutable transition record suitable for a simulator event log."""

    sequence: int
    previous: LifecycleState
    current: LifecycleState
    reason: str


class WarPigLifecycle:
    """Explicit finite-state machine owned by a single simulated WarPig."""

    __slots__ = ("_pig", "_state", "_sequence", "_events")

    def __init__(self, pig: WarPig) -> None:
        self._pig = pig
        self._state = LifecycleState.CREATED
        self._sequence = 0
        self._events: list[LifecycleEvent] = []

    @property
    def pig(self) -> WarPig:
        return self._pig

    @property
    def state(self) -> LifecycleState:
        return self._state

    @property
    def events(self) -> tuple[LifecycleEvent, ...]:
        return tuple(self._events)

    def transition(self, target: LifecycleState, *, reason: str) -> LifecycleEvent:
        """Apply one validated transition and return its immutable event."""
        if not isinstance(target, LifecycleState):
            raise TypeError("target must be a LifecycleState")
        if not isinstance(reason, str) or not reason or len(reason) > 256:
            raise ValueError("reason must contain 1..256 characters")
        if target not in _ALLOWED[self._state]:
            raise LifecycleError(
                f"invalid transition: {self._state.value} -> {target.value}"
            )
        self._sequence += 1
        event = LifecycleEvent(self._sequence, self._state, target, reason)
        self._state = target
        self._events.append(event)
        return event

    def prepare(self) -> LifecycleEvent:
        return self.transition(LifecycleState.READY, reason="prepared")

    def start(self) -> LifecycleEvent:
        if not self._pig.reproductive:
            return self.transition(LifecycleState.RUNNING, reason="started sterile simulation")
        raise LifecycleError("reproductive WarPig instances are forbidden")

    def quarantine(self, *, reason: str = "supervisor quarantine") -> LifecycleEvent:
        return self.transition(LifecycleState.QUARANTINED, reason=reason)

    def terminate(self, *, reason: str = "supervisor termination") -> LifecycleEvent:
        return self.transition(LifecycleState.TERMINATED, reason=reason)

    def snapshot(self) -> dict[str, object]:
        return {
            "identity": self._pig.identity,
            "configuration": self._pig.configuration_code,
            "reproductive": False,
            "state": self._state.value,
            "sequence": self._sequence,
        }
