"""External population control for the sterile WarPig simulator."""

from __future__ import annotations

from dataclasses import dataclass
import random
from uuid import uuid4

from .model import ComponentState, WarPig, WarPigConfiguration, POSITION_COUNT


DEFAULT_MAX_POPULATION = 10_000


@dataclass(frozen=True, slots=True)
class PopulationConfig:
    """Bounded parameters for one simulation population."""

    size: int
    max_size: int = DEFAULT_MAX_POPULATION

    def __post_init__(self) -> None:
        if not isinstance(self.size, int) or isinstance(self.size, bool):
            raise TypeError("size must be an integer")
        if not isinstance(self.max_size, int) or isinstance(self.max_size, bool):
            raise TypeError("max_size must be an integer")
        if self.max_size < 1:
            raise ValueError("max_size must be positive")
        if self.size < 0 or self.size > self.max_size:
            raise ValueError(f"size must be between 0 and {self.max_size}")


class PopulationSupervisor:
    """Creates and owns a fixed, inert population.

    The supervisor is the only population source. A WarPig has no operation
    that can create, clone, launch, or discover another WarPig.
    """

    __slots__ = ("_pigs", "_config", "_rng")

    def __init__(self, config: PopulationConfig, *, seed: int | None = None) -> None:
        self._config = config
        self._rng = random.Random(seed)
        self._pigs: tuple[WarPig, ...] = ()

    @property
    def population(self) -> tuple[WarPig, ...]:
        return self._pigs

    @property
    def size(self) -> int:
        return len(self._pigs)

    def create_population(self) -> tuple[WarPig, ...]:
        """Create exactly the configured number of inert instances."""
        pigs = []
        for _ in range(self._config.size):
            states = tuple(
                self._rng.choice(tuple(ComponentState))
                for _ in range(POSITION_COUNT)
            )
            pigs.append(
                WarPig(
                    identity=f"wp-{uuid4().hex}",
                    configuration=WarPigConfiguration(states),  # type: ignore[arg-type]
                )
            )
        self._pigs = tuple(pigs)
        return self._pigs

    def configuration_histogram(self) -> dict[str, int]:
        """Count configurations without changing the population."""
        histogram: dict[str, int] = {}
        for pig in self._pigs:
            histogram[pig.configuration_code] = histogram.get(pig.configuration_code, 0) + 1
        return histogram

    def terminate(self) -> None:
        """Drop all simulated instances from the supervisor."""
        self._pigs = ()
