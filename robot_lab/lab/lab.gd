extends Node3D
## Lab root. Wires the plant (physics), environment (visual), end-effector
## camera (sensor), HUD (view) and bridge (Robot API transport) together.
##
## Command line (after `--`):
##   --port=47011        bridge port
##   --specs=/abs/ARM_SPECS.json
##   --tool=gripper|pen
##   --seed=1337
##   --free-run          run physics without a controller (visual demo only;
##                       not deterministic with respect to any controller)
##   --view=overview     initial camera preset
## This scene is also the unit that a future Abiyss Office "robot room" can
## instance: it only needs a parent Node3D and an optional bridge port.

const Specs := preload("res://sim/specs.gd")
const ArmPlant := preload("res://sim/arm_plant.gd")
const CameraSensor := preload("res://sim/camera_sensor.gd")
const LabEnvironment := preload("res://lab/environment.gd")
const Hud := preload("res://ui/hud.gd")
const Bridge := preload("res://bridge/sim_bridge.gd")

var specs: Dictionary
var specs_path: String = ""
var plant: Node3D
var environment: Node3D
var camera_sensor: SubViewport
var hud: CanvasLayer
var bridge: Node
var main_camera: Camera3D
var orbit_target := Vector3.ZERO
var orbit_yaw := 0.0
var orbit_pitch := 0.0
var orbit_dist := 1.0
var dragging := false

const VIEWS := {
	# robot-frame eye and target (m); optional third entry = robot-frame up vector
	"overview": [[0.50, -0.36, 0.32], [0.10, 0.0, 0.05]],
	"paper_top": [[0.175, -0.008, 0.34], [0.175, -0.008, 0.0], [1.0, 0.0, 0.0]],
	"front": [[0.58, 0.0, 0.26], [0.10, 0.0, 0.07]],
	"behind": [[-0.30, -0.10, 0.36], [0.16, 0.0, 0.02]],
	"paper": [[0.02, -0.17, 0.30], [0.172, -0.005, 0.0]],
	"side": [[0.12, -0.60, 0.16], [0.10, 0.0, 0.08]],
	"objects": [[0.46, -0.34, 0.30], [0.16, 0.0, 0.02]],
	"top": [[0.17, 0.0, 0.55], [0.17, 0.0, 0.0]],
	"gripper": [[0.34, -0.22, 0.22], [0.15, 0.0, 0.05]],
}


func _ready() -> void:
	var args := _parse_args()
	specs_path = str(args.get("specs", ""))
	specs = Specs.load_specs(specs_path)
	if specs.is_empty():
		push_error("lab: specs could not be loaded; quitting")
		get_tree().quit(2)
		return
	environment = LabEnvironment.new()
	environment.name = "environment"
	add_child(environment)
	environment.build(specs)
	_build_plant(str(args.get("tool", "gripper")), int(args.get("seed", int(Specs.v(specs["simulation"], "default_seed", 1337)))))
	camera_sensor = CameraSensor.new()
	camera_sensor.name = "ee_camera_viewport"
	add_child(camera_sensor)
	camera_sensor.setup(plant, specs["arm"]["links"]["gripper"]["camera"])
	main_camera = Camera3D.new()
	main_camera.fov = 42.0
	main_camera.near = 0.01
	main_camera.far = 20.0
	add_child(main_camera)
	main_camera.current = true
	set_view(str(args.get("view", "overview")))
	bridge = Bridge.new()
	bridge.name = "bridge"
	bridge.lab = self
	bridge.port = int(args.get("port", 47011))
	bridge.free_run = args.has("free-run")
	add_child(bridge)
	if DisplayServer.get_name() != "headless":
		hud = Hud.new()
		hud.name = "hud"
		add_child(hud)
		hud.build(self, bridge)
		hud.set_camera_texture(camera_sensor.get_texture())
	print("[lab] ready: renderer=%s adapter=%s display=%s" % [RenderingServer.get_current_rendering_method(), RenderingServer.get_video_adapter_name(), DisplayServer.get_name()])


func _parse_args() -> Dictionary:
	var out := {}
	for a in OS.get_cmdline_user_args():
		var s := str(a)
		if s.begins_with("--"):
			s = s.substr(2)
		var eq := s.find("=")
		if eq >= 0:
			out[s.substr(0, eq)] = s.substr(eq + 1)
		else:
			out[s] = true
	return out


func _build_plant(tool: String, seed_value: int) -> void:
	plant = ArmPlant.new()
	plant.name = "plant"
	add_child(plant)
	plant.build(specs, tool, seed_value)


func reset_world(tool: String, seed_value: int, specs_override: Variant) -> void:
	if specs_override is Dictionary and not (specs_override as Dictionary).is_empty():
		specs = specs_override
	var old := plant
	remove_child(old)
	old.free()
	_build_plant(tool, seed_value)
	camera_sensor.set_plant(plant)
	if hud != null:
		plant.trace_joint = hud.trace_joint


func camera_available() -> bool:
	return DisplayServer.get_name() != "headless"


func capture_camera() -> Dictionary:
	return await camera_sensor.capture()


func hud_status(status: Dictionary) -> void:
	if hud != null:
		hud.set_status(status)
	if status.has("view"):
		set_view(str(status["view"]))
	if hud != null and status.has("trace_joint"):
		hud.select_trace(str(status["trace_joint"]))


func set_view(view_name: String) -> void:
	if not VIEWS.has(view_name):
		view_name = "overview"
	var v: Array = VIEWS[view_name]
	var eye := Specs.r2g(v[0])
	orbit_target = Specs.r2g(v[1])
	if v.size() > 2:
		# Fixed view with an explicit up vector (e.g. top-down on the paper so
		# the text reads upright). Mouse orbit resumes from the default views.
		main_camera.position = eye
		main_camera.look_at(orbit_target, Specs.r2g(v[2]))
		return
	var d := eye - orbit_target
	orbit_dist = d.length()
	orbit_yaw = atan2(d.x, d.z)
	orbit_pitch = asin(d.y / orbit_dist)
	_apply_orbit()


func _apply_orbit() -> void:
	var d := Vector3(sin(orbit_yaw) * cos(orbit_pitch), sin(orbit_pitch), cos(orbit_yaw) * cos(orbit_pitch)) * orbit_dist
	main_camera.position = orbit_target + d
	main_camera.look_at(orbit_target, Vector3.UP)


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_RIGHT or mb.button_index == MOUSE_BUTTON_MIDDLE:
			dragging = mb.pressed
		elif mb.button_index == MOUSE_BUTTON_WHEEL_UP and mb.pressed:
			orbit_dist = maxf(orbit_dist * 0.9, 0.08)
			_apply_orbit()
		elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN and mb.pressed:
			orbit_dist = minf(orbit_dist * 1.1, 4.0)
			_apply_orbit()
	elif event is InputEventMouseMotion and dragging:
		var mm := event as InputEventMouseMotion
		orbit_yaw -= mm.relative.x * 0.006
		orbit_pitch = clampf(orbit_pitch + mm.relative.y * 0.006, -0.2, 1.5)
		_apply_orbit()
