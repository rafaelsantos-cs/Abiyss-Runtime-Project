# Physics model

The lab never sets a pose. Every motion is the result of servo torques,
gravity, inertia, contacts and friction integrated by Jolt Physics at 1 kHz.
This page states the model exactly as implemented, and which numbers are
facts versus estimates (the authoritative list with sources is
[`ARM_SPECS.json`](../../ARM_SPECS.json)).

## 1. Rigid bodies and joints (Jolt, `robot_lab/sim/arm_plant.gd`)

| Body | Built from | Mass (kg) |
|---|---|---|
| base | static (clamped to the bench) or free (`arm.base_mount`) | 0.0534 incl. yaw servo |
| turret | turntable + 2 brackets + shoulder MG90S | 0.0294 |
| upper_arm | beam + elbow MG90S | 0.0214 |
| forearm | beam + wrist SG90 | 0.0150 |
| gripper (palm) | palm + wrist plate + gripper SG90 (+ pen 6 g for the writing tool) | 0.017 (0.023) |
| finger_left / finger_right | finger + rubber pad | 0.003 each |

* Each link is a `RigidBody3D` with collision boxes/cylinders for its parts.
  Mass, centre of mass and inertia are computed from the parts (box/cylinder
  formulas + parallel-axis theorem). Products of inertia are ignored (the
  principal axes are assumed to be the link axes).
* Servo masses are datasheet values (13.4 g, 9 g). Printed-part masses are
  **estimated** (infill unknown).
* Revolute joints are `HingeJoint3D` with **hard stops** (servo internal stop,
  estimated ~3 deg beyond the 180 deg travel). Jolt measures the hinge angle with
  the opposite sign of the joint convention used here; the stop interval is
  mirrored accordingly (verified by `test_hard_stops`).
* Fingers slide on `SliderJoint3D`s (travel 0-18 mm per finger).
* Gravity is standard gravity 9.80665 m/s^2. Godot's default linear/angular
  damping (0.1) is set to 0: any loss comes from the explicit friction terms.
* Contacts use Coulomb friction; Jolt combines two surfaces with
  `min(f1, f2)` and restitution with `clamp(r1 + r2)` (Godot 4.7.2 source).

### Solver settings (and why)

| Setting | Default | Lab | Reason |
|---|---|---|---|
| penetration slop | 0.02 m | 0.0002 m | 20 mm slop is as big as the grasped cubes |
| speculative contact distance | 0.02 m | 0.002 m | same |
| allow sleep | on | off | a sleeping link would ignore small servo torques |
| velocity / position iterations | 10 / 2 | 64 / 16 | with 16/4 the light hinges (~1e-5 kg m^2) failed to carry base-yaw torque to the arm (~1e-3 kg m^2) and twisted up to 11 deg off-axis ("swing" telemetry); 64/16 keeps the swing below 0.06 deg |
| physics rate | 60 Hz | 1000 Hz | servo electromechanics and finger contact |

`swing` (rotation of a hinge about any axis other than its own) is reported
for every joint in telemetry as a constraint-health metric.

## 2. Servo model (`robot_lab/sim/servo_model.gd`)

One instance per servo, stepped every physics tick. Notation: `theta_s`,
`omega_s` servo output shaft (where the potentiometer is); `theta_l`, `omega_l`
the link (rigid body).

1. **Input capture (PWM, 50 Hz).** Once per 20 ms frame the servo latches the
   commanded pulse plus noise: `p_meas = p_cmd + N(0, sigma_p)`
   (`pulse_jitter_sigma`, **estimated** 1.5 us MG90S / 2 us SG90).
   Target angle: `theta_t = center + dir * (p_meas - 1500) / 500 * 90 deg`
   (datasheet mapping 1-2 ms = +/-90 deg).
2. **Potentiometer.** `theta_m = theta_s + N(0, sigma_pot)` (**estimated**
   0.15/0.2 deg). Error `e = theta_t - theta_m`.
3. **Dead band (datasheet).** If `|e| <= db/2` the amplifier is off and the
   motor coasts (`tau_motor = 0`). `db` = 5 us (MG90S) / 10 us (SG90) =
   0.9 / 1.8 deg. Outside the band the drive uses the full error (the
   discontinuity is what makes hobby servos hunt).
4. **Controller.** `tau_req = Kp e + Ki integral(e) - Kd d(theta_m)/dt`
   (derivative on measurement, low-pass filtered 60 Hz, anti-windup clamp).
   The real MG90S/SG90 controller is an analog P-type IC whose gains are not
   published: Kp/Ki/Kd are **tuned in simulation** (Ki = 0 by default) and can
   be changed at runtime (`arm.set_pid`).
5. **DC motor envelope (datasheet endpoints, linear model).**
   `tau_motor = clamp(tau_req, tau_stall (-1 - omega_s/omega_nl), tau_stall (1 - omega_s/omega_nl))`
   with `tau_stall` = 1.8 kgf cm = 0.1765 N m (4.8 V) and
   `omega_nl` = 60 deg / 0.10 s = 10.47 rad/s (4.8 V). "Saturated" = the request
   exceeds the envelope.
6. **Transmission with backlash (lumped, outside the loop).** Gap
   `g = theta_s - theta_l`; dead zone of width `b` (**estimated** 0.8 deg MG90S,
   1.5 deg SG90). Inside the gap no torque reaches the link. Outside it a gear
   contact spring/damper (`k`, `c`, **estimated**) transmits
   `tau_t = k * defl + c * (omega_s - omega_l)` (push only). It is integrated
   **implicitly** using the composite inertia of the distal subtree about the
   joint axis, because the geared output inertia (~1e-4 kg m^2) is up to ~60x a
   finger's reflected inertia and an explicit coupling diverges at 1 kHz.
7. **Geared output inertia + friction.**
   `J_m omega_s' = tau_motor - tau_t - b_v omega_s - Coulomb` with stick-slip
   (velocity clamped to zero when the Coulomb impulse exceeds it). `J_m`,
   `b_v`, Coulomb are **estimated**. The housing (parent link) receives
   `-(tau_motor - tau_friction)`; the link receives `tau_t` (Newton's third law).
8. **Current (telemetry).** `I = I_idle + (I_stall - I_idle) |tau_motor| / tau_stall + ...`
   (DC motor: current proportional to torque). Stall current is not published
   by TowerPro: MG90S uses a clone datasheet (750 mA), SG90 is **unknown**.
9. **Stall detection.** Saturated drive while `|omega_s| < 0.35 rad/s` for
   `>= 0.3 s` (design policy). The servo reports it; the controller's safety
   supervisor decides what to do.
10. **Power off.** No drive torque and no current; the joint is held only by
    gear friction and the reflected inertia: **the arm falls** if gravity
    exceeds the (estimated) Coulomb friction.

Dead band, backlash and jitter are ON by default. They can be disabled per
servo (`configure enable_deadband/backlash/jitter`) for ablation experiments:
TEST 4 shows the end-point dispersion collapsing when they are off, proving
they act on the physics and not only on the picture.

## 3. Gripper

Parallel jaws on a rack and pinion (`pinion_radius` 11.5 mm, design). The SG90
model drives an equivalent angle `q = closed + x_mean / r`. Each finger gets
`F = eta * tau_t / r / 2` along its slide axis (efficiency `eta` 0.6,
**estimated**) with the reaction on the palm. A stiff spring-damper between the
finger displacements stands in for the rack mesh (keeps the jaws symmetric).
With the gripper stalled on an object each finger pushes ~4.5 N; holding then
depends on Coulomb friction (`min(pad, object)`) and the object weight and
accelerations. A stalled gripper is treated as a grip, not a fault.

## 4. Pen and ink

The pen is rigidly attached to the palm (slip in the jaws is not modelled).
Its spherical tip (r 1.2 mm) is a collision shape: it really touches and
rubs on the paper (friction 0.25). Ink is laid where the tip is within 0.3 mm
of the paper surface inside the sheet. The ink is ground truth from physics;
`ink_analysis` compares it with the ideal strokes.

## 5. Camera

`SubViewport` 320x240, 70 deg vertical FOV, mounted on the palm looking along
the tool axis, posed from the physics state at capture time. Frames are PNG
with pose and pinhole intrinsics. Not available in `--headless` runs.

## 6. Determinism

Python drives Godot in lockstep: physics advances only during a `step`
request (`PhysicsServer3D.set_active`). Noise comes from seeded RNGs.

* Same build + seed + command sequence **from a fresh lab process** produces
  bit-identical results, headless or windowed (checked by `test_determinism`,
  `test_lockstep_determinism`, and by running TEST 3 in two fresh processes:
  identical stall events).
* Running the same scenario after a *different history* of resets inside one
  process can differ in the last digits when contacts are involved (e.g. the
  TEST 3 payload stall requested 0.3744 N m alone vs 0.3755 N m after TEST 1-2):
  Jolt's internal body ids and contact ordering depend on the bodies created
  before. Reports therefore record the full scenario sequence they ran in.

## 7. Validation of the model against first principles

* **Mass model**: shoulder subtree inertia from the Python part list
  0.0014753 kg m^2 vs Godot/Jolt body data 0.0014759 kg m^2 (0.04 %).
* **Gravity**: unpowered shoulder fall vs a rigid pendulum integrated from
  the same masses: 4.65 deg measured vs 4.38 deg predicted over the window in
  which the powered elbow stays rigid (TEST 5).
* **Torque limits**: the payload that stalled the shoulder (123 g) is above
  the static lower bound the controller infers (>= 61 g) from the datasheet
  stall torque and the arm-only gravity model (TEST 3).
