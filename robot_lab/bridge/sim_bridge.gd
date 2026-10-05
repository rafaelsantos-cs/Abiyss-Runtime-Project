extends Node
## Lockstep TCP bridge between the Python Robot API (SimulationBackend) and
## the physics plant.
##
## Protocol: one JSON object per line, request/response, 127.0.0.1 only.
## Determinism: physics only advances while a `step` request has budget.
## For every physics tick either (a) the plant controller ran and the physics
## server steps, or (b) PhysicsServer3D is inactive and nothing moves.
## `PhysicsServer3D.set_active(false)` makes JoltPhysicsServer3D::step return
## early (verified in Godot 4.7.2 source, see ARM_SPECS.json S9).

signal client_connected
signal client_disconnected

const PROTOCOL_VERSION := 1

var lab: Node                       # lab.gd (owns plant, camera, hud)
var port: int = 47011
var server := TCPServer.new()
var peer: StreamPeerTCP = null
var rx := PackedByteArray()
var budget: int = 0
var pending_step_id: Variant = null
var busy: bool = false
var free_run: bool = false          # simulate without a controller (visual demo)
var ui_events: Array = []
var frame_wait_exhausted: bool = false
var wait_budget_ms: float = 5.0     # how long a physics iteration waits for the next request


func _ready() -> void:
	process_physics_priority = -1000
	process_priority = -1000
	var err := server.listen(port, "127.0.0.1")
	if err != OK:
		push_error("bridge: cannot listen on 127.0.0.1:%d (%s)" % [port, error_string(err)])
	else:
		print("[bridge] listening on 127.0.0.1:%d" % port)
	PhysicsServer3D.set_active(free_run)


func connected() -> bool:
	return peer != null and peer.get_status() == StreamPeerTCP.STATUS_CONNECTED


func _physics_process(delta: float) -> void:
	# Serve the controller inside the physics loop so several control periods
	# can run per rendered frame (rendering is slow on software GL).
	if connected():
		if pending_step_id != null and budget == 0:
			_finish_step()
		if budget == 0 and pending_step_id == null and not busy and not frame_wait_exhausted:
			_pump(wait_budget_ms)
			if budget == 0:
				frame_wait_exhausted = true
	if budget > 0:
		PhysicsServer3D.set_active(true)
		lab.plant.tick(delta)
		budget -= 1
	elif free_run and not connected():
		PhysicsServer3D.set_active(true)
		lab.plant.tick(delta)
	else:
		PhysicsServer3D.set_active(false)


func _process(_delta: float) -> void:
	frame_wait_exhausted = false
	if server.is_connection_available():
		var incoming := server.take_connection()
		if connected():
			incoming.disconnect_from_host()   # one controller at a time
		else:
			peer = incoming
			peer.set_no_delay(true)
			rx.clear()
			print("[bridge] controller connected")
			client_connected.emit()
	if peer == null:
		return
	peer.poll()
	var status := peer.get_status()
	if status != StreamPeerTCP.STATUS_CONNECTED:
		if status == StreamPeerTCP.STATUS_NONE or status == StreamPeerTCP.STATUS_ERROR:
			print("[bridge] controller disconnected")
			peer = null
			budget = 0
			pending_step_id = null
			busy = false
			client_disconnected.emit()
		return
	if pending_step_id != null and budget == 0:
		_finish_step()
	if not busy and pending_step_id == null:
		_pump(0)


func _finish_step() -> void:
	var reply := {"id": pending_step_id, "ok": true, "state": lab.plant.telemetry(), "ui_events": _drain_ui()}
	pending_step_id = null
	_send(reply)


## Read and handle request lines until a step starts, an async op starts, or
## no complete line is available within `wait_ms`.
func _pump(wait_ms: float) -> void:
	var deadline := Time.get_ticks_usec() + int(wait_ms * 1000.0)
	while true:
		peer.poll()
		var avail := peer.get_available_bytes()
		if avail > 0:
			var res: Array = peer.get_partial_data(avail)
			if res[0] == OK:
				rx.append_array(res[1])
		var handled := false
		while not busy and pending_step_id == null:
			var nl := rx.find(10)
			if nl < 0:
				break
			var line := rx.slice(0, nl).get_string_from_utf8()
			rx = rx.slice(nl + 1)
			if line.strip_edges() == "":
				continue
			handled = true
			_handle_line(line)
		if busy or pending_step_id != null:
			return
		if Time.get_ticks_usec() >= deadline:
			return
		if not handled:
			OS.delay_usec(50)


func _drain_ui() -> Array:
	var out := ui_events.duplicate()
	ui_events.clear()
	return out


func push_ui_event(ev: Dictionary) -> void:
	ui_events.append(ev)


func _send(obj: Dictionary) -> void:
	if not connected():
		return
	var text := JSON.stringify(obj, "", false, true) + "\n"
	peer.put_data(text.to_utf8_buffer())


func _error(id: Variant, msg: String) -> void:
	_send({"id": id, "ok": false, "error": msg})


func _handle_line(line: String) -> void:
	var parsed: Variant = JSON.parse_string(line)
	if not (parsed is Dictionary):
		_error(null, "invalid JSON request")
		return
	var req: Dictionary = parsed
	var id: Variant = req.get("id", null)
	var op := str(req.get("op", ""))
	match op:
		"hello":
			_send({"id": id, "ok": true, "protocol": PROTOCOL_VERSION, "engine": Engine.get_version_info()["string"],
				"physics_engine": str(ProjectSettings.get_setting("physics/3d/physics_engine")),
				"physics_hz": Engine.physics_ticks_per_second,
				"renderer": RenderingServer.get_current_rendering_method(),
				"video_adapter": RenderingServer.get_video_adapter_name(),
				"headless": DisplayServer.get_name() == "headless",
				"camera_available": lab.camera_available(),
				"tool": lab.plant.tool, "seed": lab.plant.seed_value})
		"reset":
			lab.reset_world(str(req.get("tool", "gripper")), int(req.get("seed", 1337)), req.get("specs", null))
			_send({"id": id, "ok": true, "state": lab.plant.telemetry(), "masses": lab.plant.mass_report()})
		"step":
			var ticks := int(req.get("ticks", 20))
			if ticks < 1 or ticks > 100000:
				_error(id, "ticks out of range")
				return
			var cmds: Dictionary = req.get("servos", {})
			for jn in cmds:
				var c: Dictionary = cmds[jn]
				if not lab.plant.set_servo_command(str(jn), float(c.get("pulse_us", 1500.0)), bool(c.get("power", true))):
					_error(id, "unknown servo: %s" % jn)
					return
			budget = ticks
			pending_step_id = id if id != null else -1
		"configure":
			if not lab.plant.configure_servo(str(req.get("servo", "")), req.get("params", {})):
				_error(id, "unknown servo")
				return
			_send({"id": id, "ok": true})
		"state":
			_send({"id": id, "ok": true, "state": lab.plant.telemetry(bool(req.get("include_ink", false)))})
		"ink":
			_send({"id": id, "ok": true, "strokes": lab.plant.ink_snapshot()})
		"clear_ink":
			lab.plant.clear_ink()
			_send({"id": id, "ok": true})
		"hud":
			lab.hud_status(req.get("status", {}))
			_send({"id": id, "ok": true})
		"view":
			lab.set_view(str(req.get("name", "overview")))
			_send({"id": id, "ok": true})
		"camera":
			busy = true
			_camera_async(id)
		"screenshot":
			busy = true
			_screenshot_async(id, str(req.get("path", "")))
		"quit":
			_send({"id": id, "ok": true})
			get_tree().quit()
		_:
			_error(id, "unknown op: %s" % op)


func _camera_async(id: Variant) -> void:
	if not lab.camera_available():
		busy = false
		_error(id, "camera unavailable: renderer is headless (dummy). Run with a display (e.g. xvfb-run) to render frames.")
		return
	var frame: Dictionary = await lab.capture_camera()
	busy = false
	frame["id"] = id
	frame["ok"] = true
	_send(frame)


func _screenshot_async(id: Variant, path: String) -> void:
	if DisplayServer.get_name() == "headless":
		busy = false
		_error(id, "screenshot unavailable in headless mode")
		return
	if path == "" or not path.is_absolute_path():
		busy = false
		_error(id, "screenshot path must be absolute")
		return
	await RenderingServer.frame_post_draw
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	var err := img.save_png(path)
	busy = false
	if err != OK:
		_error(id, "cannot save screenshot: %s" % error_string(err))
	else:
		_send({"id": id, "ok": true, "path": path, "width": img.get_width(), "height": img.get_height()})
