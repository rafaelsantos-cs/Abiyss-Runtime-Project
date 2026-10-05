# Build/validation environment (investigated, not assumed)

Everything below was measured on the VM where the lab was built and
validated (2026-10-05). Commands in parentheses.

## Machine

| Item | Value | How it was checked |
|---|---|---|
| Architecture | x86_64 | `uname -m`, `lscpu` |
| CPU | Intel Xeon @ 2.10 GHz (family 6, model 207), 4 vCPU, KVM guest | `lscpu` |
| RAM | 15 GiB, no swap | `free -h` |
| OS | Ubuntu 24.04.4 LTS, kernel 6.18.44 | `/etc/os-release`, `uname -a` |
| GPU | **none**: `/dev/dri` does not exist | `ls /dev/dri` |
| OpenGL | Mesa 25.2.8 **llvmpipe** (LLVM 20.1.2, 256-bit SIMD), OpenGL 4.5 core, software rasterisation | Godot `RenderingServer.get_video_adapter_*()` under Xvfb |
| Vulkan | loader `libvulkan.so.1` (1.3.275) installed, **no ICD** (`/usr/share/vulkan/icd.d` missing): no Vulkan device | `ls`, `dpkg -l` |
| Display | none (`$DISPLAY` unset); `Xvfb` and `xvfb-run` installed | `which` |
| Network | egress restricted: github.com release downloads reachable; godotengine.org, docs.godotengine.org and the datasheet hosts blocked (HTTP 403 at the proxy) | `curl` |

## Engine choice: Godot 4 kept

Godot 4 was the preferred engine (Abiyss Office is built on it). It was
checked against the needs of this lab instead of being assumed:

| Need | Finding | Verdict |
|---|---|---|
| Rigid-body physics with joints, limits, contacts, friction | Godot 4.7.2 ships **Jolt Physics** built in (`physics/3d/physics_engine="Jolt Physics"`). Hinge and slider joints with limits work; behaviour confirmed by tests (hard stops, gripper travel). | OK |
| Determinism (repeatable experiments) | Same build + same command sequence -> bit-identical trajectories (GDScript test `test_determinism`; headless and windowed runs produced the same numbers to the last digit). | OK |
| Stepping physics under external control (lockstep) | `PhysicsServer3D.set_active(false)` makes `JoltPhysicsServer3D::step()` return immediately (read in the 4.7.2 source) and was verified empirically (body frozen for 300 ticks). | OK |
| Running without a GPU | `--headless` runs physics with a dummy renderer (no images). Rendering works under Xvfb with the **Compatibility** renderer (OpenGL 3.3 path) on llvmpipe. Forward+/Mobile need Vulkan: **not available here**. | OK with Compatibility renderer |
| Camera sensor | `SubViewport` + `Camera3D` rendered on llvmpipe at 320x240; PNG via `Image.save_png_to_buffer()`. Not available in `--headless` (dummy renderer): the API returns an explicit error. | OK (needs a display or Xvfb) |
| Small scale (mm, grams) | Jolt defaults are metre-scale (penetration slop and speculative distance 20 mm, sleeping on, default damping 0.1; read in the 4.7.2 source). Overridden in `project.godot`; documented in ARM_SPECS.json `simulation.engine_overrides`. | OK after overrides |
| ARM/aarch64 | Not tested here (x86_64 VM). Official `Godot_v4.7.2-stable_linux.arm64.zip` exists in the release checksum list; `robot_lab/tools/install_godot.sh` selects it on aarch64. Software rendering performance on ARM is unknown. | Unverified |

No critical limitation was found, so no engine change was made.

## Godot build used

* `Godot_v4.7.2-stable_linux.x86_64.zip` from the official GitHub release.
* SHA-512 verified against the release's `SHA512-SUMS.txt`:
  `9aa00f7a605200940bce3027a567b782f49bd8e940dd06ae9e987bd65aee1b1467edd56ed84fcdcbdd44354bf613bdbb4e5d2913e925850368e150c59ed54c65`.
* Reproduce: `robot_lab/tools/install_godot.sh` (downloads, verifies, installs).

## Measured throughput (this VM)

| Mode | Simulated : wall time |
|---|---|
| headless (physics 1 kHz, lockstep from Python at 50 Hz) | ~3x faster than real time (1 s simulated in ~0.35 s) |
| windowed via Xvfb, llvmpipe, 1600x900, HUD + camera viewport, `--fixed-fps 10` | ~0.4x real time (1 s simulated in ~2.5 s) |
| windowed, `--fixed-fps 5` | ~0.7x real time |

On a machine with a GPU the windowed mode is limited by physics, not
rendering.
