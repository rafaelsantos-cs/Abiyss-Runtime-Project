# Tests

Three layers, none of which needs hardware.

| Layer | Where | Needs | Runtime (this VM) |
|---|---|---|---|
| Python unit tests | `tests/test_robotics_unit.py` | Python only | < 1 s |
| Godot unit/plant tests | `robot_lab/tests/run_tests.gd` | Godot 4.7 (headless) | ~20 s |
| Integration (Python <-> Godot, physics) | `tests/test_robotics_sim.py` (`-m sim`) | Godot; Xvfb for the camera test | ~2 min |
| Validation scenarios (TEST 1-5 + extras) | `abiyss-robotics scenario all` | Godot; `--window --screenshots` needs Xvfb or a display | ~1.5 min headless, ~10 min windowed |

```bash
PYTHONPATH=src python -m pytest -q tests/test_robotics_unit.py
godot --headless --path robot_lab --script res://tests/run_tests.gd
PYTHONPATH=src python -m pytest -q -m sim                  # skipped without Godot
PYTHONPATH=src python -m abiyss.robotics scenario all --out artifacts/robotics
```

`ABIYSS_GODOT=/path/to/godot` selects the engine binary;
`robot_lab/tools/install_godot.sh` installs the validated build after checking
its SHA-512.

## Coverage of the requested areas

| Area | Tests |
|---|---|
| limits | `test_pulses_are_quantised_and_limited` (soft limits reject), `test_ik_rejects_unreachable_and_reports_why`, `test_workspace_fixtures_are_reachable`, Godot `test_hard_stops` (mechanical stop reached and never passed) |
| PID | Godot `test_deadband`, `test_torque_envelope`; `test_step_metrics_on_known_response` (overshoot/settling vs analytic 2nd-order); scenario `pid_step` (gains changed at runtime change the response) |
| stall detection | Godot `test_stall_detection` (timing, current); `test_stall_event_is_auditable_and_latched`, `test_persistent_stall_escalates_to_fault_and_powers_off`, `test_gripper_stall_is_a_grip_not_a_fault`; TEST 3 |
| queue | `test_queue_lifecycle`, `test_queue_text_dsl_is_data_only` (no code execution), `test_controller_runs_queue_through_backend`, `test_queue_executes_through_physics_not_teleport` |
| states | `test_warning_hysteresis_and_estop`, `test_invalid_physics_is_a_fault`, `test_stop_estop_and_reset`, scenario `emergency_stop` |
| telemetry | `test_journal_is_auditlog_compatible` (format identical to `AuditLog`, redaction, NaN, mirroring), `test_camera_frame_is_journaled`, event checks in `test_controller_runs_queue_through_backend` |
| repeatability | TEST 4 (20 moves same side, 20 alternating, 20 with effects disabled) |
| serialisation | ARM_SPECS round trip (`dumps_specs`), queue JSON/text parsing, journal JSONL, `CameraFrame.to_dict`, `test_every_parameter_has_a_verification_status` |
| agent commands | `test_action_validation`, `test_runtime_tools_validate_intent` (AST executes `robot.enqueue`, rejects teleport-like actions), `test_hardware_backend_path_without_hardware` |
| determinism | Godot `test_determinism` (bit-exact), `test_controller_is_deterministic`, `test_lockstep_determinism` |
| physics sanity | Godot `test_hold_and_gravity`, `test_joint_constraints` (hinge swing < 0.5 deg), `test_gripper_travel`; TEST 5 (fall vs rigid pendulum, Python vs Godot inertia) |

## Validation scenarios (`abiyss/robotics/scenarios.py`)

Each scenario resets the world with a fixed seed, uses only the Robot API,
writes a JSON report (`<name>.json`), SVG plots and, with `--screenshots`,
PNG screenshots. Checks verify the mechanism, not a pretty result. Latest
numbers: [VALIDATION.md](VALIDATION.md).

| Scenario | Checks |
|---|---|
| TEST 1 `write_oi` | action completes; ink deposited by physical pen contact; both letters (>= 2 strokes); coverage within 1 mm >= 40 %; result is imperfect (mean deviation > 0.2 mm); no STALL/FAULT |
| TEST 2 `pick_four` | 4 masses attempted; a light object held+placed; insufficient torque detected for the 123 g cube; an object escapes with a dead-band-limited grip; placed objects not disturbed by later placements |
| TEST 3 `shoulder_stall` | shoulder stall under payload with requested > maximum torque; auditable event; STALL latched; new commands rejected; obstruction stall detected |
| TEST 4 `repeatability` | 20 trials; non-zero dispersion; approach-direction hysteresis larger than same-side dispersion; dispersion collapses when dead band/backlash/jitter are disabled |
| TEST 5 `gravity` | powered hold < 3 deg; unpowered shoulder falls > 20 deg; Python and Godot inertia agree within 5 %; fall matches a rigid pendulum within 25 % while the rigid assumption holds |
| `emergency_stop` | state, queue cleared, commands rejected, pose held, explicit reset |
| `pid_step` | metrics for 3 gain sets; gains change the response |
| `camera` | arm reached the viewpoint; camera above the object; 320x240 PNG with pose and intrinsics |
| `stability` | clamped base does not move; static model predicts tipping (combined COM beyond the base edge); a free-standing base lifts off (> 5 deg) when the arm reaches out |

## Pre-existing runtime tests

`tests/test_v0_1.py::test_qups_refuses_symlink_and_fifo` used to hang forever
on Linux (opening a FIFO for reading blocks until a writer appears). QuPs now
opens with `O_NONBLOCK` and rejects non-regular files after `fstat`, which is
what the test expects. Two other pre-existing runtime tests
(`test_process_output_flood_is_killed`, `test_schema_rejects_unknown_type_and_extra_property`)
fail on this VM independently of the robotics work and were left untouched.
