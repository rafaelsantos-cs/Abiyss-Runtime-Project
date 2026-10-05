"""Hardware backend: interface and stub for the future physical arm.

No physical driver is implemented here on purpose: the arm, the PWM board and
the camera have not been chosen, and inventing a driver without hardware and
documentation would be fiction. What exists:

* :class:`PWMDriver` - the minimal contract a servo PWM board must offer
  (e.g. a PCA9685 over I2C, a microcontroller over serial). Implement it when
  the board is chosen.
* :class:`RecordingPWMDriver` - an in-memory driver that records every pulse.
  It lets the hardware path (pulse quantisation, channel mapping, real-time
  pacing, missing feedback) be exercised in tests without hardware.
* :class:`HardwareBackend` - a :class:`RobotBackend` built on a PWMDriver.
  It reports *commanded* values only: genuine SG90/MG90S servos expose no
  position, torque or current feedback, so stall detection and closed-loop
  checks are unavailable unless sensors are added (see capabilities).
* :class:`HardwareCamera` - placeholder; a real camera adapter (V4L2, CSI)
  must implement :class:`CameraSensor`.
"""

from __future__ import annotations

import time
from abc import ABC, abstractmethod
from typing import Any

from .backend import BackendCapabilities, CameraFrame, CameraSensor, CapabilityNotSupported, RobotBackend, ServoChannel
from .specs import ArmSpecs


class PWMDriver(ABC):
    """Contract for a servo PWM generator."""

    @abstractmethod
    def set_pulse_us(self, channel: int, pulse_us: float) -> None: ...

    @abstractmethod
    def disable(self, channel: int) -> None:
        """Stop pulses on a channel (most analog servos then stop driving)."""

    def close(self) -> None:
        return None


class RecordingPWMDriver(PWMDriver):
    def __init__(self) -> None:
        self.log: list[tuple[float, int, float | None]] = []

    def set_pulse_us(self, channel: int, pulse_us: float) -> None:
        self.log.append((time.monotonic(), channel, float(pulse_us)))

    def disable(self, channel: int) -> None:
        self.log.append((time.monotonic(), channel, None))


class HardwareServo(ServoChannel):
    def __init__(self, name: str, channel: int, driver: PWMDriver) -> None:
        super().__init__(name)
        self.channel = channel
        self.driver = driver

    def configure(self, **params: Any) -> None:
        raise CapabilityNotSupported(
            "SG90/MG90S controller gains are fixed inside the servo electronics and cannot be changed from the host"
        )


class HardwareCamera(CameraSensor):
    def frame(self) -> CameraFrame:
        raise CapabilityNotSupported("no physical camera adapter configured (implement CameraSensor for the chosen camera)")


class HardwareBackend(RobotBackend):
    def __init__(self, specs: ArmSpecs, driver: PWMDriver, channel_map: dict[str, int], *, camera: CameraSensor | None = None, realtime: bool = True) -> None:
        missing = [n for n in [*specs.joint_names, "gripper"] if n not in channel_map]
        if missing:
            raise ValueError(f"channel_map missing servos: {missing}")
        self.specs = specs
        self.driver = driver
        self.realtime = realtime
        self._servos = {name: HardwareServo(name, ch, driver) for name, ch in channel_map.items()}
        self._camera = camera or HardwareCamera()
        self._t = 0.0
        self._next_deadline: float | None = None
        self.capabilities = BackendCapabilities(
            name="hardware",
            deterministic=False,
            position_feedback=False,
            torque_feedback=False,
            current_feedback=False,
            configurable_servo_controller=False,
            camera=camera is not None,
            ground_truth=False,
        )

    def connect(self) -> dict[str, Any]:
        return {"backend": "hardware", "driver": type(self.driver).__name__}

    def close(self) -> None:
        for s in self._servos.values():
            self.driver.disable(s.channel)
        self.driver.close()

    @property
    def servos(self) -> dict[str, ServoChannel]:
        return self._servos

    def reset(self, *, tool: str, seed: int) -> dict[str, Any]:
        self._t = 0.0
        return self._state()

    def advance(self, dt: float) -> dict[str, Any]:
        for s in self._servos.values():
            if s.powered:
                self.driver.set_pulse_us(s.channel, s.pulse_us)
            else:
                self.driver.disable(s.channel)
        if self.realtime:
            now = time.monotonic()
            if self._next_deadline is None:
                self._next_deadline = now
            self._next_deadline += dt
            delay = self._next_deadline - time.monotonic()
            if delay > 0:
                time.sleep(delay)
        self._t += dt
        return self._state()

    def _state(self) -> dict[str, Any]:
        joints = {}
        for name, s in self._servos.items():
            joints[name] = {"pulse_us": s.pulse_us, "powered": s.powered, "feedback": False}
        grip = joints.pop("gripper")
        return {"sim_time": self._t, "joints": joints, "gripper": grip, "feedback": False}

    def camera(self) -> CameraSensor:
        return self._camera
