# Simplifications and known limits

What the simulator does **not** do, or does approximately. Read this before
trusting a number for hardware decisions.

## Servo electromechanics

| Simplification | Consequence |
|---|---|
| The analog control IC is modelled as PID on (target - potentiometer) with the datasheet dead band. Real gains are unpublished; Kp/Kd are tuned in simulation, Ki = 0. | Stiffness, overshoot and sag under load are plausible, not measured. A real MG90S may sag less or more than the ~1-4 deg seen here. |
| Linear DC-motor torque-speed line between datasheet stall torque and no-load speed; supply fixed at 4.8 V. | No voltage sag when several servos stall at once; no PWM-duty nonlinearity. |
| Gear backlash, gear stiffness/damping, reflected inertia and Coulomb/viscous friction are **estimated** (no datasheet value exists). | Repeatability, oscillation and the behaviour of an unpowered joint scale with these guesses. If the real gear friction exceeds the gravity load, a real unpowered joint will *not* fall. |
| All gear-train and spline play is lumped into one dead zone between the servo output (potentiometer) and the link, i.e. outside the feedback loop. | Real servos have part of the play inside the loop (more hunting, less static error). |
| No thermal model, no current limiting, no brown-out. Stall protection is a policy (2 s -> power off), not physics. | A real servo may overheat, reset or strip gears; the sim does not. |
| Stall current: MG90S from a clone datasheet (750 mA); SG90 **unknown** (reports 0.36-1.3 A). | Current/power telemetry is indicative only. |
| Pulse jitter and potentiometer noise are white Gaussian, sampled per PWM frame / per tick. | Real noise has structure (supply ripple, pot wear). |
| Dead band uses the datasheet value (MG90S 5 us, SG90 10 us); TowerPro's current web page lists 1 us for both. | With 1 us the arm would be more precise; the conflict is recorded in ARM_SPECS.json. |

## Mechanics and contact

| Simplification | Consequence |
|---|---|
| Printed links are rigid boxes; masses estimated; products of inertia ignored. | No link flex (real PLA links flex, adding error). |
| Base clamped to the bench by default. `base_mount: "free"` makes it a rigid body resting on the bench (validated only by the stability scenario: it tips when the arm reaches out). | The whole robot cannot tip over unless the free base is enabled. |
| Gripper rack/pinion mesh replaced by a stiff virtual spring between the fingers; transmission efficiency 0.6 estimated, same in both directions. | Grip force is right in order of magnitude only. |
| Friction is Coulomb with Jolt's `min(f1, f2)` combine; no static/dynamic distinction, no rolling resistance. | Slip onset is approximate. |
| The pen is rigidly fixed to the palm; ink is laid when the tip is within 0.3 mm of the paper. | Pen slip in the jaws, ink spreading and pen pressure on line width are not modelled. |
| Objects are perfect rigid cubes with designed masses and estimated friction. | No deformation (the foam cube is not soft). |
| No cable drag, no air drag. | - |

## Numerics

| Item | Note |
|---|---|
| The gear transmission is integrated implicitly with the composite subtree inertia (locked distal joints). | Stable at 1 kHz for every mass ratio here; slightly over-damps very fast relative motion of distal joints. |
| Jolt solver iterations raised to 64/16. | Needed for constraint accuracy with ~1e-5 kg m^2 links (measured). |
| The rigid-pendulum check in TEST 5 is valid only while the powered distal joints stay rigid (~80 ms here); later the measured fall is slower because the elbow deflects. | Stated in the report. |

## Sensing and control

| Item | Note |
|---|---|
| The controller is open loop on joint pulses (like a host driving hobby servos). Measured joint angles come from the simulation and are used for safety, telemetry and holding after a stall. | Real SG90/MG90S have no position output; on hardware these checks need added sensors (encoders or the servo pot tapped, current sensing). The hardware backend reports `position_feedback: false`. |
| The controller does not know payload masses; it infers a lower bound only when a stall happens. | - |
| Camera: ideal pinhole, no lens distortion, no noise, no motion blur, no exposure. 320x240. | Replace with a calibrated model when the real camera is chosen. |
| Only uppercase single-stroke glyphs are available for `write()`. | - |

## Not implemented (on purpose, in this stage)

* Physical drivers (PCA9685, serial, camera): only the `PWMDriver` /
  `CameraSensor` contracts and a recording driver exist. No driver was
  written without hardware and its documentation.
* Collision-aware motion planning: transit moves only check the bench plane
  (lift-cross-descend when the joint path dips below 8 mm).
* Wrist roll: the 4-DOF arm cannot rotate the gripper about the tool axis, so
  objects must be aligned radially (fixtures in ARM_SPECS.json).
