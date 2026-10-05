# Abiyss Office integration (planned)

```text
ABIYSS OFFICE (Godot 4) ──► LAB / ROBOT ROOM ──► ARM
```

The lab was built so the arm can later appear inside the existing 3D office
without a rewrite:

| Piece | Reusable as |
|---|---|
| `robot_lab/sim/arm_plant.gd` | the physical arm + bench as one `Node3D`; built from ARM_SPECS.json, no dependency on UI or network |
| `robot_lab/sim/servo_model.gd` | pure `RefCounted` model, engine-independent logic |
| `robot_lab/sim/camera_sensor.gd` | a `SubViewport` sensor that follows the gripper |
| `robot_lab/bridge/sim_bridge.gd` | the lockstep TCP bridge; any scene that owns a plant can host it |
| `robot_lab/lab/environment.gd`, `robot_lab/ui/hud.gd` | lab-only presentation; the office would replace them |
| `robot_lab/scenes/lab.tscn` | instanceable scene (`lab.gd` only needs a parent `Node3D`) |

Steps for the office:

1. Copy (or add as a git submodule / Godot addon) `robot_lab/sim` and
   `robot_lab/bridge` into the office project.
2. In the robot room, add a `Node3D` at the bench location and instance the
   plant: `plant.build(specs, "gripper", seed)` (the plant's origin is the
   centre of the base on the bench top; robot +z maps to Godot +y).
3. Add the bridge node with a port; the existing Python Robot API connects to
   it with `SimulationBackend(launch=False, port=...)`.
4. Physics settings: the office project must use Jolt with the overrides in
   `robot_lab/project.godot` (`[physics]` section), otherwise contacts at the
   millimetre scale and the light hinges misbehave (see PHYSICS_MODEL.md).
   Lockstep pauses the *whole* physics server: if the office has other
   physics, run the robot in its own `PhysicsServer3D` space or let the
   office run free (`--free-run`) and use the robot visually only.
5. Renderer: the lab only uses `StandardMaterial3D`, lights and `Label3D`,
   which work in both Compatibility (OpenGL) and Forward+ (Vulkan).

Not done yet: the office repository was not available in this session, so
the integration itself is untested.
