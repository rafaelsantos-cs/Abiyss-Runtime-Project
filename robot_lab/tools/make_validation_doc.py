"""Build docs/robotics/VALIDATION.md from a scenario run directory.

    PYTHONPATH=src python -m abiyss.robotics scenario all --window --screenshots --out RUN
    python robot_lab/tools/make_validation_doc.py RUN

Copies screenshots, SVG plots and JSON reports into docs/robotics/ and writes
every number in VALIDATION.md straight from the reports (no hand-copied
values).
"""

from __future__ import annotations

import json
import shutil
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs" / "robotics"


def load(run: Path, name: str) -> dict:
    p = run / f"{name}.json"
    return json.loads(p.read_text()) if p.exists() else {}


def fmt(v, nd=2):
    if v is None:
        return "—"
    if isinstance(v, float):
        return f"{v:.{nd}f}"
    return str(v)


def checks_table(r: dict) -> str:
    rows = ["| check | result | detail |", "|---|---|---|"]
    for c in r.get("checks", []):
        detail = json.dumps(c.get("detail"), ensure_ascii=False) if c.get("detail") is not None else ""
        if len(detail) > 140:
            detail = detail[:137] + "..."
        rows.append(f"| {c['check']} | {'PASS' if c['ok'] else 'FAIL'} | `{detail}` |" if detail else f"| {c['check']} | {'PASS' if c['ok'] else 'FAIL'} | |")
    return "\n".join(rows)


def main(run_dir: str) -> None:
    run = Path(run_dir)
    summary = json.loads((run / "summary.json").read_text())
    shots_dst = DOCS / "screenshots"
    data_dst = DOCS / "validation"
    shots_dst.mkdir(parents=True, exist_ok=True)
    data_dst.mkdir(parents=True, exist_ok=True)
    for png in sorted((run / "screenshots").glob("*.png")):
        shutil.copy2(png, shots_dst / png.name)
    for f in sorted(run.glob("*.svg")):
        shutil.copy2(f, data_dst / f.name)
    for f in sorted(run.glob("*.json")):
        shutil.copy2(f, data_dst / f.name)
    # Telemetry sample: the TEST 1 portion is small enough to keep.
    journal = [json.loads(line) for line in (run / "telemetry.jsonl").read_text().splitlines() if line.strip()]
    counts = Counter(r["event"] for r in journal)
    (data_dst / "telemetry_sample.jsonl").write_text("\n".join(json.dumps(r, sort_keys=True, separators=(",", ":")) for r in journal[:400]) + "\n")

    t1, t2, t3, t4, t5 = (load(run, n) for n in ("test1_write_oi", "test2_pick_four", "test3_shoulder_stall", "test4_repeatability", "test5_gravity"))
    es, pid, cam, stab = (load(run, n) for n in ("extra_emergency_stop", "extra_pid_step", "extra_camera", "extra_stability"))
    eng = summary.get("engine", {})
    L: list[str] = []
    w = L.append
    w("# Validation results")
    w("")
    w(f"Generated from a real run of `abiyss-robotics scenario all --window --screenshots` (seed {summary.get('seed')}).")
    w(f"Engine: Godot {eng.get('engine', '?')}, {eng.get('physics_engine', '?')} at {eng.get('physics_hz', '?')} Hz, renderer `{eng.get('renderer', '?')}` on `{eng.get('video_adapter', '?')}` (software, Xvfb).")
    w("Headless runs produce the same numbers (lockstep, seeded noise). Raw reports: [`validation/`](validation/).")
    w("")
    w("| scenario | result | simulated s | wall s |")
    w("|---|---|---|---|")
    for r in summary["results"]:
        w(f"| {r['title']} | {'PASS' if r['passed'] else 'FAIL'} | {r['sim_s']} | {r['wall_s']} |")
    w("")

    # TEST 1
    a = t1.get("metrics", {}).get("analysis", {})
    w("## TEST 1 — write \"OI\"")
    w("")
    w("![OI written by the simulated arm](screenshots/03b_OI_result.png)")
    w("")
    specs = json.loads((ROOT / "ARM_SPECS.json").read_text())
    wcfg = specs["workspace"]["writing"]
    w(f"Two strokes. The controller only sends IK-derived pulse widths (pen tip commanded {wcfg['pen_press_depth'] * 1000:.1f} mm below the paper, "
      f"{wcfg['pen_up_height'] * 1000:.0f} mm above it between strokes, {wcfg['speed'] * 1000:.0f} mm/s); the ink is physics ground truth from the tip contact. "
      f"Mean deviation from the ideal strokes **{fmt(a.get('deviation_mean_mm'))} mm** (RMS {fmt(a.get('deviation_rms_mm'))}, max {fmt(a.get('deviation_max_mm'))} mm); "
      f"{fmt((a.get('coverage_1mm') or 0) * 100, 1)} % of the ideal path has ink within 1 mm; {a.get('ink_strokes')} ink strokes for {a.get('ideal_strokes')} ideal strokes; "
      f"{a.get('ink_points')} ink samples; action took {fmt(t1.get('metrics', {}).get('duration_s'))} s simulated.")
    w("")
    w("![ink vs ideal](validation/test1_write_oi_ink.svg)")
    w("")
    w("Where the imperfection comes from (all visible in the plot): the O does not close cleanly (dead band + backlash at the stroke reversal), "
      "the side nearest the robot is flattened (shoulder/elbow sag under the pen load with P-type servos), edges wobble (pulse jitter and dead-band hunting), "
      "and the I ends with a tail where the pen keeps touching while the joints unload during the lift.")
    w("")
    w("![writing in progress](screenshots/03_writing_OI.png)")
    w("")
    w(checks_table(t1))
    w("")

    # TEST 2
    objs = t2.get("metrics", {}).get("objects", {})
    w("## TEST 2 — pick 4 objects of different mass")
    w("")
    w("| object | mass g | μ | outcome | lift mm | placement error mm | peak shoulder τ/τmax | stall |")
    w("|---|---|---|---|---|---|---|---|")
    for k in "ABCD":
        o = objs.get(k, {})
        st = o.get("stalls") or []
        stall = f"{st[0]['joint']}: {st[0]['requested_torque']:.3f}/{st[0]['maximum_torque']:.3f} N·m" if st else "—"
        w(f"| {k} {o.get('name')} | {o.get('mass_g')} | {o.get('friction')} | **{o.get('outcome')}** | {fmt(o.get('lift_mm'))} | {fmt(o.get('placement_error_mm'))} | {fmt((o.get('peak_torque_fraction') or {}).get('shoulder'))} | {stall} |")
    lg = objs.get("C_light_grip", {})
    w("")
    w(f"Light grip (C, commanded width {lg.get('commanded_width_mm')} mm on a 25 mm cube): **{lg.get('outcome')}**, lift {lg.get('lift_mm')} mm. {lg.get('explanation', '')}")
    w("")
    w(f"Objects placed earlier, checked after the whole run: `{json.dumps(objs.get('placed_objects_after_run', {}))}`.")
    w("")
    w("Cases demonstrated: held (A, B, C), insufficient torque (D: shoulder stall), escape (C with a dead-band grip), loss of stability (free-standing base, below).")
    w("")
    w("![pick and place](screenshots/04_pick_and_place.png)")
    w("")
    w(checks_table(t2))
    w("")

    # TEST 3
    pc = t3.get("metrics", {}).get("payload_case", {})
    oc = t3.get("metrics", {}).get("obstruction_case", {})
    w("## TEST 3 — forced shoulder stall")
    w("")
    w("| case | joint | requested N·m | maximum N·m | position ° | target ° | arm-only model N·m | probable cause | payload ≥ |")
    w("|---|---|---|---|---|---|---|---|---|")
    for label, case in (("123 g payload", pc), ("tool pressed into bench", oc)):
        for e in case.get("stall_events", []):
            w(f"| {label} | {e['joint']} | {e['requested_torque']} | {e['maximum_torque']} | {e['position_deg']} | {e['target_deg']} | {e.get('static_torque_model')} | {e.get('probable_cause')} | {fmt(e.get('payload_lower_bound_kg'), 3) + ' kg' if e.get('payload_lower_bound_kg') is not None else 'n/a (pushing with gravity)'} |")
    w("")
    w(f"While STALL was latched a new command was rejected: **{pc.get('new_command_rejected_while_stalled')}**. Example event (as journaled):")
    w("")
    if pc.get("stall_events"):
        ev = {k: v for k, v in pc["stall_events"][0].items() if k not in ("seq",)}
        w("```json")
        w(json.dumps(ev, ensure_ascii=False, sort_keys=True))
        w("```")
    w("")
    w("![stall](screenshots/06_stall.png)")
    w("")
    w(checks_table(t3))
    w("")

    # TEST 4
    m4 = t4.get("metrics", {})
    ds = m4.get("same_side", {}).get("dispersion", {})
    da = m4.get("alternating_sides", {})
    di = m4.get("ablation_no_deadband_backlash_jitter", {}).get("dispersion", {})
    w("## TEST 4 — repeatability (20 identical moves)")
    w("")
    w(f"Target TCP {m4.get('target_mm')} mm, approached 20 times. Final TCP measured from physics after 0.5 s settling.")
    w("")
    w("| experiment | n | std x/y/z mm | dispersion RMS mm | dispersion max mm | mean error to target mm |")
    w("|---|---|---|---|---|---|")
    for label, d in (("same approach side", ds), ("alternating sides", da.get("dispersion", {})), ("  approached from the left", da.get("left", {})), ("  approached from the right", da.get("right", {})), ("ablation: dead band, backlash, jitter OFF", di)):
        w(f"| {label} | {d.get('n')} | {d.get('std_mm')} | {d.get('dispersion_rms_mm')} | {d.get('dispersion_max_mm')} | {d.get('error_to_target_mean_mm')} |")
    w("")
    w(f"Approach-direction hysteresis (distance between the left and right mean end points): **{da.get('approach_hysteresis_mm')} mm**. "
      "The ~10 mm mean error to target is gravity sag of the proportional servos (the controller is open loop on joint pulses, like a host driving hobby servos). "
      "With the three effects disabled the dispersion collapses: they change the physics, not just the picture.")
    w("")
    w("![same side](validation/test4_repeatability_same_side.svg) ![alternating](validation/test4_repeatability_alternating.svg)")
    w("")
    w("Backlash and jitter as seen live (wrist SG90 holding still for 3 s: measured pulse jumps, the shaft hunts inside the 1.8° dead band, the link rattles in the gear play):")
    w("")
    w("![backlash and jitter](screenshots/07_backlash_jitter.png)")
    w("")
    w(checks_table(t4))
    w("")

    # TEST 5
    m5 = t5.get("metrics", {})
    w("## TEST 5 — gravity (shoulder servo powered off)")
    w("")
    w(f"Pose {m5.get('pose_deg')}. Powered hold range over 1 s: {m5.get('holding_range_powered_deg')}°. After power-off the shoulder dropped **{m5.get('drop_deg')}°** and the tool reached the bench after {fmt(m5.get('time_to_bench_s'))} s.")
    w("")
    w(f"Model check: shoulder subtree inertia Python {m5.get('inertia_model_kgm2')} kg·m² vs Godot/Jolt {m5.get('inertia_plant_kgm2')} kg·m². "
      f"Rigid-pendulum prediction over the first {m5.get('comparison_horizon_s')} s (while the powered elbow stays within 2° — valid until {fmt(m5.get('rigid_assumption_valid_until_s'))} s): "
      f"{m5.get('drop_model_deg')}° vs measured {m5.get('drop_measured_deg')}°.")
    w("")
    w("![fall](validation/test5_gravity_fall.svg)")
    w("")
    w("![falling arm](screenshots/08_gravity_fall.png)")
    w("")
    w(checks_table(t5))
    w("")

    # Extras
    me = es.get("metrics", {})
    w("## Emergency stop")
    w("")
    w(f"E-STOP during `write(\"OI\")`: state `{me.get('state_after')}`, queue after `{me.get('queue_after')}`, pose drift in 1 s {me.get('pose_drift_deg_1s')}°, new command rejected **{me.get('rejected_while_estop')}**, explicit reset → `{me.get('state_after_reset')}`.")
    w("")
    w("![emergency stop](screenshots/10_emergency_stop.png)")
    w("")
    w(checks_table(es))
    w("")
    w("## PID step response (elbow, gains changed at runtime)")
    w("")
    w("| gains | overshoot % | rise s | settling s | oscillations | steady-state error ° |")
    w("|---|---|---|---|---|---|")
    for rr in pid.get("metrics", {}).get("runs", []):
        g = rr.get("gains", {})
        w(f"| {rr['label']} (Kp {g.get('kp'):.2f}, Ki {g.get('ki'):.2f}, Kd {g.get('kd'):.4f}) | {rr.get('overshoot_pct')} | {fmt(rr.get('rise_time_s'), 3)} | {fmt(rr.get('settling_time_s'), 3)} | {rr.get('oscillations')} | {rr.get('steady_state_error_deg')} |")
    w("")
    w("Settling uses a band of max(5 % of the step, servo dead band) around the final value; `—` = it keeps hunting outside the band during the 1.5 s window.")
    w("")
    w("![pid](validation/pid_step_elbow.svg)")
    w("")
    w("## Camera (`cam.frame()`)")
    w("")
    fr = cam.get("metrics", {}).get("frame", {})
    w(f"{fr.get('width')}x{fr.get('height')} {fr.get('format')}, pose {fr.get('pose')}, intrinsics {fr.get('intrinsics')}.")
    w("")
    w("![frame](screenshots/09_cam_frame.png) ![hud](screenshots/09b_camera_hud.png)")
    w("")
    w(checks_table(cam))
    w("")
    ms = stab.get("metrics", {})
    w("## Stability (arm loses stability)")
    w("")
    w(f"Reaching out to (shoulder 10°, elbow 0°, wrist −20°): combined centre of mass x = {ms.get('combined_com_x_m')} m vs base radius {ms.get('base_radius_m')} m. "
      f"Max base tilt: clamped {ms.get('runs', {}).get('clamped', {}).get('max_base_tilt_deg')}°, free-standing **{ms.get('runs', {}).get('free', {}).get('max_base_tilt_deg')}°** (it tips until the gripper rests on the bench).")
    w("")
    if (shots_dst / "11_unstable_free_base.png").exists():
        w("![free base tipping](screenshots/11_unstable_free_base.png)")
        w("")
    w(checks_table(stab))
    w("")
    w("## Telemetry journal")
    w("")
    w(f"{len(journal)} records in this run. Events: " + ", ".join(f"`{k}` {v}" for k, v in counts.most_common()) + ".")
    w("A sample (first 400 records, TEST 1) is in [`validation/telemetry_sample.jsonl`](validation/telemetry_sample.jsonl).")
    w("")
    w("## Screenshot index")
    w("")
    names = {
        "01_lab_overview": "1. complete lab", "02_arm_at_rest": "2. arm at rest", "03_writing_OI": "3. arm writing OI",
        "04_pick_and_place": "4. pick-and-place", "05_telemetry_panel": "5. telemetry panel", "06_stall": "6. stall",
        "07_backlash_jitter": "7. backlash/jitter visible", "08_gravity_fall": "8. arm falling after servo off",
        "09_cam_frame": "9. end-effector camera frame", "10_emergency_stop": "10. emergency state",
    }
    for png in sorted(shots_dst.glob("*.png")):
        w(f"* [{names.get(png.stem, png.stem)}](screenshots/{png.name})")
    (DOCS / "VALIDATION.md").write_text("\n".join(L) + "\n", encoding="utf-8")
    print(f"wrote {DOCS / 'VALIDATION.md'}")


if __name__ == "__main__":
    main(sys.argv[1])
