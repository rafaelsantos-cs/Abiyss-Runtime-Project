extends Node3D
## Visual-only lab environment: room, bench frame, lights, test-area markings
## and the status beacon. Nothing here takes part in the physics of the arm
## (the bench top and paper collision live in the plant).

const Specs := preload("res://sim/specs.gd")

var beacon_mesh: MeshInstance3D
var beacon_light: OmniLight3D
var beacon_mat: StandardMaterial3D
var labels: Array = []


func _mat(hex: String, roughness: float = 0.8, metallic: float = 0.0) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = Color(hex)
	m.roughness = roughness
	m.metallic = metallic
	return m


func _box(size_g: Vector3, pos_g: Vector3, mat: Material, parent: Node = self) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = size_g
	mi.mesh = bm
	mi.position = pos_g
	mi.material_override = mat
	parent.add_child(mi)
	return mi


func build(specs: Dictionary) -> void:
	var ws: Dictionary = specs["workspace"]
	var bench: Dictionary = ws["bench"]
	var bc := Specs.r2g(bench["center"])
	var bs := Specs.r2g_size(bench["size"])
	var top_y := bc.y + bs.y / 2.0
	var floor_y := top_y - 0.76
	# World environment
	var we := WorldEnvironment.new()
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color("#14181c")
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color("#9aa7b4")
	env.ambient_light_energy = 0.5
	env.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	we.environment = env
	add_child(we)
	# Key light with shadows, fill light
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-58, 35, 0)
	sun.light_energy = 0.9
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 3.0
	add_child(sun)
	var fill := DirectionalLight3D.new()
	fill.rotation_degrees = Vector3(-25, -140, 0)
	fill.light_energy = 0.35
	add_child(fill)
	# Floor and walls
	_box(Vector3(8, 0.02, 8), Vector3(0, floor_y - 0.01, 0), _mat("#23282d", 0.95))
	_box(Vector3(8, 3.0, 0.05), Vector3(0, floor_y + 1.5, -1.2), _mat("#2c3238", 0.95))
	_box(Vector3(0.05, 3.0, 8), Vector3(-1.4, floor_y + 1.5, 0), _mat("#2a2f35", 0.95))
	# Safety stripe on the back wall
	_box(Vector3(8, 0.06, 0.01), Vector3(0, floor_y + 1.18, -1.17), _mat("#e9a23b", 0.6))
	# Bench legs and frame (the top itself is the physics bench in the plant)
	var leg_mat := _mat("#5a636d", 0.5, 0.6)
	var hx := bs.x / 2.0 - 0.05
	var hz := bs.z / 2.0 - 0.05
	for sx in [-1, 1]:
		for sz in [-1, 1]:
			_box(Vector3(0.04, 0.73, 0.04), Vector3(bc.x + sx * hx, floor_y + 0.365, bc.z + sz * hz), leg_mat)
	_box(Vector3(bs.x - 0.1, 0.04, 0.04), Vector3(bc.x, floor_y + 0.18, bc.z - hz), leg_mat)
	# Test-area tape around the paper and the object fixtures
	var tape := _mat("#e9a23b", 0.6)
	var paper: Dictionary = ws["paper"]
	_frame_tape(Specs.r2g(paper["center"]), Specs.r2g_size(paper["size"]), top_y, 0.006, tape)
	for o in ws["objects"]:
		var p := Specs.r2g(o["position"])
		var fixture := Node3D.new()
		fixture.position = Vector3(p.x, 0, p.z)
		fixture.rotation = Vector3(0, deg_to_rad(float(o.get("yaw_deg", 0.0))), 0)
		add_child(fixture)
		_frame_tape(Vector3.ZERO, Vector3(0.034, 0, 0.034), top_y, 0.002, _mat("#6f7a85", 0.8), fixture)
		var l := Label3D.new()
		l.text = "%s  %d g" % [o["id"], int(round(float(o["mass"]) * 1000.0))]
		l.font_size = 64
		l.pixel_size = 0.00018
		l.modulate = Color("#c3cad1")
		l.outline_size = 8
		l.outline_modulate = Color("#101316")
		l.rotation_degrees = Vector3(-90, 0, 0)
		l.position = Vector3(p.x, top_y + 0.0006, p.z + 0.03)
		add_child(l)
		labels.append(l)
	var dz: Dictionary = ws["drop_zone"]
	var dzc := Specs.r2g(dz["center"])
	_frame_tape(Vector3(dzc.x, top_y, dzc.z), Vector3(float(dz["size"][0]), 0, float(dz["size"][1])), top_y, 0.003, _mat("#5fd3b3", 0.7))
	var dl := Label3D.new()
	dl.text = "DROP"
	dl.font_size = 64
	dl.pixel_size = 0.00022
	dl.modulate = Color("#5fd3b3")
	dl.rotation_degrees = Vector3(-90, 0, 0)
	dl.position = Vector3(dzc.x, top_y + 0.0006, dzc.z + float(dz["size"][1]) / 2.0 + 0.012)
	add_child(dl)
	# Bench sign
	var sign := Label3D.new()
	sign.text = "ABIYSS ROBOTICS LAB · BENCH 01"
	sign.font_size = 72
	sign.pixel_size = 0.0006
	sign.modulate = Color("#e6e9ec")
	sign.position = Vector3(0.2, floor_y + 1.0, -1.165)
	add_child(sign)
	# Status beacon (post at the back-left corner of the bench)
	var post_pos := Specs.r2g([-0.12, 0.26, 0.0])
	_box(Vector3(0.012, 0.16, 0.012), Vector3(post_pos.x, top_y + 0.08, post_pos.z), _mat("#5a636d", 0.5, 0.6))
	beacon_mesh = MeshInstance3D.new()
	var cyl := CylinderMesh.new()
	cyl.top_radius = 0.018
	cyl.bottom_radius = 0.018
	cyl.height = 0.035
	beacon_mesh.mesh = cyl
	beacon_mat = StandardMaterial3D.new()
	beacon_mat.emission_enabled = true
	beacon_mesh.material_override = beacon_mat
	beacon_mesh.position = Vector3(post_pos.x, top_y + 0.18, post_pos.z)
	add_child(beacon_mesh)
	beacon_light = OmniLight3D.new()
	beacon_light.omni_range = 0.22
	beacon_light.position = beacon_mesh.position
	add_child(beacon_light)
	set_beacon("NO_CONTROLLER")


func _frame_tape(center: Vector3, size: Vector3, top_y: float, margin: float, mat: Material, parent: Node = self) -> void:
	var w := 0.004
	var y := top_y + 0.0004
	var hx := size.x / 2.0 + margin
	var hz := size.z / 2.0 + margin
	_box(Vector3(2 * hx + w, 0.0008, w), Vector3(center.x, y, center.z - hz), mat, parent)
	_box(Vector3(2 * hx + w, 0.0008, w), Vector3(center.x, y, center.z + hz), mat, parent)
	_box(Vector3(w, 0.0008, 2 * hz), Vector3(center.x - hx, y, center.z), mat, parent)
	_box(Vector3(w, 0.0008, 2 * hz), Vector3(center.x + hx, y, center.z), mat, parent)


const STATE_COLORS := {
	"SAFE": "#2fbf8f",
	"WARNING": "#f2a93b",
	"STALL": "#b05cff",
	"FAULT": "#ff7a1a",
	"EMERGENCY_STOP": "#ff2a1f",
	"NO_CONTROLLER": "#56606a",
}


func set_beacon(state: String) -> void:
	var c := Color(STATE_COLORS.get(state, "#56606a"))
	beacon_mat.albedo_color = c
	beacon_mat.emission = c
	beacon_mat.emission_energy_multiplier = 2.5 if state != "NO_CONTROLLER" else 0.3
	beacon_light.light_color = c
	beacon_light.light_energy = 0.0 if state in ["SAFE", "NO_CONTROLLER"] else 0.6
