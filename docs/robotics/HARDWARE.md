# From simulation to the physical arm

```text
ABIYSS RUNTIME ──► ROBOT API (RobotController, queue, IK, safety, journal)
                        │
          ┌─────────────┴──────────────┐
  SimulationBackend              HardwareBackend
  SimulationServo                HardwareServo ──► PWMDriver ──► servos
  SimulationCamera               CameraSensor (to be written for the real camera)
  (Godot/Jolt lab)
```

The controller, the action queue, IK/trajectories, the safety state machine,
the telemetry journal, the Robot API service and the runtime tools do not
change when the backend changes. What a backend must provide
(`abiyss/robotics/backend.py`):

| Interface | Contract |
|---|---|
| `ServoChannel.set_pulse_us(us)` / `set_power(on)` | exactly what an RC servo accepts: one pulse width per 20 ms frame, and power/signal on-off |
| `ServoChannel.configure(**params)` | may raise `CapabilityNotSupported` (real SG90/MG90S gains are fixed) |
| `RobotBackend.advance(dt)` | hold the current commands for `dt` and return the observed state (simulation: lockstep physics; hardware: real-time pacing) |
| `RobotBackend.camera().frame()` | `CameraFrame` (PNG bytes + pose + intrinsics) |
| `capabilities` | tells the controller what can be observed (position, torque, current feedback, camera, ground truth) |

## What exists today

* `HardwareBackend(specs, driver, channel_map)` - real-time pacing, pulse
  output through a `PWMDriver`, signals disabled on close, reports
  `position_feedback/torque_feedback/current_feedback = false`.
* `PWMDriver` - two methods: `set_pulse_us(channel, us)` and `disable(channel)`.
* `RecordingPWMDriver` - records every pulse; used by the unit tests to
  exercise the hardware path (channel mapping, quantisation, pacing) without
  hardware.
* `HardwareCamera` - raises `CapabilityNotSupported` until a real camera
  adapter exists.

**No physical driver was written**: the PWM board, the arm and the camera
have not been chosen, and writing a driver without the hardware and its
documentation would be fiction.

## What the real arm will need (findings from the simulator)

1. **Feedback, or no safety.** Genuine SG90/MG90S give no position, torque or
   current. Stall detection, tracking-error warnings and "hold the measured
   pose after a stall" need at least one of: servo current sensing per
   channel (e.g. a shunt + ADC), the servo potentiometer wired out, or joint
   encoders. Without them `stall_check()` returns `available: false`.
2. **PWM resolution matters.** A PCA9685 at 50 Hz has 4.88 us steps
   (~0.9 deg for 1000 us = 180 deg) - five times coarser than the 1 us used
   in the simulator and as large as the MG90S dead band. Set
   `simulation.pwm_resolution_us` to the real value and rerun TEST 1/4.
3. **Measure the estimated parameters** (backlash, gear friction, stiffness,
   reflected inertia, pulse jitter, real dead band, stall current) and put
   the measured values with `"status": "measured"` sources in ARM_SPECS.json.
   Suggested bench tests: hanging-mass sag curve (stiffness, Kp equivalent),
   reversal test with a dial gauge (backlash), unpowered back-drive torque
   (Coulomb friction), stall current on a locked horn.
4. **Power.** Four MG90S/SG90 stalling at once can draw ~3 A at 4.8-6 V; the
   simulator does not model supply sag.
5. **Calibrate `servo_center_deg` and `direction`** per joint after
   assembly (horn spline offset is up to half a tooth: 360/21 or 360/25 deg).
