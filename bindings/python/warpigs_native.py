"""Thin ctypes consumer for the canonical WarPigs C ABI.

This module is orchestration-only. It deliberately delegates all simulation
semantics to the native C++ engine and requires the library path to be supplied
explicitly via ``WARPIGS_NATIVE_LIB`` or ``Engine(library=...)``.
"""

from __future__ import annotations

import ctypes
import os
from pathlib import Path


class WarPigsError(RuntimeError):
    pass


class Engine:
    ABSOLUTE_MAX_POPULATION = 1_000_000
    ABI_VERSION = 1

    def __init__(self, population: int, max_population: int, seed: int, *, library: str | os.PathLike[str]) -> None:
        self._lib = ctypes.CDLL(str(Path(library)))
        handle = ctypes.c_void_p()
        self._configure_signatures()
        self._raise_for(self._lib.wp_engine_create(population, max_population, seed, ctypes.byref(handle)))
        if not handle:
            raise WarPigsError("native engine returned a null handle")
        self._handle = handle
        if self._lib.wp_engine_abi_version() != self.ABI_VERSION:
            self.close()
            raise WarPigsError("unsupported native ABI version")

    @classmethod
    def from_environment(cls, population: int, max_population: int, seed: int) -> "Engine":
        library = os.environ.get("WARPIGS_NATIVE_LIB")
        if not library:
            raise WarPigsError("WARPIGS_NATIVE_LIB is required")
        return cls(population, max_population, seed, library=library)

    def _configure_signatures(self) -> None:
        lib = self._lib
        lib.wp_engine_abi_version.argtypes = []
        lib.wp_engine_abi_version.restype = ctypes.c_uint32
        lib.wp_engine_create.argtypes = [ctypes.c_uint64, ctypes.c_uint64, ctypes.c_uint64, ctypes.POINTER(ctypes.c_void_p)]
        lib.wp_engine_create.restype = ctypes.c_uint32
        lib.wp_engine_destroy.argtypes = [ctypes.c_void_p]
        lib.wp_engine_destroy.restype = None
        lib.wp_engine_step.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        lib.wp_engine_step.restype = ctypes.c_uint32
        lib.wp_engine_tick.argtypes = [ctypes.c_void_p]
        lib.wp_engine_tick.restype = ctypes.c_uint64
        lib.wp_engine_population.argtypes = [ctypes.c_void_p]
        lib.wp_engine_population.restype = ctypes.c_uint64
        lib.wp_engine_configuration_code.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint8)]
        lib.wp_engine_configuration_code.restype = ctypes.c_uint32
        lib.wp_engine_configuration_text.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.POINTER(ctypes.c_char), ctypes.c_size_t]
        lib.wp_engine_configuration_text.restype = ctypes.c_uint32
        lib.wp_engine_lifecycle.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint32)]
        lib.wp_engine_lifecycle.restype = ctypes.c_uint32
        lib.wp_engine_configuration_count.argtypes = []
        lib.wp_engine_configuration_count.restype = ctypes.c_uint32

    @staticmethod
    def _raise_for(code: int) -> None:
        if code != 0:
            names = {
                1: "null pointer",
                2: "invalid argument",
                3: "limit exceeded",
                4: "index out of bounds",
                5: "invalid state transition",
                6: "buffer too small",
                255: "native internal error",
            }
            raise WarPigsError(names.get(code, f"unknown native error {code}"))

    def close(self) -> None:
        handle = getattr(self, "_handle", None)
        if handle:
            self._lib.wp_engine_destroy(handle)
            self._handle = None

    def __enter__(self) -> "Engine":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    @property
    def population(self) -> int:
        self._require_open()
        return int(self._lib.wp_engine_population(self._handle))

    @property
    def tick(self) -> int:
        self._require_open()
        return int(self._lib.wp_engine_tick(self._handle))

    @staticmethod
    def _action_code(action: str) -> int:
        try:
            return {"prepare": 0, "start": 1, "quarantine": 2, "terminate": 3}[action]
        except KeyError as exc:
            raise ValueError(f"unknown action: {action!r}") from exc

    def step(self, action: str) -> None:
        self._require_open()
        self._raise_for(self._lib.wp_engine_step(self._handle, self._action_code(action)))

    def configuration(self, index: int) -> str:
        self._require_open()
        out = ctypes.create_string_buffer(5)
        self._raise_for(self._lib.wp_engine_configuration_text(self._handle, index, out, len(out)))
        return out.value.decode("ascii")

    def configuration_code(self, index: int) -> int:
        self._require_open()
        out = ctypes.c_uint8()
        self._raise_for(self._lib.wp_engine_configuration_code(self._handle, index, ctypes.byref(out)))
        return int(out.value)

    def lifecycle(self, index: int) -> int:
        self._require_open()
        out = ctypes.c_uint32()
        self._raise_for(self._lib.wp_engine_lifecycle(self._handle, index, ctypes.byref(out)))
        return int(out.value)

    @property
    def configuration_count(self) -> int:
        self._require_open()
        return int(self._lib.wp_engine_configuration_count())

    def _require_open(self) -> None:
        if not getattr(self, "_handle", None):
            raise WarPigsError("engine is closed")
