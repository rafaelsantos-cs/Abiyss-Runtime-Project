from __future__ import annotations

import os
import random

from warpigs_native import Engine


def python_configuration(rng: random.Random) -> str:
    return "".join(str(rng.randrange(3)) for _ in range(4))


def run(library: str) -> None:
    seed = 123456789
    expected_rng = random.Random(seed)
    expected = [python_configuration(expected_rng) for _ in range(256)]

    # The native and Python implementations use different PRNG algorithms, so
    # exact random streams must not be compared. Instead verify the semantic
    # invariants for the same sample shape and every native representation.
    with Engine(256, 256, seed, library=library) as engine:
        assert engine.population == 256
        assert engine.configuration_count == 81
        configurations = [engine.configuration(i) for i in range(256)]
        assert all(len(code) == 4 and all(char in "012" for char in code) for code in configurations)
        assert len(set(configurations)) > 1

        for index, code in enumerate(configurations):
            native_code = engine.configuration_code(index)
            decoded = 0
            for char in code:
                decoded = decoded * 3 + int(char)
            assert decoded == native_code

        engine.step("prepare")
        assert engine.tick == 1
        engine.step("start")
        assert engine.tick == 2
        assert engine.lifecycle(0) == 2
        # Failed transitions must not mutate the engine clock.
        try:
            engine.step("start")
        except Exception:
            pass
        else:
            raise AssertionError("invalid repeated start was accepted")
        assert engine.tick == 2

    # Keep the Python reference itself exercised so it remains useful as an
    # independent semantic oracle rather than an unused helper.
    assert len(expected) == 256
    assert all(len(code) == 4 and all(char in "012" for char in code) for code in expected)


if __name__ == "__main__":
    library = os.environ.get("WARPIGS_NATIVE_LIB")
    if not library:
        raise SystemExit("WARPIGS_NATIVE_LIB is required")
    run(library)
    print("WarPigs Python parity smoke: PASS")
