extends Node3D
## Physical plant of the Abiyss lab: bench, paper, arm rigid bodies, joints,
## servo models, gripper, pen and test objects.
##
## Built procedurally from ARM_SPECS.json so the specs file is the single
## source of truth. This node knows nothing about the network bridge, the UI
## or the Python controller: it is stepped with `tick(dt)` and inspected with
## `telemetry()`.

const Specs := preload("res://sim/specs.gd")
const ServoModel := preload("res://sim/servo_model.gd")

signal collision_event(event: Dictionary)

var specs: Dictionary = {}
var tool: String = "gripper"
var seed_value: int = 1337
var sim_time: float = 0.0
var tick_count: int = 0

var bodies: Dictionary = {}          # link name -> PhysicsBody3D
var joints: Array = []               # joint records (Dictionary)
var joint_by_name: Dictionary = {}
var servos: Dictionary = {}          # joint name -> ServoModel (includes "gripper")
var gripper: Dictionary = {}         # gripper record
var objects: Dictionary = {}         # id -> RigidBody3D
var paper_body: StaticBody3D
var bench_body: StaticBody3D
var paper_top: float = 0.0
var paper_rect: Rect2

var pen_enabled: bool = false
var pen_tip_local: Vector3            # Godot coords, gripper body local
var pen_tip_radius: float = 0.0012
var pen_down: bool = false
var ink_strokes: Array = []           # Array[PackedVector2Array] (robot x, y)
var ink_new: Array = []               # points added since last telemetry read
var ink_mesh: MeshInstance3D
var ink_dirty: bool = false
var ink_width: float = 0.0012
var ink_color: Color = Color(0.82, 0.24, 0.24)

var pending_collisions: Array = []
var trace_joint: String = "shoulder"
var trace: Array = []                 # selected joint: [sim_time, target, servo_pos, link_pos]
var traces: Dictionary = {}           # every servo, same format (switching joints keeps history)
var trace_window: float = 3.0
var trace_every: int = 2
var _materials: Dictionary = {}
var name_of_body: Dictionary = {}     # instance id -> name


# ---------------------------------------------------------------------------
# Construction
# ---------------------------------------------------------------------------

func build(p_specs: Dictionary, p_tool: String = "gripper", p_seed: int = 1337) -> void:
	specs = p_specs
	tool = p_tool
	seed_value = p_seed
	sim_time = 0.0
	tick_count = 0
	pen_enabled = tool == "pen"
	_build_workspace()
	_build_arm()
	_build_gripper()
	if pen_enabled:
		_build_pen()
	_build_objects()
	_build_ink()
	_fill_subtrees()


func _fill_subtrees() -> void:
	var order := ["turret", "upper_arm", "forearm", "gripper"]
	for rec in joints:
		var child_name := str(rec["spec"]["child"])
		var start := order.find(child_name)
		var lst := []
		for i in range(start, order.size()):
			lst.append(bodies[order[i]])
		lst.append(bodies["finger_left"])
		lst.append(bodies["finger_right"])
		rec["subtree"] = lst


func _material(hex: String, roughness: float = 0.75, metallic: float = 0.0) -> StandardMaterial3D:
	var key := "%s/%s/%s" % [hex, roughness, metallic]
	if _materials.has(key):
		return _materials[key]
	var m := StandardMaterial3D.new()
	m.albedo_color = Color(hex)
	m.roughness = roughness
	m.metallic = metallic
	_materials[key] = m
	return m


func _phys_material(friction: float) -> PhysicsMaterial:
	var pm := PhysicsMaterial.new()
	pm.friction = friction
	pm.bounce = 0.0
	return pm


func _friction(material_name: String) -> float:
	return float(specs["materials"]["friction"][material_name])


func _add_part_shape(body: Node3D, part: Dictionary, with_collision: bool = true) -> void:
	var offset := Specs.r2g(part.get("offset", [0, 0, 0]))
	var mesh_inst := MeshInstance3D.new()
	mesh_inst.name = str(part.get("name", "part"))
	var shape: Shape3D
	if part["shape"] == "cylinder":
		var cm := CylinderMesh.new()
		cm.top_radius = float(part["radius"])
		cm.bottom_radius = float(part["radius"])
		cm.height = float(part["height"])
		cm.radial_segments = 20
		cm.rings = 1
		mesh_inst.mesh = cm
		var cs := CylinderShape3D.new()
		cs.radius = float(part["radius"])
		cs.height = float(part["height"])
		shape = cs
	else:
		var size := Specs.r2g_size(part["size"])
		var bm := BoxMesh.new()
		bm.size = size
		mesh_inst.mesh = bm
		var bs := BoxShape3D.new()
		bs.size = size
		shape = bs
	mesh_inst.position = offset
	mesh_inst.material_override = _material(str(part.get("color", "#888888")))
	body.add_child(mesh_inst)
	if with_collision:
		var col := CollisionShape3D.new()
		col.name = "col_" + str(part.get("name", "part"))
		col.shape = shape
		col.position = offset
		body.add_child(col)


func _part_mass(part: Dictionary) -> float:
	if part.has("servo"):
		return float(Specs.v(specs["servo_models"][part["servo"]]["parameters"], "mass"))
	return float(part.get("mass", 0.0))


## Mass, centre of mass and diagonal inertia (about the COM, link axes) of a
## list of parts. Products of inertia are ignored (documented simplification).
func _mass_properties(parts: Array) -> Dictionary:
	var total := 0.0
	var com := Vector3.ZERO
	for part in parts:
		var m := _part_mass(part)
		total += m
		com += Vector3(part["offset"][0], part["offset"][1], part["offset"][2]) * m
	if total <= 0.0:
		return {"mass": 0.0, "com": Vector3.ZERO, "inertia": Vector3.ZERO}
	com /= total
	var inertia := Vector3.ZERO
	for part in parts:
		var m := _part_mass(part)
		var o := Vector3(part["offset"][0], part["offset"][1], part["offset"][2])
		var local := Vector3.ZERO
		if part["shape"] == "cylinder":
			var r := float(part["radius"])
			var h := float(part["height"])
			local = Vector3(m * (3.0 * r * r + h * h) / 12.0, m * (3.0 * r * r + h * h) / 12.0, m * r * r / 2.0)
		else:
			var s: Array = part["size"]
			var sx := float(s[0])
			var sy := float(s[1])
			var sz := float(s[2])
			local = Vector3(m * (sy * sy + sz * sz) / 12.0, m * (sx * sx + sz * sz) / 12.0, m * (sx * sx + sy * sy) / 12.0)
		var d := o - com
		inertia += local + Vector3(m * (d.y * d.y + d.z * d.z), m * (d.x * d.x + d.z * d.z), m * (d.x * d.x + d.y * d.y))
	return {"mass": total, "com": com, "inertia": inertia}


func _make_rigid(body_name: String, parts: Array, xform_robot: Transform3D, friction: float) -> RigidBody3D:
	var body := RigidBody3D.new()
	body.name = body_name
	var mp := _mass_properties(parts)
	body.mass = maxf(float(mp["mass"]), 1e-4)
	body.center_of_mass_mode = RigidBody3D.CENTER_OF_MASS_MODE_CUSTOM
	body.center_of_mass = Specs.r2g(mp["com"])
	var ir: Vector3 = mp["inertia"]
	body.inertia = Vector3(ir.x, ir.z, ir.y)   # robot (x,y,z) axes -> godot (x,z,y)
	body.can_sleep = false
	body.linear_damp_mode = RigidBody3D.DAMP_MODE_REPLACE
	body.angular_damp_mode = RigidBody3D.DAMP_MODE_REPLACE
	body.linear_damp = 0.0
	body.angular_damp = 0.0
	body.physics_material_override = _phys_material(friction)
	body.contact_monitor = true
	body.max_contacts_reported = 8
	for part in parts:
		_add_part_shape(body, part)
	body.transform = _xform_r2g(xform_robot)
	add_child(body)
	body.set_meta("robot_name", body_name)
	body.set_meta("mass_properties", {"mass": mp["mass"], "com": Specs.vec_to_array(mp["com"]), "inertia": Specs.vec_to_array(mp["inertia"])})
	name_of_body[body.get_instance_id()] = body_name
	bodies[body_name] = body
	return body


func _xform_r2g(t: Transform3D) -> Transform3D:
	return Transform3D(Specs.basis_r2g(t.basis), Specs.r2g(t.origin))


func _build_workspace() -> void:
	var ws: Dictionary = specs["workspace"]
	# Bench
	bench_body = StaticBody3D.new()
	bench_body.name = "bench"
	bench_body.physics_material_override = _phys_material(_friction(ws["bench"]["friction_material"]))
	var bsize := Specs.r2g_size(ws["bench"]["size"])
	var bcol := CollisionShape3D.new()
	var bshape := BoxShape3D.new()
	bshape.size = bsize
	bcol.shape = bshape
	bench_body.add_child(bcol)
	var bmesh := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = bsize
	bmesh.mesh = bm
	bmesh.material_override = _material("#3b4148", 0.9)
	bench_body.add_child(bmesh)
	bench_body.position = Specs.r2g(ws["bench"]["center"])
	add_child(bench_body)
	name_of_body[bench_body.get_instance_id()] = "bench"
	# Paper
	paper_body = StaticBody3D.new()
	paper_body.name = "paper"
	paper_body.physics_material_override = _phys_material(_friction(ws["paper"]["friction_material"]))
	var psize := Specs.r2g_size(ws["paper"]["size"])
	var pcol := CollisionShape3D.new()
	var pshape := BoxShape3D.new()
	pshape.size = psize
	pcol.shape = pshape
	paper_body.add_child(pcol)
	var pmesh := MeshInstance3D.new()
	var pm := BoxMesh.new()
	pm.size = psize
	pmesh.mesh = pm
	pmesh.material_override = _material("#f3f0e6", 0.95)
	paper_body.add_child(pmesh)
	paper_body.position = Specs.r2g(ws["paper"]["center"])
	add_child(paper_body)
	name_of_body[paper_body.get_instance_id()] = "paper"
	var pc: Array = ws["paper"]["center"]
	var ps: Array = ws["paper"]["size"]
	paper_top = float(pc[2]) + float(ps[2]) / 2.0
	paper_rect = Rect2(float(pc[0]) - float(ps[0]) / 2.0, float(pc[1]) - float(ps[1]) / 2.0, float(ps[0]), float(ps[1]))


func _build_arm() -> void:
	var arm: Dictionary = specs["arm"]
	var sim_spec: Dictionary = specs["simulation"]
	var safety: Dictionary = specs["safety"]
	# Base: clamped to the bench (static) or free-standing.
	var base_parts: Array = arm["base"]["parts"]
	if str(arm.get("base_mount", "clamped")) == "free":
		# Free-standing base resting on the bench (not bolted): it can slide,
		# spin under the yaw reaction torque, or tip over.
		_make_rigid("base", base_parts, Transform3D.IDENTITY, _friction("pla_link"))
	else:
		var sb := StaticBody3D.new()
		sb.name = "base"
		for part in base_parts:
			_add_part_shape(sb, part)
		add_child(sb)
		bodies["base"] = sb
		name_of_body[sb.get_instance_id()] = "base"
	# Forward kinematics at the home pose, in robot coordinates.
	var link_xform: Dictionary = {"base": Transform3D.IDENTITY}
	var index := 0
	for js in arm["joints"]:
		var parent_x: Transform3D = link_xform[js["parent"]]
		var origin := Vector3(js["origin"][0], js["origin"][1], js["origin"][2])
		var axis := Vector3(js["axis"][0], js["axis"][1], js["axis"][2]).normalized()
		var q_home := deg_to_rad(float(js["home_deg"]))
		var child_x := parent_x * Transform3D(Basis(axis, q_home), origin)
		link_xform[js["child"]] = child_x
		var child_parts: Array = arm["links"][js["child"]]["parts"]
		var body := _make_rigid(js["child"], child_parts, child_x, _friction("pla_link"))
		var parent_body: PhysicsBody3D = bodies[js["parent"]]
		# Hinge: joint local Z must be the rotation axis (Godot/Jolt convention).
		var axis_world_r := parent_x.basis * axis
		var joint_origin_r := parent_x * origin
		var hinge := HingeJoint3D.new()
		hinge.name = "hinge_" + str(js["name"])
		var z := Specs.r2g(axis_world_r).normalized()
		var x := z.cross(Vector3.UP)
		if x.length() < 0.1:
			x = z.cross(Vector3.RIGHT)
		x = x.normalized()
		var y := z.cross(x).normalized()
		hinge.transform = Transform3D(Basis(x, y, z), Specs.r2g(joint_origin_r))
		hinge.set_flag(HingeJoint3D.FLAG_USE_LIMIT, true)
		var stops: Array = js["hard_stops_deg"]
		# Jolt measures the hinge angle with the opposite sign of the
		# right-hand rule about the joint Z axis that our joint angle uses
		# (verified by tests/run_tests.gd::test_hard_stops), so the stop
		# interval relative to the build pose is mirrored here.
		hinge.set_param(HingeJoint3D.PARAM_LIMIT_LOWER, -(deg_to_rad(float(stops[1])) - q_home))
		hinge.set_param(HingeJoint3D.PARAM_LIMIT_UPPER, -(deg_to_rad(float(stops[0])) - q_home))
		add_child(hinge)
		hinge.node_a = hinge.get_path_to(parent_body)
		hinge.node_b = hinge.get_path_to(body)
		# Servo model
		var servo := ServoModel.new()
		servo.configure(specs["servo_models"][js["servo"]], js, sim_spec, safety, seed_value * 31 + index)
		servo.reset_state(q_home)
		servos[js["name"]] = servo
		var parent_basis_g: Basis = _xform_r2g(parent_x).basis
		var rec := {
			"name": js["name"],
			"spec": js,
			"parent": parent_body,
			"child": body,
			"hinge": hinge,
			"servo": servo,
			"q_home": q_home,
			"axis_parent_local": parent_basis_g.inverse() * Specs.r2g(axis_world_r).normalized(),
			"rel0": parent_basis_g.inverse() * _xform_r2g(child_x).basis,
			"q": q_home,
			"qd": 0.0,
			"swing": 0.0,
			"joint_point_parent_local": _xform_r2g(parent_x).affine_inverse() * Specs.r2g(joint_origin_r),
			"point_world": Specs.r2g(joint_origin_r),
			"subtree": [],
			"soft_limits": [deg_to_rad(float(js["soft_limits_deg"][0])), deg_to_rad(float(js["soft_limits_deg"][1]))],
			"hard_stops": [deg_to_rad(float(stops[0])), deg_to_rad(float(stops[1]))],
		}
		joints.append(rec)
		joint_by_name[js["name"]] = rec
		index += 1


func _build_gripper() -> void:
	var g: Dictionary = specs["arm"]["gripper"]
	var palm: RigidBody3D = bodies["gripper"]
	var f: Dictionary = g["finger"]
	var pad: Dictionary = g["pad"]
	var closed_half := float(Specs.v(g, "closed_half_gap"))
	var travel := float(Specs.v(g, "finger_travel"))
	var home_deg := float(g["home_deg"])
	var closed_deg := float(g["servo_closed_deg"])
	var r_p := float(Specs.v(g, "pinion_radius"))
	var x_home := clampf(deg_to_rad(home_deg - closed_deg) * r_p, 0.0, travel)
	var fs: Array = f["size"]
	var ps: Array = pad["size"]
	var fingers: Array = []
	for side in [1, -1]:
		# Finger frame in the gripper link frame (robot coords).
		var inner := closed_half + x_home
		var y_center := float(side) * (inner + float(ps[1]) + float(fs[1]) / 2.0)
		var fparts := [
			{"name": "finger", "shape": "box", "size": fs, "offset": [0, 0, 0], "mass": f["mass"], "color": f["color"]},
			{"name": "pad", "shape": "box", "size": ps, "offset": [0, -float(side) * (float(fs[1]) / 2.0 + float(ps[1]) / 2.0), 0], "mass": 0.0, "color": pad["color"]},
		]
		var local_r := Transform3D(Basis.IDENTITY, Vector3(float(f["offset_x"]), y_center, 0.0))
		var palm_r := Transform3D(Specs.basis_g2r(palm.transform.basis), Specs.g2r(palm.transform.origin))
		var fname := "finger_left" if side == 1 else "finger_right"
		var fb := _make_rigid(fname, fparts, palm_r * local_r, _friction("finger_pad_rubber"))
		# Slider: joint local X = slide axis (outward = +side * y of gripper).
		var slide_world_r := palm_r.basis * Vector3(0, float(side), 0)
		var sx := Specs.r2g(slide_world_r).normalized()
		var helper := Vector3.UP if absf(sx.dot(Vector3.UP)) < 0.9 else Vector3.RIGHT
		var sz := sx.cross(helper).normalized()
		var sy := sz.cross(sx).normalized()
		var slider := SliderJoint3D.new()
		slider.name = "slider_" + fname
		slider.transform = Transform3D(Basis(sx, sy, sz), fb.position)
		slider.set_param(SliderJoint3D.PARAM_LINEAR_LIMIT_LOWER, -x_home)
		slider.set_param(SliderJoint3D.PARAM_LINEAR_LIMIT_UPPER, travel - x_home)
		add_child(slider)
		slider.node_a = slider.get_path_to(palm)
		slider.node_b = slider.get_path_to(fb)
		fingers.append({"body": fb, "side": side, "home_pos_local": palm.transform.affine_inverse() * fb.position, "slide_local": palm.transform.basis.inverse() * sx, "x": x_home, "v": 0.0})
	var servo := ServoModel.new()
	var jspec := {"name": "gripper", "servo": g["servo"], "servo_center_deg": g["servo_center_deg"], "direction": 1}
	servo.configure(specs["servo_models"][g["servo"]], jspec, specs["simulation"], specs["safety"], seed_value * 31 + 97)
	var q_home := deg_to_rad(home_deg)
	servo.reset_state(q_home)
	servos["gripper"] = servo
	gripper = {
		"fingers": fingers,
		"servo": servo,
		"r_p": r_p,
		"eta": float(Specs.v(g, "transmission_efficiency")),
		"x_home": x_home,
		"travel": travel,
		"closed_rad": deg_to_rad(closed_deg),
		"closed_half": closed_half,
		"k_sync": float(Specs.v(g, "sync_stiffness")),
		"c_sync": float(Specs.v(g, "sync_damping")),
		"soft_limits": [deg_to_rad(float(g["soft_limits_deg"][0])), deg_to_rad(float(g["soft_limits_deg"][1]))],
		"q": q_home,
		"force_each": 0.0,
	}


func _build_pen() -> void:
	var pen: Dictionary = specs["arm"]["pen"]
	var palm: RigidBody3D = bodies["gripper"]
	var tip: Array = pen["tip_offset"]
	var length := float(pen["length"])
	var radius := float(pen["radius"])
	pen_tip_radius = float(pen["tip_radius"])
	ink_width = float(pen["ink_width"])
	ink_color = Color(str(pen["color"]))
	var center_x := float(tip[0]) - length / 2.0
	# Body of the pen (cylinder along the tool x axis) + spherical felt tip.
	var cyl := CylinderShape3D.new()
	cyl.radius = radius
	cyl.height = length - pen_tip_radius * 2.0
	var col := CollisionShape3D.new()
	col.name = "col_pen"
	col.shape = cyl
	col.position = Specs.r2g([center_x - pen_tip_radius, 0, 0])
	col.rotation = Vector3(0, 0, PI / 2.0)   # godot cylinder axis Y -> X
	palm.add_child(col)
	var tip_shape := SphereShape3D.new()
	tip_shape.radius = pen_tip_radius
	var tcol := CollisionShape3D.new()
	tcol.name = "col_pen_tip"
	tcol.shape = tip_shape
	pen_tip_local = Specs.r2g([float(tip[0]) - pen_tip_radius, 0, 0])
	tcol.position = pen_tip_local
	palm.add_child(tcol)
	var mesh := MeshInstance3D.new()
	var cm := CylinderMesh.new()
	cm.top_radius = radius
	cm.bottom_radius = radius * 0.55
	cm.height = length
	cm.radial_segments = 12
	mesh.mesh = cm
	mesh.position = Specs.r2g([center_x, 0, 0])
	mesh.rotation = Vector3(0, 0, -PI / 2.0)
	mesh.material_override = _material("#e7e2d8", 0.6)
	palm.add_child(mesh)
	var tip_mesh := MeshInstance3D.new()
	var sm := SphereMesh.new()
	sm.radius = pen_tip_radius * 1.4
	sm.height = pen_tip_radius * 2.8
	tip_mesh.mesh = sm
	tip_mesh.position = pen_tip_local
	tip_mesh.material_override = _material(str(pen["color"]), 0.5)
	palm.add_child(tip_mesh)
	# Add pen mass to the palm (point mass at the pen centre): update COM/inertia.
	var mp: Dictionary = palm.get_meta("mass_properties")
	var parts: Array = specs["arm"]["links"]["gripper"]["parts"].duplicate(true)
	parts.append({"name": "pen", "shape": "box", "size": [length, radius * 2.0, radius * 2.0], "offset": [center_x, 0, 0], "mass": float(pen["mass"])})
	var mp2 := _mass_properties(parts)
	palm.mass = float(mp2["mass"])
	palm.center_of_mass = Specs.r2g(mp2["com"])
	var ir: Vector3 = mp2["inertia"]
	palm.inertia = Vector3(ir.x, ir.z, ir.y)
	palm.set_meta("mass_properties", {"mass": mp2["mass"], "com": Specs.vec_to_array(mp2["com"]), "inertia": Specs.vec_to_array(mp2["inertia"]), "without_pen": mp})


func _build_objects() -> void:
	for o in specs["workspace"]["objects"]:
		var body := RigidBody3D.new()
		body.name = "object_" + str(o["id"])
		body.mass = float(o["mass"])
		body.can_sleep = false
		body.linear_damp_mode = RigidBody3D.DAMP_MODE_REPLACE
		body.angular_damp_mode = RigidBody3D.DAMP_MODE_REPLACE
		body.physics_material_override = _phys_material(float(o["friction"]))
		body.contact_monitor = true
		body.max_contacts_reported = 8
		var part := {"name": "body", "shape": "box", "size": o["size"], "offset": [0, 0, 0], "color": o["color"]}
		_add_part_shape(body, part)
		body.position = Specs.r2g(o["position"])
		body.rotation = Vector3(0, deg_to_rad(float(o.get("yaw_deg", 0.0))), 0)   # robot yaw about +z = godot +y
		add_child(body)
		objects[str(o["id"])] = body
		name_of_body[body.get_instance_id()] = "object_" + str(o["id"])
		body.set_meta("robot_name", "object_" + str(o["id"]))
	for b in bodies.values():
		if b is RigidBody3D:
			(b as RigidBody3D).body_entered.connect(_on_body_entered.bind(b))
			(b as RigidBody3D).body_exited.connect(_on_body_exited.bind(b))
	for b in objects.values():
		(b as RigidBody3D).body_entered.connect(_on_body_entered.bind(b))
		(b as RigidBody3D).body_exited.connect(_on_body_exited.bind(b))


func _build_ink() -> void:
	ink_mesh = MeshInstance3D.new()
	ink_mesh.name = "ink"
	var m := StandardMaterial3D.new()
	m.albedo_color = ink_color
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.cull_mode = BaseMaterial3D.CULL_DISABLED
	ink_mesh.material_override = m
	ink_mesh.mesh = ImmediateMesh.new()
	add_child(ink_mesh)


func _on_body_entered(other: Node, me: Node) -> void:
	_queue_collision("collision_begin", me, other)


func _on_body_exited(other: Node, me: Node) -> void:
	_queue_collision("collision_end", me, other)


func _queue_collision(kind: String, a: Node, b: Node) -> void:
	var an := str(name_of_body.get(a.get_instance_id(), a.name))
	var bn := str(name_of_body.get(b.get_instance_id(), b.name))
	# Each pair is reported by both bodies when both monitor contacts: keep one.
	if a is RigidBody3D and b is RigidBody3D and (b as RigidBody3D).contact_monitor and an > bn:
		return
	var ev := {"event": kind, "a": an, "b": bn, "sim_time": sim_time}
	pending_collisions.append(ev)
	collision_event.emit(ev)


# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------

func _body_xform(b: PhysicsBody3D) -> Transform3D:
	if b is RigidBody3D:
		return PhysicsServer3D.body_get_state(b.get_rid(), PhysicsServer3D.BODY_STATE_TRANSFORM)
	return b.global_transform


func _body_angvel(b: PhysicsBody3D) -> Vector3:
	if b is RigidBody3D:
		return PhysicsServer3D.body_get_state(b.get_rid(), PhysicsServer3D.BODY_STATE_ANGULAR_VELOCITY)
	return Vector3.ZERO


func _body_linvel(b: PhysicsBody3D) -> Vector3:
	if b is RigidBody3D:
		return PhysicsServer3D.body_get_state(b.get_rid(), PhysicsServer3D.BODY_STATE_LINEAR_VELOCITY)
	return Vector3.ZERO


## Composite ("locked subtree") inertia of the bodies distal to a joint,
## about the joint axis: sum of each body's own inertia about the axis
## direction plus m * (distance of its COM to the axis)^2. Used only by the
## implicit transmission integrator. A single-body estimate under-estimates it
## by up to ~100x for the base yaw with the arm extended, which made the gear
## coupling artificially soft.
func _subtree_inertia(rec: Dictionary, axis_world: Vector3, point_world: Vector3) -> float:
	var total := 0.0
	for b in rec["subtree"]:
		var body: RigidBody3D = b
		var x := _body_xform(body)
		var a_local := x.basis.inverse() * axis_world
		var il: Vector3 = body.inertia
		total += il.x * a_local.x * a_local.x + il.y * a_local.y * a_local.y + il.z * a_local.z * a_local.z
		var com := x * body.center_of_mass
		var r := (com - point_world).cross(axis_world)
		total += body.mass * r.length_squared()
	return maxf(total, 1e-9)


## Joint angle from the relative orientation of child vs parent (twist about
## the hinge axis), plus relative angular velocity about that axis.
func _measure_joint(rec: Dictionary) -> void:
	var pb: PhysicsBody3D = rec["parent"]
	var cb: PhysicsBody3D = rec["child"]
	var px := _body_xform(pb)
	var cx := _body_xform(cb)
	var rel := px.basis.inverse() * cx.basis
	var delta: Basis = rel * (rec["rel0"] as Basis).inverse()
	var q := delta.get_rotation_quaternion()
	var a: Vector3 = rec["axis_parent_local"]
	var twist := 2.0 * atan2(Vector3(q.x, q.y, q.z).dot(a), q.w)
	# Swing = rotation NOT about the hinge axis (should stay ~0: a large value
	# means the joint constraint is being violated).
	var tq := Quaternion(a.x * sin(twist / 2.0), a.y * sin(twist / 2.0), a.z * sin(twist / 2.0), cos(twist / 2.0))
	var swing_q := q * tq.inverse()
	rec["swing"] = 2.0 * acos(clampf(absf(swing_q.w), 0.0, 1.0))
	rec["point_world"] = px * (rec["joint_point_parent_local"] as Vector3)
	if twist > PI:
		twist -= TAU
	elif twist < -PI:
		twist += TAU
	rec["q"] = float(rec["q_home"]) + twist
	var axis_world := px.basis * a
	rec["axis_world"] = axis_world
	rec["qd"] = (_body_angvel(cb) - _body_angvel(pb)).dot(axis_world)


func tick(dt: float) -> void:
	for rec in joints:
		_measure_joint(rec)
		var servo: RefCounted = rec["servo"]
		var axis_world: Vector3 = rec["axis_world"]
		var cb: PhysicsBody3D = rec["child"]
		var pb: PhysicsBody3D = rec["parent"]
		var tau: float = servo.step(dt, rec["q"], rec["qd"], _subtree_inertia(rec, axis_world, rec["point_world"]))
		PhysicsServer3D.body_apply_torque(cb.get_rid(), axis_world * tau)
		if pb is RigidBody3D:
			# Motor (stator) and gear-friction reactions act on the servo housing,
			# which is bolted to the parent link.
			PhysicsServer3D.body_apply_torque(pb.get_rid(), axis_world * (servo.housing_reaction))
	_tick_gripper(dt)
	if pen_enabled:
		_tick_pen()
	sim_time += dt
	tick_count += 1
	if tick_count % trace_every == 0:
		_sample_trace()


func _sample_trace() -> void:
	for jn in servos:
		var s: RefCounted = servos[jn]
		var link_q: float = gripper["q"] if jn == "gripper" else float(joint_by_name[jn]["q"])
		if not traces.has(jn):
			traces[jn] = []
		var buf: Array = traces[jn]
		buf.append([sim_time, s.target, s.theta_s, link_q])
		while buf.size() > 2 and float(buf[0][0]) < sim_time - trace_window:
			buf.pop_front()
	trace = traces.get(trace_joint, [])


func _tick_gripper(dt: float) -> void:
	var palm: RigidBody3D = bodies["gripper"]
	var px := _body_xform(palm)
	var pv := _body_linvel(palm)
	var pw := _body_angvel(palm)
	var xs: Array = []
	var vs: Array = []
	var dirs: Array = []
	for f in gripper["fingers"]:
		var fb: RigidBody3D = f["body"]
		var fx := _body_xform(fb)
		var dir: Vector3 = (px.basis * (f["slide_local"] as Vector3)).normalized()
		var home_world: Vector3 = px * (f["home_pos_local"] as Vector3)
		var x := float(gripper["x_home"]) + (fx.origin - home_world).dot(dir)
		# Velocity of the finger relative to the palm point it slides on.
		var rel_v := _body_linvel(fb) - (pv + pw.cross(fx.origin - px.origin))
		var v := rel_v.dot(dir)
		f["x"] = x
		f["v"] = v
		xs.append(x)
		vs.append(v)
		dirs.append(dir)
	var x_mean := (float(xs[0]) + float(xs[1])) / 2.0
	var v_mean := (float(vs[0]) + float(vs[1])) / 2.0
	var r_p := float(gripper["r_p"])
	var q := float(gripper["closed_rad"]) + x_mean / r_p
	var qd := v_mean / r_p
	gripper["q"] = q
	gripper["qd"] = qd
	var servo: RefCounted = gripper["servo"]
	var m_f := float((gripper["fingers"][0]["body"] as RigidBody3D).mass)
	var j_fingers := 2.0 * m_f * r_p * r_p / float(gripper["eta"])
	var tau: float = servo.step(dt, q, qd, j_fingers)
	var f_each := float(gripper["eta"]) * tau / r_p / 2.0
	gripper["force_each"] = f_each
	var f_sync := float(gripper["k_sync"]) * (float(xs[0]) - float(xs[1])) + float(gripper["c_sync"]) * (float(vs[0]) - float(vs[1]))
	for i in range(2):
		var fb: RigidBody3D = gripper["fingers"][i]["body"]
		var dir: Vector3 = dirs[i]
		var sync_sign := -1.0 if i == 0 else 1.0
		var force := dir * (f_each + sync_sign * f_sync)
		var fpos := _body_xform(fb).origin
		PhysicsServer3D.body_apply_central_force(fb.get_rid(), force)
		PhysicsServer3D.body_apply_force(palm.get_rid(), -force, fpos - px.origin)


func pen_tip_world_robot() -> Vector3:
	var palm: RigidBody3D = bodies["gripper"]
	return Specs.g2r(_body_xform(palm) * pen_tip_local)


func _tick_pen() -> void:
	var tip := pen_tip_world_robot()
	var touching := tip.z - pen_tip_radius <= paper_top + 0.0003 and paper_rect.has_point(Vector2(tip.x, tip.y))
	if touching:
		var p := Vector2(tip.x, tip.y)
		if not pen_down or ink_strokes.is_empty():
			ink_strokes.append(PackedVector2Array([p]))
			ink_new.append({"stroke": ink_strokes.size() - 1, "p": [p.x, p.y]})
			ink_dirty = true
		else:
			var stroke: PackedVector2Array = ink_strokes[ink_strokes.size() - 1]
			if stroke[stroke.size() - 1].distance_to(p) >= 0.0002:
				stroke.append(p)
				ink_strokes[ink_strokes.size() - 1] = stroke
				ink_new.append({"stroke": ink_strokes.size() - 1, "p": [p.x, p.y]})
				ink_dirty = true
	pen_down = touching


func clear_ink() -> void:
	ink_strokes.clear()
	ink_new.clear()
	ink_dirty = true


func _process(_delta: float) -> void:
	if ink_dirty and ink_mesh != null:
		_rebuild_ink_mesh()
		ink_dirty = false


func _rebuild_ink_mesh() -> void:
	var im: ImmediateMesh = ink_mesh.mesh
	im.clear_surfaces()
	var any := false
	for stroke in ink_strokes:
		if (stroke as PackedVector2Array).size() >= 1:
			any = true
	if not any:
		return
	im.surface_begin(Mesh.PRIMITIVE_TRIANGLES)
	var z := paper_top + 0.00015
	var hw := ink_width / 2.0
	for stroke in ink_strokes:
		var s: PackedVector2Array = stroke
		if s.size() == 1:
			_ink_quad(im, s[0] + Vector2(-hw, 0), s[0] + Vector2(hw, 0), hw, z)
			continue
		for i in range(s.size() - 1):
			_ink_quad(im, s[i], s[i + 1], hw, z)
	im.surface_end()


func _ink_quad(im: ImmediateMesh, a: Vector2, b: Vector2, hw: float, z: float) -> void:
	var d := b - a
	if d.length() < 1e-6:
		d = Vector2(1, 0) * 1e-6
	var n := Vector2(-d.y, d.x).normalized() * hw
	var e := d.normalized() * hw * 0.5
	var p0 := Specs.r2g(Vector3(a.x - e.x + n.x, a.y - e.y + n.y, z))
	var p1 := Specs.r2g(Vector3(a.x - e.x - n.x, a.y - e.y - n.y, z))
	var p2 := Specs.r2g(Vector3(b.x + e.x - n.x, b.y + e.y - n.y, z))
	var p3 := Specs.r2g(Vector3(b.x + e.x + n.x, b.y + e.y + n.y, z))
	im.surface_add_vertex(p0)
	im.surface_add_vertex(p1)
	im.surface_add_vertex(p2)
	im.surface_add_vertex(p0)
	im.surface_add_vertex(p2)
	im.surface_add_vertex(p3)


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

func set_servo_command(joint_name: String, pulse_us: float, powered: bool) -> bool:
	if not servos.has(joint_name):
		return false
	var s: RefCounted = servos[joint_name]
	s.set_pulse(pulse_us)
	s.powered = powered
	return true


func configure_servo(joint_name: String, params: Dictionary) -> bool:
	if not servos.has(joint_name):
		return false
	var s: RefCounted = servos[joint_name]
	for key in ["kp", "ki", "kd", "integral_limit"]:
		if params.has(key):
			s.set(key, float(params[key]))
	for key in ["enable_deadband", "enable_backlash", "enable_jitter"]:
		if params.has(key):
			s.set(key, bool(params[key]))
	return true


# ---------------------------------------------------------------------------
# Telemetry
# ---------------------------------------------------------------------------

func tcp_robot() -> Dictionary:
	var palm: RigidBody3D = bodies["gripper"]
	var g := specs["arm"]["links"]["gripper"] as Dictionary
	var off: Array = g["pen_tcp_offset"] if pen_enabled else g["tcp_offset"]
	var x := _body_xform(palm)
	var p := Specs.g2r(x * Specs.r2g(off))
	var tool_axis := Specs.g2r(x.basis * Specs.r2g([1, 0, 0]))
	return {"position": Specs.vec_to_array(p), "tool_axis": Specs.vec_to_array(tool_axis)}


func telemetry(include_ink: bool = true, drain: bool = true) -> Dictionary:
	var jt := {}
	for rec in joints:
		_measure_joint(rec)
		var s: RefCounted = rec["servo"]
		var d: Dictionary = s.telemetry()
		d["link_pos"] = rec["q"]
		d["link_vel"] = rec["qd"]
		d["swing"] = rec["swing"]
		d["axis_inertia"] = _subtree_inertia(rec, rec["axis_world"], rec["point_world"])
		d["soft_limits"] = rec["soft_limits"]
		jt[rec["name"]] = d
	var gs: RefCounted = gripper["servo"]
	var gd: Dictionary = gs.telemetry()
	gd["link_pos"] = gripper["q"]
	gd["link_vel"] = gripper.get("qd", 0.0)
	var opening := 0.0
	var finger_contacts := []
	for f in gripper["fingers"]:
		opening += float(f["x"])
		var names := []
		for b in (f["body"] as RigidBody3D).get_colliding_bodies():
			names.append(str(name_of_body.get(b.get_instance_id(), b.name)))
		finger_contacts.append(names)
	gd["opening"] = opening + 2.0 * float(gripper["closed_half"])
	gd["finger_force_each"] = gripper["force_each"]
	gd["finger_contacts"] = finger_contacts
	gd["soft_limits"] = gripper["soft_limits"]
	var objs := {}
	for id in objects:
		var ob: RigidBody3D = objects[id]
		var ox := _body_xform(ob)
		var contacts := []
		for b in ob.get_colliding_bodies():
			contacts.append(str(name_of_body.get(b.get_instance_id(), b.name)))
		var held := contacts.has("finger_left") and contacts.has("finger_right")
		objs[id] = {
			"position": Specs.vec_to_array(Specs.g2r(ox.origin)),
			"velocity": Specs.vec_to_array(Specs.g2r(_body_linvel(ob))),
			"tilt_deg": rad_to_deg(acos(clampf((ox.basis * Vector3.UP).dot(Vector3.UP), -1.0, 1.0))),
			"contacts": contacts,
			"held": held,
		}
	var base_info := {"mount": str(specs["arm"].get("base_mount", "clamped"))}
	if bodies["base"] is RigidBody3D:
		var bx := _body_xform(bodies["base"])
		base_info["position"] = Specs.vec_to_array(Specs.g2r(bx.origin))
		base_info["tilt_deg"] = rad_to_deg(acos(clampf((bx.basis * Vector3.UP).dot(Vector3.UP), -1.0, 1.0)))
	var out := {
		"tick": tick_count,
		"sim_time": sim_time,
		"base": base_info,
		"joints": jt,
		"gripper": gd,
		"tcp": tcp_robot(),
		"objects": objs,
		"collisions": pending_collisions.duplicate() if drain else [],
		"tool": tool,
	}
	if drain:
		pending_collisions.clear()
	if pen_enabled:
		out["pen"] = {"tip": Specs.vec_to_array(pen_tip_world_robot()), "down": pen_down, "strokes": ink_strokes.size()}
		if include_ink and drain:
			out["pen"]["ink_new"] = ink_new.duplicate()
			ink_new.clear()
	return out


func mass_report() -> Dictionary:
	var out := {}
	for n in bodies:
		var b: Node = bodies[n]
		if b.has_meta("mass_properties"):
			out[n] = b.get_meta("mass_properties")
	return out


func ink_snapshot() -> Array:
	var out := []
	for stroke in ink_strokes:
		var pts := []
		for p in (stroke as PackedVector2Array):
			pts.append([p.x, p.y])
		out.append(pts)
	return out
