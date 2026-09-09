from __future__ import annotations

import os

from warpigs_native import Engine, WarPigsError

library = os.environ.get("WARPIGS_NATIVE_LIB")
if not library:
    raise SystemExit("WARPIGS_NATIVE_LIB is required")

with Engine(128, 10_000, 42, library=library) as engine:
    assert engine.configuration_count == 81
    assert engine.population == 128
    config = engine.configuration(0)
    assert len(config) == 4
    assert all(char in "012" for char in config)
    assert engine.configuration_code(0) < 81
    engine.step("prepare")
    engine.step("start")
    assert engine.tick == 2
    assert engine.lifecycle(0) == 2

try:
    Engine(1_000_001, 1_000_001, 1, library=library)
except WarPigsError:
    pass
else:
    raise AssertionError("native population limit was not enforced")

print("WarPigs Python native smoke: PASS")
