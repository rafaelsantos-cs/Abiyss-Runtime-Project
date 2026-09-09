"""Sterile WarPigs research simulator.

This package models the four-position / three-state WarPig design as inert
Python objects. It intentionally has no self-replication, host execution,
network propagation, persistence, or destructive capabilities.
"""

from .model import ComponentState, WarPig, WarPigConfiguration
from .population import PopulationConfig, PopulationSupervisor

__all__ = [
    "ComponentState",
    "PopulationConfig",
    "PopulationSupervisor",
    "WarPig",
    "WarPigConfiguration",
]
