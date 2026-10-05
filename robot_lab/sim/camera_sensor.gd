extends SubViewport
## End-effector camera: a sensor of the simulation, not a UI element.
##
## Renders the scene from a pose rigidly attached to the gripper link. The
## pose comes from the physics state (not from the interpolated node), so a
## frame corresponds to the simulated instant at which it is captured.

const Specs := preload("res://sim/specs.gd")

var cam: Camera3D
var mount_offset_robot: Vector3
var look_axis_robot: Vector3
var up_axis_robot: Vector3
var fov_deg: float = 70.0
var plant: Node3D
var frame_counter: int = 0


func setup(p_plant: Node3D, cam_spec: Dictionary) -> void:
	plant = p_plant
	var res: Array = cam_spec["resolution"]
	size = Vector2i(int(res[0]), int(res[1]))
	render_target_update_mode = SubViewport.UPDATE_ALWAYS
	msaa_3d = Viewport.MSAA_DISABLED
	mount_offset_robot = Vector3(cam_spec["offset"][0], cam_spec["offset"][1], cam_spec["offset"][2])
	look_axis_robot = Vector3(cam_spec["look_axis"][0], cam_spec["look_axis"][1], cam_spec["look_axis"][2]).normalized()
	up_axis_robot = Vector3(cam_spec["up_axis"][0], cam_spec["up_axis"][1], cam_spec["up_axis"][2]).normalized()
	fov_deg = float(cam_spec["fov_deg"])
	if cam == null:
		cam = Camera3D.new()
		cam.name = "ee_camera"
		add_child(cam)
	cam.fov = fov_deg
	cam.near = float(cam_spec["near"])
	cam.far = float(cam_spec["far"])
	cam.current = true
	sync_pose()


func set_plant(p_plant: Node3D) -> void:
	plant = p_plant
	sync_pose()


func pose_godot() -> Transform3D:
	var palm: RigidBody3D = plant.bodies["gripper"]
	var x: Transform3D = PhysicsServer3D.body_get_state(palm.get_rid(), PhysicsServer3D.BODY_STATE_TRANSFORM)
	var origin := x * Specs.r2g(mount_offset_robot)
	var fwd := (x.basis * Specs.r2g(look_axis_robot)).normalized()
	var up := (x.basis * Specs.r2g(up_axis_robot)).normalized()
	# Camera3D looks along its local -Z.
	var z := -fwd
	var xa := up.cross(z).normalized()
	var ya := z.cross(xa).normalized()
	return Transform3D(Basis(xa, ya, z), origin)


func sync_pose() -> void:
	if cam != null and plant != null and plant.bodies.has("gripper"):
		cam.global_transform = pose_godot()


func _process(_delta: float) -> void:
	sync_pose()


## Render one frame now and return it with metadata.
func capture() -> Dictionary:
	sync_pose()
	await RenderingServer.frame_post_draw
	sync_pose()
	await RenderingServer.frame_post_draw
	var img := get_texture().get_image()
	frame_counter += 1
	var pose := pose_godot()
	var robot_pos := Specs.g2r(pose.origin)
	var robot_fwd := Specs.g2r(-pose.basis.z)
	var png := img.save_png_to_buffer()
	var fy := float(size.y) / 2.0 / tan(deg_to_rad(fov_deg) / 2.0)
	return {
		"frame_id": frame_counter,
		"sim_time": plant.sim_time,
		"tick": plant.tick_count,
		"width": img.get_width(),
		"height": img.get_height(),
		"format": "png",
		"encoding": "base64",
		"data": Marshalls.raw_to_base64(png),
		"pose": {"position": Specs.vec_to_array(robot_pos), "forward": Specs.vec_to_array(robot_fwd)},
		"intrinsics": {"fov_y_deg": fov_deg, "fx": fy, "fy": fy, "cx": float(size.x) / 2.0, "cy": float(size.y) / 2.0},
		"sensor": "SimulationCamera",
	}
