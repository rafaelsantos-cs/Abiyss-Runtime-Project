extends RefCounted
## ARM_SPECS.json access and robot <-> Godot frame conversion.
##
## Robot frame: +x forward, +y left, +z up (right-handed).
## Godot frame: Y up. robot (x, y, z) -> godot (x, z, -y), a proper rotation.

const DEFAULT_RELATIVE_PATH := "../ARM_SPECS.json"


static func default_path() -> String:
	return ProjectSettings.globalize_path("res://").path_join(DEFAULT_RELATIVE_PATH).simplify_path()


static func load_specs(path: String = "") -> Dictionary:
	if path == "":
		path = default_path()
	var text := FileAccess.get_file_as_string(path)
	if text == "":
		push_error("cannot read specs: %s" % path)
		return {}
	var parsed: Variant = JSON.parse_string(text)
	if not (parsed is Dictionary):
		push_error("invalid specs JSON: %s" % path)
		return {}
	return parsed


static func v(d: Dictionary, key: String, fallback: Variant = null) -> Variant:
	if not d.has(key):
		return fallback
	var item: Variant = d[key]
	if item is Dictionary and (item as Dictionary).has("value"):
		return item["value"]
	return item


static func r2g(p: Variant) -> Vector3:
	if p is Vector3:
		return Vector3(p.x, p.z, -p.y)
	return Vector3(float(p[0]), float(p[2]), -float(p[1]))


static func g2r(p: Vector3) -> Vector3:
	return Vector3(p.x, -p.z, p.y)


## Box size given in robot axes -> Godot axes (magnitudes).
static func r2g_size(s: Variant) -> Vector3:
	return Vector3(float(s[0]), float(s[2]), float(s[1]))


static func vec_to_array(p: Vector3) -> Array:
	return [snappedf(p.x, 1e-7), snappedf(p.y, 1e-7), snappedf(p.z, 1e-7)]


## Robot-frame rotation basis (columns = robot axes of a frame) -> Godot basis.
static func basis_r2g(b_robot: Basis) -> Basis:
	var c := Basis(Vector3(1, 0, 0), Vector3(0, 0, -1), Vector3(0, 1, 0))
	return c * b_robot * c.transposed()


static func basis_g2r(b_godot: Basis) -> Basis:
	var c := Basis(Vector3(1, 0, 0), Vector3(0, 0, -1), Vector3(0, 1, 0))
	return c.transposed() * b_godot * c
