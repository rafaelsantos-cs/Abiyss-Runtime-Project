extends RefCounted
## Electromechanical model of one hobby RC servo (MG90S / SG90 class).
##
## Pure model: no scene nodes, no rendering. It is stepped once per physics
## tick with the measured state of the link it drives and returns the torque
## that the servo output exerts on that link. The caller applies that torque
## to the child body and the reaction (see `housing_reaction`) to the parent.
##
## Structure (see docs/PHYSICS_MODEL.md):
##   pulse width (controller, 50 Hz) --+-- pulse jitter (servo input capture)
##                                     v
##   target angle -> [dead band] -> PID -> DC motor envelope (stall torque,
##   back-EMF line to no-load speed) -> geared output inertia J (+ Coulomb and
##   viscous gear friction) -> backlash dead-zone spring/damper -> link.
##   The potentiometer measures the servo output shaft (with noise), so the
##   lumped backlash sits OUTSIDE the feedback loop.

const NAME_UNSET := "servo"

var joint_name: String = NAME_UNSET
var model: String = ""

# --- datasheet / derived ---------------------------------------------------
var stall_torque: float = 0.17652      # N*m at the operating voltage
var no_load_speed: float = 10.472      # rad/s at the operating voltage
var deadband_us: float = 5.0           # us
var pulse_neutral_us: float = 1500.0   # us
var pulse_per_90_us: float = 500.0     # us per 90 deg of servo travel
var pwm_period: float = 0.020          # s
var voltage: float = 4.8               # V
var stall_current: float = 0.75        # A
var no_load_current: float = 0.09      # A
var idle_current: float = 0.006        # A

# --- estimated ---------------------------------------------------------------
var backlash: float = 0.014            # rad, full gap width
var gear_stiffness: float = 8.0        # N*m/rad
var gear_damping: float = 0.01         # N*m*s/rad
var reflected_inertia: float = 1.0e-4  # kg*m^2
var coulomb_friction: float = 0.004    # N*m
var viscous_friction: float = 0.0005   # N*m*s/rad
var pulse_sigma_us: float = 1.5        # us
var pot_sigma: float = 0.0026          # rad

# --- controller (simulated servo electronics) --------------------------------
var kp: float = 2.5
var ki: float = 0.0
var kd: float = 0.03
var integral_limit: float = 0.1
var derivative_filter_hz: float = 60.0

# --- mapping between servo travel and joint coordinate ----------------------
var center: float = 0.0                # joint angle (rad) at 1500 us
var direction: float = 1.0

# --- stall detection (design policy) ----------------------------------------
var stall_speed_threshold: float = 0.35
var stall_saturation_ratio: float = 1.0
var stall_min_duration: float = 0.3

# --- feature switches (all ON by default; tests may switch them off) -------
var enable_deadband: bool = true
var enable_backlash: bool = true
var enable_jitter: bool = true

# --- state -------------------------------------------------------------------
var powered: bool = true
var pulse_cmd_us: float = 1500.0
var pulse_meas_us: float = 1500.0
var frame_timer: float = 0.0
var theta_s: float = 0.0               # servo output shaft angle (joint coords)
var omega_s: float = 0.0
var integ: float = 0.0
var d_filt: float = 0.0
var prev_meas: float = 0.0
var has_prev: bool = false
var rng: RandomNumberGenerator

# --- diagnostics (read by telemetry) ----------------------------------------
var target: float = 0.0
var measured: float = 0.0
var error: float = 0.0
var tau_request: float = 0.0
var tau_motor: float = 0.0
var tau_transmission: float = 0.0
var tau_friction: float = 0.0
var tau_avail: float = 0.0
var current: float = 0.0
var saturated: bool = false
var in_deadband: bool = false
var stall_timer: float = 0.0
var stalled: bool = false
var gap: float = 0.0                   # theta_s - theta_link
var housing_reaction: float = 0.0      # torque to apply to the parent body


func configure(servo_spec: Dictionary, joint_spec: Dictionary, sim_spec: Dictionary, safety_spec: Dictionary, seed_value: int) -> void:
	var p: Dictionary = servo_spec["parameters"]
	var c: Dictionary = servo_spec["controller"]
	model = str(joint_spec.get("servo", ""))
	joint_name = str(joint_spec.get("name", NAME_UNSET))
	stall_torque = _v(p, "stall_torque_4v8")
	no_load_speed = _v(p, "no_load_speed_4v8")
	deadband_us = _v(p, "dead_band_width")
	pulse_neutral_us = _v(p, "pulse_neutral")
	pulse_per_90_us = _v(p, "pulse_per_90deg")
	pwm_period = _v(p, "pwm_period")
	stall_current = _v(p, "stall_current_4v8")
	no_load_current = _v(p, "no_load_current_4v8")
	idle_current = _v(p, "idle_current")
	backlash = _v(p, "backlash")
	gear_stiffness = _v(p, "gear_stiffness")
	gear_damping = _v(p, "gear_damping")
	reflected_inertia = _v(p, "reflected_inertia")
	coulomb_friction = _v(p, "coulomb_friction")
	viscous_friction = _v(p, "viscous_friction")
	pulse_sigma_us = _v(p, "pulse_jitter_sigma")
	pot_sigma = _v(p, "potentiometer_noise_sigma")
	kp = _v(c, "kp")
	ki = _v(c, "ki")
	kd = _v(c, "kd")
	integral_limit = _v(c, "integral_limit")
	derivative_filter_hz = _v(c, "derivative_filter_hz")
	voltage = _v(sim_spec, "supply_voltage")
	center = deg_to_rad(float(joint_spec.get("servo_center_deg", 0.0)))
	direction = float(joint_spec.get("direction", 1.0))
	stall_speed_threshold = _v(safety_spec, "stall_speed_threshold")
	stall_saturation_ratio = _v(safety_spec, "stall_saturation_ratio")
	stall_min_duration = _v(safety_spec, "stall_min_duration")
	rng = RandomNumberGenerator.new()
	rng.seed = seed_value


static func _v(d: Dictionary, key: String) -> float:
	var item: Variant = d[key]
	if item is Dictionary:
		return float(item["value"])
	return float(item)


## Joint angle (rad) that a pulse width (us) asks for.
func pulse_to_angle(pulse_us: float) -> float:
	return center + direction * (pulse_us - pulse_neutral_us) / pulse_per_90_us * (PI / 2.0)


func angle_to_pulse(angle: float) -> float:
	return pulse_neutral_us + direction * (angle - center) / (PI / 2.0) * pulse_per_90_us


func deadband_half_angle() -> float:
	return (deadband_us * 0.5) / pulse_per_90_us * (PI / 2.0)


## Place the servo at rest at `angle` (used when the scene is built).
func reset_state(angle: float) -> void:
	theta_s = angle
	omega_s = 0.0
	integ = 0.0
	d_filt = 0.0
	has_prev = false
	pulse_cmd_us = angle_to_pulse(angle)
	pulse_meas_us = pulse_cmd_us
	frame_timer = 0.0
	stall_timer = 0.0
	stalled = false
	target = angle


func set_pulse(pulse_us: float) -> void:
	pulse_cmd_us = pulse_us


## Advance one physics tick. Returns the torque (N*m, about the joint axis,
## positive = increasing joint angle) that the servo output applies to the link.
## `link_inertia` is a (conservative) estimate of the link inertia about the
## joint axis; it is only used to integrate the transmission implicitly.
func step(dt: float, theta_link: float, omega_link: float, link_inertia: float = 1.0e9) -> float:
	# 1. Input capture: the servo measures the pulse once per PWM frame.
	frame_timer -= dt
	if frame_timer <= 0.0:
		frame_timer += pwm_period
		var noise := rng.randfn(0.0, pulse_sigma_us) if enable_jitter else 0.0
		pulse_meas_us = pulse_cmd_us + noise
	target = pulse_to_angle(pulse_meas_us)

	# 2. Potentiometer on the output shaft.
	var pot_noise := rng.randfn(0.0, pot_sigma) if enable_jitter else 0.0
	measured = theta_s + pot_noise
	error = target - measured

	# Derivative on measurement, first-order low-pass filtered.
	if not has_prev:
		prev_meas = measured
		has_prev = true
	var dmeas := (measured - prev_meas) / dt
	prev_meas = measured
	var tau_f := 1.0 / (TAU * derivative_filter_hz)
	d_filt += (dt / (dt + tau_f)) * (dmeas - d_filt)

	# 3. Controller + amplifier.
	var tau_hi := stall_torque * (1.0 - omega_s / no_load_speed)
	var tau_lo := stall_torque * (-1.0 - omega_s / no_load_speed)
	saturated = false
	if powered:
		in_deadband = enable_deadband and absf(error) <= deadband_half_angle()
		if in_deadband:
			# Amplifier off inside the dead band: the motor coasts.
			tau_request = 0.0
			tau_motor = 0.0
		else:
			integ = clampf(integ + ki * error * dt, -integral_limit, integral_limit)
			tau_request = kp * error + integ - kd * d_filt
			tau_motor = clampf(tau_request, tau_lo, tau_hi)
			saturated = (tau_request > tau_hi * stall_saturation_ratio and tau_request > 0.0) \
				or (tau_request < tau_lo * stall_saturation_ratio and tau_request < 0.0)
	else:
		in_deadband = false
		tau_request = 0.0
		tau_motor = 0.0
		integ = 0.0
	tau_avail = tau_hi if tau_request >= 0.0 else -tau_lo

	# 4. Transmission with lumped backlash (dead-zone spring + damper).
	# The spring/damper is integrated implicitly (backward Euler on the
	# relative motion of the two inertias). The geared output inertia is up to
	# ~60x larger than a gripper finger seen through the rack, which makes an
	# explicit coupling unstable at 1 kHz; the implicit form is stable for any
	# stiffness and reduces to the explicit one when dt -> 0.
	gap = theta_s - theta_link
	var half := backlash * 0.5 if enable_backlash else 0.0
	var defl := 0.0
	if gap > half:
		defl = gap - half
	elif gap < -half:
		defl = gap + half
	var tau_t := 0.0
	if defl != 0.0:
		var h := gear_stiffness * dt + gear_damping
		var a_motor := (tau_motor - viscous_friction * omega_s) / reflected_inertia
		var inv_sum := 1.0 / reflected_inertia + 1.0 / maxf(link_inertia, 1e-12)
		tau_t = (gear_stiffness * defl + h * ((omega_s - omega_link) + dt * a_motor)) / (1.0 + h * dt * inv_sum)
		# A gear contact can push but never pull.
		if defl > 0.0:
			tau_t = maxf(tau_t, 0.0)
		else:
			tau_t = minf(tau_t, 0.0)
	tau_transmission = tau_t

	# 5. Geared output inertia with viscous + Coulomb friction (stick-slip).
	var tau_drive := tau_motor - tau_t - viscous_friction * omega_s
	var omega_free := omega_s + dt * tau_drive / reflected_inertia
	var dv_fric := coulomb_friction * dt / reflected_inertia
	var omega_new := 0.0
	if absf(omega_free) > dv_fric:
		omega_new = omega_free - signf(omega_free) * dv_fric
	tau_friction = (omega_free - omega_new) * reflected_inertia / dt + viscous_friction * omega_s
	omega_s = omega_new
	theta_s += omega_s * dt

	# Reaction on the servo housing (parent body): electromagnetic and
	# friction torques act between the rotor/gear train and the housing.
	housing_reaction = -(tau_motor - tau_friction)

	# 6. Electrical model (DC motor: current ~ torque), telemetry only.
	if not powered:
		current = 0.0
	elif in_deadband:
		current = idle_current
	else:
		current = idle_current + (stall_current - idle_current) * absf(tau_motor) / stall_torque \
			+ maxf(no_load_current - idle_current, 0.0) * minf(absf(omega_s) / no_load_speed, 1.0)

	# 7. Stall detection: saturated drive while the output barely moves.
	if powered and saturated and absf(omega_s) < stall_speed_threshold:
		stall_timer += dt
	else:
		stall_timer = 0.0
	stalled = stall_timer >= stall_min_duration

	return tau_t


func telemetry() -> Dictionary:
	return {
		"servo": model,
		"powered": powered,
		"pulse_us": pulse_cmd_us,
		"pulse_measured_us": pulse_meas_us,
		"target": target,
		"servo_pos": theta_s,
		"servo_vel": omega_s,
		"measured": measured,
		"error": error,
		"tau_request": tau_request,
		"tau_motor": tau_motor,
		"tau_transmission": tau_transmission,
		"tau_avail": tau_avail,
		"tau_max": stall_torque,
		"current": current,
		"power_w": current * voltage,
		"saturated": saturated,
		"in_deadband": in_deadband,
		"stall_time": stall_timer,
		"stalled": stalled,
		"backlash_gap": gap,
		"kp": kp, "ki": ki, "kd": kd,
	}
