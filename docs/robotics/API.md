# Agent API

The agent says **what** to do; the controller and the physics decide **how**
it happens. No call writes a position into the world: every action becomes
trajectories, IK, pulse widths, servo torques and Jolt integration.

## 1. Python (`abiyss.robotics`)

```python
from abiyss.robotics import Robot

with Robot.simulation(tool="pen") as robot:          # starts the Godot lab headless
    robot.queue([{"action": "write", "text": "OI"}, {"action": "home"}])
    robot.run()                                       # deterministic, simulated time

    robot.arm.move_to(0.15, 0.0, 0.05)                # blocking (wait=True) by default
    robot.arm.move_joint("shoulder", 90)
    robot.arm.home()
    robot.arm.stop()                                  # controlled stop: cancel queue, hold
    robot.arm.stall_check()                           # torques requested/available/max per servo
    robot.arm.set_pid("elbow", kp=5.0, kd=0.01)       # simulation backend only
    robot.arm.step_response("elbow", 20.0)            # overshoot, settling, oscillation
    robot.arm.power("shoulder", False)                # unpowered joint: gravity acts

    robot.gripper.open()
    robot.gripper.close(width_mm=20)

    frame = robot.cam.frame()                          # CameraFrame: PNG + pose + intrinsics
    frame.save("/tmp/ee.png")

    robot.status()                                     # safety state, queue, joints, TCP
    robot.telemetry()                                  # per joint: target, position, error,
                                                       # velocity, torque, current, stall...
    robot.emergency_stop("reason"); robot.reset()      # latched E-STOP and explicit reset
```

`wait=False` only enqueues; `robot.run()` / `robot.wait(seconds)` advance
simulated time. With the hardware backend the same calls run in real time.

`Robot.with_backend(backend, specs)` builds the same API on any
`RobotBackend` (see `HARDWARE.md`).

## 2. Actions (queue vocabulary)

Validated with JSON schemas (`abiyss/robotics/actions.py`). Coordinates in
metres (robot frame: +x forward, +y left, +z up, origin at the base on the
bench top), angles in degrees.

| action | parameters | what happens |
|---|---|---|
| `move_to` | `x y z` [`pitch_deg`=-90, `speed`, `settle_s`] | IK -> joint-space min-jerk move; if the joint path would sweep the tool below 8 mm, lift -> cross at 70 mm -> descend |
| `move_line` | `x y z` [`pitch_deg`, `speed_mps`] | straight Cartesian line, IK every 20 ms |
| `move_joint` | `joint angle_deg` [`speed`] | one joint, soft limits enforced |
| `move_joints` | `angles_deg{}` [`speed`] | several joints together |
| `home` | [`speed`] | home pose from ARM_SPECS.json |
| `gripper_open` | [`width_mm`] | |
| `gripper_close` | [`width_mm`, `hold_s`] | a full close stalls the SG90 on the object (that is the grip) |
| `write` | `text` [`origin`, `letter_height`, `letter_width`, `spacing`, `speed_mps`] | pen tool; single-stroke font; pen-up transits in straight lines |
| `pick` | `x y` [`z`, `approach_height`, `label`] | open, approach, descend, close, lift |
| `place` | `x y` [`z`, `approach_height`, `open_width_mm`] | approach, descend, release, lift |
| `wait` | `seconds` | physics keeps running |
| `servo_power` | `joint on` | off = no torque, joint held only by gear friction |
| `set_pid` | `joint` [`kp ki kd`] | simulated servo electronics only |
| `step_response` | `joint delta_deg` [`duration_s`] | measures overshoot, rise/settling time, oscillations, steady-state error |

Unknown actions, extra fields, out-of-range values and unreachable targets
are rejected (schema / IK / soft limits) and journaled.

### Queue documents

```text
queue:
  write("OI")
  home()
```

or JSON `{"queue": [{"action": "write", "text": "OI"}, {"action": "home"}]}`.
The text form is parsed with `ast.literal_eval` on arguments only.

## 3. Command line

```bash
abiyss-robotics lab --queue 'write("OI")'                # 3D window (Xvfb if no display) + API service
abiyss-robotics run examples/robotics/write_oi.queue      # headless, prints the results
abiyss-robotics scenario all --out artifacts/robotics     # TEST 1-5 + extras (reports, SVG plots)
abiyss-robotics scenario all --window --screenshots       # also captures the lab screenshots
abiyss-robotics ctl status|telemetry|stall|estop|reset|stop
abiyss-robotics ctl frame /abs/path/frame.png
abiyss-robotics ctl enqueue 'pick(0.105, 0.095)'
abiyss-robotics specs --audit                             # verification status of every parameter
```

(`python -m abiyss.robotics ...` is equivalent.)

## 4. Robot API service (other processes)

`abiyss-robotics lab` exposes JSON lines on `127.0.0.1:47012`. Each request is
executed on the control thread at a 20 ms boundary.

```json
{"call": "status"}
{"call": "telemetry"}
{"call": "stall_check"}
{"call": "enqueue", "actions": [{"action": "write", "text": "OI"}]}
{"call": "frame", "path": "/abs/frame.png"}
{"call": "stop"} {"call": "emergency_stop", "reason": "..."} {"call": "reset"}
{"call": "queue"} {"call": "journal_tail"}
```

`abiyss.robotics.server.RobotClient` is the Python client.

## 5. Abiyss runtime tools

```python
from abiyss.robotics.runtime_tools import register_robot_tools
from abiyss.robotics.server import RobotClient
register_robot_tools(registry, RobotClient())
```

Registers Aquery tools `robot.enqueue`, `robot.status`, `robot.telemetry`,
`robot.stall_check`, `robot.camera_frame`, `robot.emergency_stop`,
`robot.reset`. A model function call can only *queue intent*; schema
validation happens again inside the tool before anything reaches the robot.

## 6. Safety states

`SAFE`, `WARNING`, `STALL`, `FAULT`, `EMERGENCY_STOP` - see
[ARCHITECTURE.md](../ARCHITECTURE.md#robotics-safety-state-machine). While
`STALL`, `FAULT` or `EMERGENCY_STOP` is latched every new command is rejected
(`command_rejected` in the journal) until `reset()`.

## 7. Telemetry journal (JSONL)

Same record format as the runtime `AuditLog`: one JSON object per line,
compact separators, **sorted keys**, `ts` (unix time) and `event`, secret-like
keys redacted. Robotics records add `source="abiyss.robotics"`, `run_id`,
`seq`, `sim_time`, `backend`. Discrete events can be mirrored into the runtime
audit log as `robotics.<event>`.

| event | key fields |
|---|---|
| `run_start` / `run_end` | backend info, capabilities, seed, tool / stall history |
| `command`, `command_rejected` | action, params, source (api, ui, cli, abiyss_runtime) / reason |
| `action_start`, `action_end` | action id, status (done/failed/cancelled), result, error |
| `move_joint` | joint, phase (start/end), target_deg, position_deg, velocity_dps, torque_nm |
| `joint_state` (10 Hz) | per joint: target/position/servo/error deg, velocity, torque, requested, max, current, powered, stalled; tcp_mm, tcp_cmd_mm |
| `servo_stall` | joint, requested_torque, maximum_torque, available_torque, position_deg, target_deg, static_torque_model, payload_lower_bound_kg, probable_cause, current_a, time |
| `servo_stall_cleared`, `servo_power_off` | |
| `safety_state` | from, to, reason |
| `emergency_stop`, `stop` | reason, mode, cancelled ids |
| `collision` | phase (begin/end), a, b |
| `gripper` | command, target/measured opening, finger force, held objects |
| `pick_result`, `place_result` | object, lift, held, placement error (ground truth in simulation) |
| `write_plan`, `write_stroke` | |
| `camera_frame` | frame id, size, pose, intrinsics |
| `pid_config`, `pid_metrics` | gains / overshoot, settling, oscillations, steady-state error |
| `path_replanned` | why and the safe height used |
| `servo_power`, `ui_event`, `backend_error`, `error` | |

Example (`servo_stall` from TEST 3):

```json
{"available_torque":0.17385,"backend":"simulation","current_a":0.7401,"event":"servo_stall","gripper_holding":true,"joint":"shoulder","maximum_torque":0.17652,"payload_lower_bound_kg":0.061,"position_deg":24.029,"probable_cause":"overload by payload: gripper is closed on an object; arm alone needs 0.079 N*m of 0.177; payload >= 61 g","requested_torque":0.49906,"run_id":"run_...","seq":583,"servo":"MG90S","sim_time":44.76,"source":"abiyss.robotics","static_torque_model":0.07853,"target_deg":38.328,"time":44.76,"ts":1791231409.78}
```

## 8. Godot bridge protocol (backend internals)

Python <-> Godot, JSON lines on 127.0.0.1 (port chosen by the backend):
`hello`, `reset {tool, seed}`, `step {ticks, servos: {name: {pulse_us, power}}}`,
`configure {servo, params}`, `state`, `ink`, `clear_ink`, `camera`,
`screenshot {path}`, `hud {status}`, `view {name}`, `quit`. Only `step`
advances physics; replies to `step` carry the full plant telemetry and HUD
button events.
