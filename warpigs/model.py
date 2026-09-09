"""Core inert WarPig model.

A WarPig is represented as four component states. The model is deliberately
not an operating-system process model: components are values only.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import IntEnum
from itertools import product

POSITION_COUNT = 4
CONFIGURATION_COUNT = 3**POSITION_COUNT


class ComponentState(IntEnum):
    """State assigned to one of the four WarPig positions."""

    MASKED = 0
    ACTIVE = 1
    STERILE_UTERUS = 2


@dataclass(frozen=True, slots=True)
class WarPigConfiguration:
    """Immutable four-position configuration.

    Exactly 81 configurations exist because every position has three possible
    states and positions are ordered.
    """

    positions: tuple[ComponentState, ComponentState, ComponentState, ComponentState]

    def __post_init__(self) -> None:
        if len(self.positions) != POSITION_COUNT:
            raise ValueError(f"a WarPig requires exactly {POSITION_COUNT} positions")
        if any(not isinstance(state, ComponentState) for state in self.positions):
            raise TypeError("positions must contain ComponentState values")

    @classmethod
    def from_digits(cls, digits: str) -> "WarPigConfiguration":
        if len(digits) != POSITION_COUNT or any(ch not in "012" for ch in digits):
            raise ValueError("configuration must be exactly four digits from 0, 1, or 2")
        return cls(tuple(ComponentState(int(ch)) for ch in digits))  # type: ignore[arg-type]

    def encode(self) -> str:
        return "".join(str(int(state)) for state in self.positions)

    @property
    def active_count(self) -> int:
        return self.positions.count(ComponentState.ACTIVE)

    @property
    def sterile_uterus_count(self) -> int:
        return self.positions.count(ComponentState.STERILE_UTERUS)

    @property
    def masked_count(self) -> int:
        return self.positions.count(ComponentState.MASKED)


@dataclass(frozen=True, slots=True)
class WarPig:
    """One inert WarPig instance.

    ``identity`` is an identifier for simulation telemetry only. No method on
    this object can create another WarPig or interact with the host system.
    """

    identity: str
    configuration: WarPigConfiguration

    def __post_init__(self) -> None:
        if not self.identity or len(self.identity) > 128:
            raise ValueError("identity must contain 1..128 characters")

    @property
    def configuration_code(self) -> str:
        return self.configuration.encode()

    @property
    def reproductive(self) -> bool:
        """Always false: state 2 is structurally present but sterile."""
        return False

    def snapshot(self) -> dict[str, object]:
        return {
            "identity": self.identity,
            "configuration": self.configuration_code,
            "reproductive": False,
        }


def all_configurations() -> tuple[WarPigConfiguration, ...]:
    """Return every one of the 81 ordered configurations."""
    return tuple(WarPigConfiguration(tuple(states)) for states in product(ComponentState, repeat=POSITION_COUNT))  # type: ignore[arg-type]
