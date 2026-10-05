"""Backend abstraction: the seam between the Robot API and the world.

``RobotController`` only talks to :class:`RobotBackend`, :class:`ServoChannel`
and :class:`CameraSensor`. ``SimulationBackend`` (Godot/Jolt) and
``HardwareBackend`` implement them; swapping one for the other does not change
the controller, the queue, the safety logic, the journal or the Abiyss
runtime tools.

The servo interface is deliberately the one a real RC servo offers: a pulse
width (us) every PWM period and power on/off. Anything richer (true joint
angle, torque, current, controller gains) is an optional capability.
"""

from __future__ import annotations

import base64
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from ..errors import AbiyssError


class BackendError(AbiyssError):
    """The backend failed (process died, timeout, protocol error)."""


class CapabilityNotSupported(AbiyssError):
    """The backend cannot perform this operation (e.g. PID gains on a real SG90)."""


@dataclass(frozen=True, slots=True)
class BackendCapabilities:
    name: str
    deterministic: bool              # same commands + seed -> same result
    position_feedback: bool          # measured joint angles available
    torque_feedback: bool            # requested/applied torque available
    current_feedback: bool           # servo current available
    configurable_servo_controller: bool   # Kp/Ki/Kd can be changed
    camera: bool
    ground_truth: bool               # object poses, contacts, ink (simulation only)

    def as_dict(self) -> dict[str, Any]:
        return {k: getattr(self, k) for k in self.__slots__}  # type: ignore[attr-defined]


@dataclass(slots=True)
class CameraFrame:
    frame_id: int
    sim_time: float
    width: int
    height: int
    format: str
    data: bytes
    pose: dict[str, Any] = field(default_factory=dict)
    intrinsics: dict[str, Any] = field(default_factory=dict)
    sensor: str = "camera"

    def save(self, path: str | Path) -> Path:
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(self.data)
        return p

    def to_dict(self, include_data: bool = False) -> dict[str, Any]:
        out = {
            "frame_id": self.frame_id,
            "sim_time": self.sim_time,
            "width": self.width,
            "height": self.height,
            "format": self.format,
            "bytes": len(self.data),
            "pose": self.pose,
            "intrinsics": self.intrinsics,
            "sensor": self.sensor,
        }
        if include_data:
            out["data_base64"] = base64.b64encode(self.data).decode("ascii")
        return out


class ServoChannel(ABC):
    """One RC servo output: pulse width + power."""

    def __init__(self, name: str) -> None:
        self.name = name
        self.pulse_us = 1500.0
        self.powered = True

    def set_pulse_us(self, pulse_us: float) -> None:
        self.pulse_us = float(pulse_us)

    def set_power(self, on: bool) -> None:
        self.powered = bool(on)

    @abstractmethod
    def configure(self, **params: Any) -> None:
        """Change servo controller parameters (only where the hardware allows)."""


class CameraSensor(ABC):
    @abstractmethod
    def frame(self) -> CameraFrame:
        """Capture one frame now."""


class RobotBackend(ABC):
    capabilities: BackendCapabilities

    @abstractmethod
    def connect(self) -> dict[str, Any]:
        """Start/attach; return backend information."""

    @abstractmethod
    def close(self) -> None: ...

    @abstractmethod
    def reset(self, *, tool: str, seed: int) -> dict[str, Any]:
        """Bring the world to its initial state; return the first state sample."""

    @property
    @abstractmethod
    def servos(self) -> dict[str, ServoChannel]: ...

    @abstractmethod
    def advance(self, dt: float) -> dict[str, Any]:
        """Hold the current servo commands for ``dt`` seconds and return the
        observed state. Simulation: lockstep physics. Hardware: real time."""

    @abstractmethod
    def camera(self) -> CameraSensor: ...

    # Optional presentation hooks (no-ops by default) -------------------------

    def hud(self, status: dict[str, Any]) -> None:
        return None

    def screenshot(self, path: str | Path) -> Path:
        raise CapabilityNotSupported("screenshots are not available on this backend")

    def set_view(self, name: str) -> None:
        return None

    def ink(self) -> list[list[list[float]]]:
        raise CapabilityNotSupported("ink ground truth is only available in simulation")
