"""Abiyss robotics: Robot API, controller and backends (simulation / hardware).

    ABIYSS RUNTIME -> ROBOT API -> SIMULATION BACKEND -> PHYSICS ENGINE -> 3D ROBOT
    ABIYSS RUNTIME -> ROBOT API -> HARDWARE BACKEND   -> SERVOS
"""

from .actions import ACTION_DESCRIPTIONS, ACTION_SCHEMAS, validate_action
from .api import Robot
from .backend import BackendCapabilities, BackendError, CameraFrame, CameraSensor, CapabilityNotSupported, RobotBackend, ServoChannel
from .controller import RobotController
from .hardware import HardwareBackend, HardwareCamera, HardwareServo, PWMDriver, RecordingPWMDriver
from .kinematics import ArmKinematics, IKError
from .safety import SafetyState, SafetySupervisor
from .sim_backend import SimulationBackend, SimulationCamera, SimulationServo
from .specs import ArmSpecs, load_specs
from .telemetry import TelemetryJournal

__all__ = [
    "ACTION_DESCRIPTIONS",
    "ACTION_SCHEMAS",
    "ArmKinematics",
    "ArmSpecs",
    "BackendCapabilities",
    "BackendError",
    "CameraFrame",
    "CameraSensor",
    "CapabilityNotSupported",
    "HardwareBackend",
    "HardwareCamera",
    "HardwareServo",
    "IKError",
    "PWMDriver",
    "RecordingPWMDriver",
    "Robot",
    "RobotBackend",
    "RobotController",
    "SafetyState",
    "SafetySupervisor",
    "ServoChannel",
    "SimulationBackend",
    "SimulationCamera",
    "SimulationServo",
    "TelemetryJournal",
    "load_specs",
    "validate_action",
]
