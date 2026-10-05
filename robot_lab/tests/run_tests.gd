extends SceneTree
## Headless test runner for the Godot side of the lab.
##   godot --headless --path robot_lab --script res://tests/run_tests.gd
## Exit code 0 = all passed.

const Specs := preload("res://sim/specs.gd")
const ServoModel := preload("res://sim/servo_model.gd")
const ArmPlant := preload("res://sim/arm_plant.gd")

var specs: Dictionary
var failures: Array = []
var passed := 0
var current := ""


func _initialize() -> void:
	specs = Specs.load_specs()
	_run_all()


func _ok(cond: bool, msg: String) -> void:
	if cond:
		passed += 1
	else:
		failures.append("%s: %s" % [current, msg])
		push_error("FAIL %s: %s" % [current, msg])


func _servo(model: String = "MG90S") -> RefCounted:
	var s := ServoModel.new()
	var js := {"name": "t", "servo": model, "servo_center_deg": 0, "direction": 1}
	s.configure(specs["servo_models"][model], js, specs["simulation"], specs["safety"], 7)
	s.reset_state(0.0)
	return s


func _run_all() -> void:
	for t in ["test_pulse_mapping", "test_deadband", "test_torque_envelope", "test_backlash_gap", "test_stall_detection", "test_power_off", "test_jitter_switch"]:
		current = t
		call(t)
	# Plants need the scene tree and a physics space: wait for the first frame.
	await physics_frame
	for t in ["test_hold_and_gravity", "test_hard_stops", "test_gripper_travel", "test_determinism", "test_joint_constraints"]:
		current = t
		await call(t)
	print("GODOT TESTS: %d checks passed, %d failed" % [passed, failures.size()])
	for f in failures:
		print("  FAILED ", f)
	quit(0 if failures.is_empty() else 1)


# ---------------------------------------------------------------- servo model

func test_pulse_mapping() -> void:
	var s := _servo()
	_ok(is_equal_approx(s.pulse_to_angle(1500.0), 0.0), "1500 us is the centre")
	_ok(is_equal_approx(s.pulse_to_angle(2000.0), PI / 2.0), "2000 us = +90 deg (datasheet)")
	_ok(is_equal_approx(s.pulse_to_angle(1000.0), -PI / 2.0), "1000 us = -90 deg (datasheet)")
	_ok(absf(s.angle_to_pulse(s.pulse_to_angle(1234.5)) - 1234.5) < 1e-6, "mapping is invertible")
	_ok(absf(rad_to_deg(s.deadband_half_angle()) - 0.45) < 1e-6, "MG90S 5 us dead band = 0.9 deg wide")


func test_deadband() -> void:
	var s := _servo()
	s.enable_jitter = false
	var half: float = s.deadband_half_angle()
	s.set_pulse(s.angle_to_pulse(half * 0.8))
	s.step(0.001, 0.0, 0.0)
	_ok(s.in_deadband and s.tau_motor == 0.0, "error inside the dead band -> no drive")
	var s2 := _servo()
	s2.enable_jitter = false
	s2.set_pulse(s2.angle_to_pulse(half * 1.5))
	s2.step(0.001, 0.0, 0.0)
	_ok(not s2.in_deadband and s2.tau_motor > 0.0, "error outside the dead band -> drive")
	var s3 := _servo()
	s3.enable_jitter = false
	s3.enable_deadband = false
	s3.set_pulse(s3.angle_to_pulse(half * 0.5))
	s3.step(0.001, 0.0, 0.0)
	_ok(s3.tau_motor > 0.0, "dead band can be disabled (ablation)")


func test_torque_envelope() -> void:
	var s := _servo()
	s.enable_jitter = false
	s.set_pulse(s.angle_to_pulse(deg_to_rad(60.0)))
	s.step(0.001, 0.0, 0.0)
	_ok(absf(s.tau_motor - s.stall_torque) < 1e-9, "large error at rest -> stall torque (datasheet 1.8 kgf*cm)")
	_ok(s.saturated, "request beyond the envelope is flagged as saturated")
	var s2 := _servo()
	s2.enable_jitter = false
	s2.omega_s = s2.no_load_speed
	s2.set_pulse(s2.angle_to_pulse(deg_to_rad(60.0)))
	s2.step(0.001, 0.0, s2.no_load_speed)
	_ok(absf(s2.tau_motor) < 0.01 * s2.stall_torque + 1e-6, "no drive torque at the no-load speed (back-EMF line)")


func test_backlash_gap() -> void:
	var s := _servo()
	s.enable_jitter = false
	var half: float = s.backlash / 2.0
	s.theta_s = half * 0.9
	var tau: float = s.step(0.001, 0.0, 0.0)
	_ok(tau == 0.0, "inside the backlash gap no torque reaches the link")
	var s2 := _servo()
	s2.enable_jitter = false
	s2.theta_s = half * 1.5
	tau = s2.step(0.001, 0.0, 0.0)
	_ok(tau > 0.0, "gap closed -> gear contact pushes the link")
	var s3 := _servo()
	s3.enable_jitter = false
	s3.enable_backlash = false
	s3.theta_s = half * 0.9
	tau = s3.step(0.001, 0.0, 0.0)
	_ok(tau > 0.0, "backlash can be disabled (ablation)")


func test_stall_detection() -> void:
	var s := _servo()
	s.enable_jitter = false
	s.set_pulse(s.angle_to_pulse(deg_to_rad(45.0)))
	var stalled_at := -1.0
	for i in range(1000):
		# A link that cannot move (blocked): theta_link = 0 always.
		s.step(0.001, 0.0, 0.0)
		if s.stalled and stalled_at < 0.0:
			stalled_at = (i + 1) * 0.001
	_ok(stalled_at > 0.0, "blocked servo is detected as stalled")
	_ok(absf(stalled_at - s.stall_min_duration) < 0.05, "stall reported after the configured minimum duration (%.3f s)" % stalled_at)
	_ok(s.current > 0.7 * s.stall_current, "stall draws ~stall current")


func test_power_off() -> void:
	var s := _servo()
	s.set_pulse(s.angle_to_pulse(deg_to_rad(30.0)))
	s.powered = false
	s.step(0.001, 0.0, 0.0)
	_ok(s.tau_motor == 0.0 and s.current == 0.0, "unpowered servo produces no torque and draws no current")


func test_jitter_switch() -> void:
	var a := _servo()
	var b := _servo()
	a.enable_jitter = false
	a.step(0.001, 0.0, 0.0)
	_ok(a.measured == a.theta_s, "no potentiometer noise when jitter is disabled")
	b.step(0.001, 0.0, 0.0)
	_ok(b.measured != b.theta_s, "potentiometer noise present by default")


# ---------------------------------------------------------------- plant

func _plant(tool: String = "gripper", seed_value: int = 1337) -> Node3D:
	var p: Node3D = ArmPlant.new()
	root.add_child(p)
	p.build(specs, tool, seed_value)
	return p


func _free(p: Node3D) -> void:
	root.remove_child(p)
	p.free()


func _run(p: Node3D, ticks: int, each: Callable = Callable()) -> void:
	for i in range(ticks):
		if each.is_valid():
			each.call(i)
		p.tick(1.0 / Engine.physics_ticks_per_second)
		await physics_frame


func test_hold_and_gravity() -> void:
	var p := _plant()
	await _run(p, 1000)
	var q0: float = p.joint_by_name["shoulder"]["q"]
	await _run(p, 500)
	var q1: float = p.joint_by_name["shoulder"]["q"]
	_ok(absf(rad_to_deg(q1 - q0)) < 3.0, "powered shoulder holds the home pose (%.2f deg drift)" % rad_to_deg(q1 - q0))
	p.servos["shoulder"].powered = false
	await _run(p, 500)
	var q2: float = p.joint_by_name["shoulder"]["q"]
	_ok(absf(rad_to_deg(q2 - q1)) > 20.0, "unpowered shoulder falls under gravity (%.1f deg)" % rad_to_deg(q2 - q1))
	_free(p)


func test_hard_stops() -> void:
	var p := _plant()
	# Yaw only (free motion): push against both stops with the servo unpowered.
	# The link bounces off the stop (gear spring + reflected inertia store the
	# impact energy), so the test checks the extreme reached, not the end pose.
	p.servos["base_yaw"].powered = false
	var rec: Dictionary = p.joint_by_name["base_yaw"]
	var ext := {"lo": 0.0, "hi": 0.0}
	var push := func(sign: float) -> Callable:
		return func(_i: int) -> void:
			p._measure_joint(rec)
			ext["lo"] = minf(ext["lo"], rad_to_deg(rec["q"]))
			ext["hi"] = maxf(ext["hi"], rad_to_deg(rec["q"]))
			PhysicsServer3D.body_apply_torque(rec["child"].get_rid(), (rec["axis_world"] as Vector3) * 0.05 * sign)
	await _run(p, 1500, push.call(-1.0))
	await _run(p, 3000, push.call(1.0))
	var stops: Array = rec["spec"]["hard_stops_deg"]
	_ok(absf(ext["lo"] - float(stops[0])) < 1.0, "yaw reaches the lower hard stop (%.2f vs %s)" % [ext["lo"], stops[0]])
	_ok(ext["lo"] > float(stops[0]) - 1.0, "yaw never passes the lower hard stop")
	_ok(ext["hi"] < float(stops[1]) + 1.0 and ext["hi"] > 45.0, "upper side bounded by the stop or by contact (%.2f, stop %s)" % [ext["hi"], stops[1]])
	_free(p)


func test_gripper_travel() -> void:
	var p := _plant()
	var s: RefCounted = p.servos["gripper"]
	p.set_servo_command("gripper", s.angle_to_pulse(deg_to_rad(-90.0)), true)
	await _run(p, 1200)
	var closed: float = p.telemetry(false, false)["gripper"]["opening"]
	p.set_servo_command("gripper", s.angle_to_pulse(0.0), true)
	await _run(p, 1200)
	var opened: float = p.telemetry(false, false)["gripper"]["opening"]
	_ok(absf(closed - 0.004) < 0.0015, "jaws close to 2 x closed_half_gap (%.2f mm)" % (closed * 1000.0))
	_ok(absf(opened - 0.040) < 0.0015, "jaws open to 2 x (gap + travel) (%.2f mm)" % (opened * 1000.0))
	_free(p)


func _trace(seed_value: int) -> Array:
	var p := _plant("gripper", seed_value)
	var out := []
	var cmd := func(i: int) -> void:
		if i == 100:
			p.set_servo_command("shoulder", p.servos["shoulder"].angle_to_pulse(deg_to_rad(80.0)), true)
			p.set_servo_command("base_yaw", p.servos["base_yaw"].angle_to_pulse(deg_to_rad(20.0)), true)
		if i % 50 == 0:
			var tel: Dictionary = p.telemetry(false, false)
			out.append([tel["joints"]["shoulder"]["link_pos"], tel["joints"]["base_yaw"]["link_pos"], tel["tcp"]["position"]])
	await _run(p, 1500, cmd)
	_free(p)
	return out


func test_determinism() -> void:
	var a: Array = await _trace(42)
	var b: Array = await _trace(42)
	var c: Array = await _trace(43)
	_ok(a == b, "same seed + same commands -> identical trajectory (bit-exact)")
	_ok(a != c, "different jitter seed -> different trajectory")


func test_joint_constraints() -> void:
	var p := _plant()
	var max_swing := 0.0
	var cmd := func(i: int) -> void:
		if i == 50:
			p.set_servo_command("base_yaw", p.servos["base_yaw"].angle_to_pulse(deg_to_rad(60.0)), true)
			p.set_servo_command("shoulder", p.servos["shoulder"].angle_to_pulse(deg_to_rad(40.0)), true)
			p.set_servo_command("elbow", p.servos["elbow"].angle_to_pulse(deg_to_rad(-40.0)), true)
		if i % 10 == 0:
			for rec in p.joints:
				p._measure_joint(rec)
				max_swing = maxf(max_swing, rad_to_deg(rec["swing"]))
	await _run(p, 1500, cmd)
	_ok(max_swing < 0.5, "hinge constraints hold during a fast multi-joint move (max swing %.3f deg)" % max_swing)
	_free(p)
