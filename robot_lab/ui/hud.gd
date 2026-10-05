extends CanvasLayer
## Telemetry HUD. Read-only view of (a) the plant telemetry and (b) the
## controller status pushed by the Python Robot API through the bridge.
## Buttons do not touch the simulation: they become ui_events that the
## controller receives on its next step and decides what to do with.

const STATE_COLORS := {
	"SAFE": Color("#1f6f5c"),
	"WARNING": Color("#c9861f"),
	"STALL": Color("#7a3fb0"),
	"FAULT": Color("#a3520f"),
	"EMERGENCY_STOP": Color("#b3261e"),
	"NO_CONTROLLER": Color("#3a424a"),
}
const JOINTS := ["base_yaw", "shoulder", "elbow", "wrist", "gripper"]

var lab: Node
var bridge: Node
var status: Dictionary = {}
var font_mono: SystemFont

var badge: Label
var badge_panel: PanelContainer
var time_label: Label
var backend_label: Label
var joint_cells: Dictionary = {}
var tcp_label: Label
var gripper_label: Label
var pid_label: Label
var queue_label: RichTextLabel
var journal_label: RichTextLabel
var action_label: Label
var cam_rect: TextureRect
var cam_title: Label
var chart: Control
var chart_title: Label
var trace_joint: String = "shoulder"


func _style(bg: Color, border: Color = Color("#2a3036"), radius: int = 6) -> StyleBoxFlat:
	var sb := StyleBoxFlat.new()
	sb.bg_color = bg
	sb.border_color = border
	sb.set_border_width_all(1)
	sb.set_corner_radius_all(radius)
	sb.content_margin_left = 12
	sb.content_margin_right = 12
	sb.content_margin_top = 8
	sb.content_margin_bottom = 8
	return sb


func _label(text: String, size: int = 14, color: Color = Color("#e6e9ec"), mono: bool = false) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	if mono:
		l.add_theme_font_override("font", font_mono)
	return l


func _section(title: String) -> VBoxContainer:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 4)
	v.add_child(_label(title, 12, Color("#9aa4ae"), true))
	return v


func _button(text: String, name_id: String, danger: bool = false) -> Button:
	var b := Button.new()
	b.text = text
	b.custom_minimum_size = Vector2(0, 40)
	b.add_theme_font_size_override("font_size", 14)
	var normal := _style(Color("#b3261e") if danger else Color("#23292f"), Color("#ff8a80") if danger else Color("#3a424a"), 4)
	var hover := _style(Color("#d0362c") if danger else Color("#2e353c"), Color("#ffb3ad") if danger else Color("#56606a"), 4)
	b.add_theme_stylebox_override("normal", normal)
	b.add_theme_stylebox_override("hover", hover)
	b.add_theme_stylebox_override("pressed", hover)
	b.add_theme_color_override("font_color", Color.WHITE)
	b.pressed.connect(func(): _on_button(name_id))
	return b


func _on_button(name_id: String) -> void:
	if bridge != null:
		bridge.push_ui_event({"event": "ui_button", "button": name_id, "sim_time": lab.plant.sim_time})
	if name_id.begins_with("trace:"):
		select_trace(name_id.substr(6))


func select_trace(joint: String) -> void:
	if not JOINTS.has(joint):
		return
	trace_joint = joint
	if lab.plant.trace_joint != joint:
		lab.plant.trace_joint = joint
		lab.plant.trace = lab.plant.traces.get(joint, [])


func build(p_lab: Node, p_bridge: Node) -> void:
	lab = p_lab
	bridge = p_bridge
	font_mono = SystemFont.new()
	font_mono.font_names = PackedStringArray(["DejaVu Sans Mono", "Liberation Mono", "monospace"])
	var root := Control.new()
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)

	# --- Top bar ------------------------------------------------------------
	var top := PanelContainer.new()
	top.add_theme_stylebox_override("panel", _style(Color(0.094, 0.11, 0.125, 0.94)))
	top.position = Vector2(12, 10)
	top.size = Vector2(1576, 60)
	root.add_child(top)
	var th := HBoxContainer.new()
	th.add_theme_constant_override("separation", 14)
	top.add_child(th)
	var titles := VBoxContainer.new()
	titles.add_theme_constant_override("separation", 0)
	titles.add_child(_label("ABIYSS RUNTIME · ROBOT API", 11, Color("#9aa4ae"), true))
	titles.add_child(_label("Robotics Lab — bench 01", 20))
	titles.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	th.add_child(titles)
	backend_label = _label("BACKEND: SIMULATION · Godot/Jolt", 13, Color("#c3cad1"), true)
	th.add_child(backend_label)
	time_label = _label("t_sim = 0.000 s", 13, Color("#c3cad1"), true)
	time_label.custom_minimum_size = Vector2(170, 0)
	th.add_child(time_label)
	badge_panel = PanelContainer.new()
	badge_panel.add_theme_stylebox_override("panel", _style(STATE_COLORS["NO_CONTROLLER"], STATE_COLORS["NO_CONTROLLER"], 4))
	badge = _label("NO CONTROLLER", 15, Color.WHITE, true)
	badge.custom_minimum_size = Vector2(170, 0)
	badge.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	badge_panel.add_child(badge)
	th.add_child(badge_panel)
	for b in [["Write OI", "write_oi"], ["Pick test", "pick_test"], ["Home", "home"], ["Stop", "stop"], ["Reset", "reset"]]:
		th.add_child(_button(b[0], b[1]))
	th.add_child(_button("  E-STOP  ", "estop", true))

	# --- Right panel --------------------------------------------------------
	var right := PanelContainer.new()
	right.add_theme_stylebox_override("panel", _style(Color(0.094, 0.11, 0.125, 0.94)))
	right.position = Vector2(1046, 80)
	right.size = Vector2(542, 810)
	root.add_child(right)
	var rv := VBoxContainer.new()
	rv.add_theme_constant_override("separation", 10)
	right.add_child(rv)

	var js := _section("JOINTS · telemetry()   (deg, N·m, mA)")
	var grid := GridContainer.new()
	grid.columns = 8
	grid.add_theme_constant_override("h_separation", 10)
	grid.add_theme_constant_override("v_separation", 3)
	for h in ["joint", "servo", "tgt", "pos", "err", "τ", "τ/τmax", "state"]:
		grid.add_child(_label(h, 12, Color("#9aa4ae"), true))
	for jn in JOINTS:
		var cells := {}
		for key in ["joint", "servo", "tgt", "pos", "err", "tau", "pct", "state"]:
			var l := _label("—", 13, Color("#e6e9ec"), true)
			grid.add_child(l)
			cells[key] = l
		cells["joint"].text = jn
		joint_cells[jn] = cells
	js.add_child(grid)
	rv.add_child(js)

	tcp_label = _label("TCP —", 12, Color("#c3cad1"), true)
	rv.add_child(tcp_label)
	gripper_label = _label("gripper —", 12, Color("#c3cad1"), true)
	rv.add_child(gripper_label)

	var trace_row := HBoxContainer.new()
	trace_row.add_child(_label("trace:", 12, Color("#9aa4ae"), true))
	for jn in JOINTS:
		var tb := _button(jn, "trace:" + jn)
		tb.custom_minimum_size = Vector2(0, 28)
		tb.add_theme_font_size_override("font_size", 11)
		trace_row.add_child(tb)
	rv.add_child(trace_row)

	var pid := _section("SERVO CONTROLLER (PID) · selected joint")
	pid_label = _label("—", 12, Color("#e6e9ec"), true)
	pid_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	pid.add_child(pid_label)
	rv.add_child(pid)

	var q := _section("ACTION QUEUE")
	action_label = _label("idle", 13, Color("#f2a93b"), true)
	q.add_child(action_label)
	queue_label = RichTextLabel.new()
	queue_label.fit_content = true
	queue_label.custom_minimum_size = Vector2(500, 70)
	queue_label.add_theme_font_override("normal_font", font_mono)
	queue_label.add_theme_font_size_override("normal_font_size", 12)
	q.add_child(queue_label)
	rv.add_child(q)

	var jr := _section("JOURNAL · telemetry.jsonl (latest)")
	journal_label = RichTextLabel.new()
	journal_label.custom_minimum_size = Vector2(500, 210)
	journal_label.scroll_following = true
	journal_label.add_theme_font_override("normal_font", font_mono)
	journal_label.add_theme_font_size_override("normal_font_size", 11)
	journal_label.add_theme_color_override("default_color", Color("#a9b4be"))
	jr.add_child(journal_label)
	rv.add_child(jr)

	# --- Bottom left: camera + chart ----------------------------------------
	var camp := PanelContainer.new()
	camp.add_theme_stylebox_override("panel", _style(Color(0.02, 0.027, 0.03, 0.96), Color("#f2a93b"), 4))
	camp.position = Vector2(12, 618)
	camp.size = Vector2(344, 272)
	root.add_child(camp)
	var cv := VBoxContainer.new()
	camp.add_child(cv)
	cam_title = _label("cam.frame() · end effector", 12, Color("#f2a93b"), true)
	cv.add_child(cam_title)
	cam_rect = TextureRect.new()
	cam_rect.custom_minimum_size = Vector2(320, 240)
	cam_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	cam_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	cv.add_child(cam_rect)

	var chp := PanelContainer.new()
	chp.add_theme_stylebox_override("panel", _style(Color(0.043, 0.055, 0.063, 0.94)))
	chp.position = Vector2(366, 618)
	chp.size = Vector2(670, 272)
	root.add_child(chp)
	var chv := VBoxContainer.new()
	chp.add_child(chv)
	chart_title = _label("", 12, Color("#9aa4ae"), true)
	chv.add_child(chart_title)
	chart = Control.new()
	chart.custom_minimum_size = Vector2(646, 228)
	chart.draw.connect(_draw_chart)
	chv.add_child(chart)


func set_camera_texture(tex: Texture2D) -> void:
	cam_rect.texture = tex


func set_status(s: Dictionary) -> void:
	status = s


func _fmt(x: float, digits: int = 1) -> String:
	return ("%+." + str(digits) + "f") % x


func _process(_delta: float) -> void:
	if lab == null or lab.plant == null:
		return
	if lab.plant.trace_joint != trace_joint:
		lab.plant.trace_joint = trace_joint
		lab.plant.trace = lab.plant.traces.get(trace_joint, [])
	var tel: Dictionary = lab.plant.telemetry(false, false)
	time_label.text = "t_sim = %.3f s" % float(tel["sim_time"])
	var connected: bool = bridge != null and bridge.connected()
	var state := str(status.get("state", "NO_CONTROLLER")) if connected else "NO_CONTROLLER"
	badge.text = state.replace("_", " ")
	badge_panel.add_theme_stylebox_override("panel", _style(STATE_COLORS.get(state, STATE_COLORS["NO_CONTROLLER"]), STATE_COLORS.get(state, STATE_COLORS["NO_CONTROLLER"]).lightened(0.3), 4))
	lab.environment.set_beacon(state)
	var joint_states: Dictionary = status.get("joint_states", {})
	for jn in JOINTS:
		var d: Dictionary = tel["gripper"] if jn == "gripper" else tel["joints"][jn]
		var c: Dictionary = joint_cells[jn]
		c["servo"].text = str(d["servo"])
		var tgt := rad_to_deg(float(d["target"]))
		var pos := rad_to_deg(float(d["link_pos"]))
		c["tgt"].text = _fmt(tgt)
		c["pos"].text = _fmt(pos)
		c["err"].text = _fmt(tgt - pos, 2)
		c["tau"].text = "%+.3f" % float(d["tau_motor"])
		var pct := absf(float(d["tau_request"])) / float(d["tau_max"]) * 100.0
		c["pct"].text = "%3.0f%%" % pct
		var st := "OFF" if not bool(d["powered"]) else ("STALL" if bool(d["stalled"]) else ("SAT" if bool(d["saturated"]) else ("DB" if bool(d["in_deadband"]) else "OK")))
		if joint_states.has(jn):
			st = str(joint_states[jn])
		c["state"].text = st
		var col := Color("#5fd3b3")
		if st == "STALL" or st == "GRIP":
			col = Color("#c58cff")
		elif st == "OFF":
			col = Color("#8a949e")
		elif st == "SAT" or st == "FAULT":
			col = Color("#f2a93b")
		c["state"].add_theme_color_override("font_color", col)
		c["pct"].add_theme_color_override("font_color", Color("#ff8a5c") if pct >= 100.0 else (Color("#f2a93b") if pct >= 85.0 else Color("#e6e9ec")))
	var tcp: Array = tel["tcp"]["position"]
	tcp_label.text = "TCP (true) x=%.1f y=%.1f z=%.1f mm   target %s" % [float(tcp[0]) * 1000.0, float(tcp[1]) * 1000.0, float(tcp[2]) * 1000.0, str(status.get("tcp_target_mm", "—"))]
	var g: Dictionary = tel["gripper"]
	var held := []
	for id in tel["objects"]:
		if bool(tel["objects"][id]["held"]):
			held.append(id)
	var extra := ""
	if tel.has("pen"):
		extra = "   pen %s  strokes %d" % ["DOWN" if tel["pen"]["down"] else "up", int(tel["pen"]["strokes"])]
	gripper_label.text = "gripper opening %.1f mm  F/finger %.2f N  held %s%s" % [float(g["opening"]) * 1000.0, float(g["finger_force_each"]), str(held) if held.size() > 0 else "—", extra]
	var sel: Dictionary = tel["gripper"] if trace_joint == "gripper" else tel["joints"][trace_joint]
	var metrics: Dictionary = status.get("pid_metrics", {})
	pid_label.text = "%s (%s)  Kp=%.3f  Ki=%.3f  Kd=%.4f\nrequested τ=%+.3f  avail=%.3f  max=%.3f N·m  I=%.0f mA\n%s" % [
		trace_joint, str(sel["servo"]), float(sel["kp"]), float(sel["ki"]), float(sel["kd"]),
		float(sel["tau_request"]), float(sel["tau_avail"]), float(sel["tau_max"]), float(sel["current"]) * 1000.0,
		str(metrics.get(trace_joint, "step metrics: run arm.step_response() to measure overshoot / settling")),
	]
	action_label.text = str(status.get("active_action", "idle"))
	var qtext := ""
	for item in status.get("queue", []):
		qtext += str(item) + "\n"
	queue_label.text = qtext if qtext != "" else "(empty)"
	var jtext := ""
	for line in status.get("journal_tail", []):
		jtext += str(line) + "\n"
	journal_label.text = jtext
	chart_title.text = "TRACE %s · last %.1f s · target (amber)  servo shaft (blue)  link (white)  — deg" % [trace_joint, lab.plant.trace_window]
	chart.queue_redraw()


func _draw_chart() -> void:
	var tr: Array = lab.plant.trace
	var w := chart.size.x
	var h := chart.size.y
	chart.draw_rect(Rect2(Vector2.ZERO, chart.size), Color("#0b0e10"))
	if tr.size() < 2:
		chart.draw_string(ThemeDB.fallback_font, Vector2(10, 20), "waiting for physics ticks…", HORIZONTAL_ALIGNMENT_LEFT, -1, 12, Color("#6f7a85"))
		return
	var t0 := float(tr[0][0])
	var t1 := float(tr[tr.size() - 1][0])
	var lo := INF
	var hi := -INF
	for s in tr:
		for k in [1, 2, 3]:
			lo = minf(lo, float(s[k]))
			hi = maxf(hi, float(s[k]))
	var span := maxf(hi - lo, deg_to_rad(2.0))
	var mid := (hi + lo) / 2.0
	lo = mid - span * 0.6
	hi = mid + span * 0.6
	# grid lines every 1 degree when the span is small, otherwise 10
	var step_deg := 1.0 if rad_to_deg(hi - lo) < 15.0 else 10.0
	var gd := ceilf(rad_to_deg(lo) / step_deg) * step_deg
	while gd < rad_to_deg(hi):
		var y := h - (deg_to_rad(gd) - lo) / (hi - lo) * h
		chart.draw_line(Vector2(0, y), Vector2(w, y), Color("#1d2328"), 1.0)
		chart.draw_string(ThemeDB.fallback_font, Vector2(4, y - 2), "%.0f°" % gd, HORIZONTAL_ALIGNMENT_LEFT, -1, 11, Color("#6f7a85"))
		gd += step_deg
	var colors := [Color("#f2a93b"), Color("#4f8fe6"), Color("#e6e9ec")]
	for k in [1, 2, 3]:
		var pts := PackedVector2Array()
		for s in tr:
			var x := (float(s[0]) - t0) / maxf(t1 - t0, 1e-6) * w
			var y := h - (float(s[k]) - lo) / (hi - lo) * h
			pts.append(Vector2(x, y))
		chart.draw_polyline(pts, colors[k - 1], 1.4 if k == 3 else 1.0, true)
