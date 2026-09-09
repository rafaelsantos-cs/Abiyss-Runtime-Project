from __future__ import annotations

import pytest

from warpigs import (
    ComponentState,
    PopulationConfig,
    PopulationSupervisor,
    WarPig,
    WarPigConfiguration,
    all_configurations,
)


def test_configuration_space_is_exactly_81() -> None:
    configurations = all_configurations()
    assert len(configurations) == 81
    assert len({configuration.encode() for configuration in configurations}) == 81
    assert configurations[0].encode() == "0000"
    assert configurations[-1].encode() == "2222"


def test_configuration_rejects_invalid_values() -> None:
    with pytest.raises(ValueError):
        WarPigConfiguration.from_digits("000")
    with pytest.raises(ValueError):
        WarPigConfiguration.from_digits("0003")
    with pytest.raises(ValueError):
        WarPigConfiguration.from_digits("00000")


def test_uterus_is_structurally_present_but_sterile() -> None:
    configuration = WarPigConfiguration.from_digits("2012")
    assert configuration.sterile_uterus_count == 2
    assert configuration.active_count == 1
    assert configuration.masked_count == 1

    pig = WarPig("test-pig", configuration)
    assert pig.reproductive is False
    assert pig.snapshot() == {
        "identity": "test-pig",
        "configuration": "2012",
        "reproductive": False,
    }


def test_supervisor_controls_population() -> None:
    supervisor = PopulationSupervisor(PopulationConfig(size=128), seed=42)
    population = supervisor.create_population()

    assert len(population) == 128
    assert supervisor.size == 128
    assert all(not pig.reproductive for pig in population)
    assert all(len(pig.configuration.positions) == 4 for pig in population)
    assert all(
        all(isinstance(state, ComponentState) for state in pig.configuration.positions)
        for pig in population
    )

    supervisor.terminate()
    assert supervisor.size == 0


def test_population_limit_is_bounded() -> None:
    with pytest.raises(ValueError):
        PopulationConfig(size=10_001, max_size=10_000)
    with pytest.raises(ValueError):
        PopulationConfig(size=-1)


def test_seed_makes_configuration_assignment_reproducible() -> None:
    first = PopulationSupervisor(PopulationConfig(size=64), seed=123).create_population()
    second = PopulationSupervisor(PopulationConfig(size=64), seed=123).create_population()

    assert [pig.configuration_code for pig in first] == [pig.configuration_code for pig in second]


def test_histogram_preserves_population() -> None:
    supervisor = PopulationSupervisor(PopulationConfig(size=256), seed=7)
    supervisor.create_population()
    before = supervisor.population

    histogram = supervisor.configuration_histogram()

    assert sum(histogram.values()) == 256
    assert supervisor.population == before
