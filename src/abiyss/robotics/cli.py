"""``abiyss-robotics`` / ``python -m abiyss.robotics`` command line.

    abiyss-robotics lab --queue 'write("OI")'      # 3D lab window + controller
    abiyss-robotics run examples/robotics/write_oi.queue --headless
    abiyss-robotics scenario all --out artifacts/robotics
    abiyss-robotics ctl status | telemetry | stall | estop | reset | frame /abs.png
    abiyss-robotics ctl enqueue 'write("OI")'
    abiyss-robotics specs --audit
"""

from __future__ import annotations

import argparse
import json
import signal
import sys
import time
from pathlib import Path
from typing import Any

from .api import Robot
from .queue_dsl import load_queue
from .server import DEFAULT_API_PORT, RobotAPIServer, RobotClient
from .specs import audit_parameters, load_specs


def _print(obj: Any) -> None:
    print(json.dumps(obj, indent=2, ensure_ascii=False, default=str))


def cmd_lab(args: argparse.Namespace) -> int:
    robot = Robot.simulation(tool=args.tool, seed=args.seed, headless=args.headless, journal_path=args.journal, godot_log=args.godot_log, render_fps=args.fps, view=args.view)
    ctl = robot.controller
    server = RobotAPIServer(ctl, args.api_port).start()
    print(f"[lab] controller running; Robot API on 127.0.0.1:{args.api_port}; journal: {args.journal}")
    ctl.ui_handlers["write_oi"] = lambda: _ui_write(robot)
    ctl.ui_handlers["pick_test"] = lambda: _ui_pick(robot)
    ctl.ui_handlers["home"] = lambda: ctl.submit({"action": "home"}, source="ui")
    if args.queue:
        actions = load_queue(args.queue)
        if any(a["action"] == "write" for a in actions) and ctl.tool != "pen":
            ctl.reset_world("pen")
        robot.queue(actions, source="cli")
    stop = {"flag": False}
    signal.signal(signal.SIGINT, lambda *_: stop.__setitem__("flag", True))
    signal.signal(signal.SIGTERM, lambda *_: stop.__setitem__("flag", True))
    wall0, sim0 = time.monotonic(), ctl.sim_time
    try:
        while not stop["flag"]:
            ctl.tick()
            if args.realtime:
                ahead = (ctl.sim_time - sim0) - (time.monotonic() - wall0)
                if ahead > 0:
                    time.sleep(ahead)
            if args.exit_when_idle and ctl.queue.idle() and ctl._program is None and ctl.sim_time - sim0 > 1.0:
                break
            if args.duration and ctl.sim_time - sim0 >= args.duration:
                break
    finally:
        server.stop()
        _print(ctl.status())
        robot.close()
    return 0


def _ui_write(robot: Robot) -> None:
    ctl = robot.controller
    if ctl.tool != "pen":
        ctl.reset_world("pen")
    ctl.submit({"action": "write", "text": "OI"}, source="ui")
    ctl.submit({"action": "home"}, source="ui")


def _ui_pick(robot: Robot) -> None:
    ctl = robot.controller
    if ctl.tool != "gripper":
        ctl.reset_world("gripper")
    objs = {o["id"]: o for o in ctl.specs.workspace["objects"]}
    slots = ctl.specs.workspace["drop_zone"]["slots"]
    for i, k in enumerate(["A", "B", "C"]):
        x, y, z = objs[k]["position"]
        ctl.submit({"action": "pick", "x": x, "y": y, "z": z, "label": k}, source="ui")
        ctl.submit({"action": "place", "x": slots[i][0], "y": slots[i][1], "z": z}, source="ui")
    ctl.submit({"action": "home"}, source="ui")


def cmd_run(args: argparse.Namespace) -> int:
    actions = load_queue(args.queue)
    tool = args.tool or ("pen" if any(a["action"] == "write" for a in actions) else "gripper")
    with Robot.simulation(tool=tool, seed=args.seed, headless=args.headless, journal_path=args.journal, godot_log=args.godot_log) as robot:
        recs = robot.queue(actions, source="cli")
        ok = robot.run(args.timeout)
        out = {"completed": ok, "status": robot.status(), "actions": []}
        for rec in recs:
            res = {k: v for k, v in rec.result.items() if k not in ("ideal", "ink")}
            out["actions"].append({"id": rec.id, "action": rec.action, "status": rec.status.value, "error": rec.error, "result": res})
        _print(out)
        return 0 if ok and all(r.status.value == "done" for r in recs) else 1


def cmd_scenario(args: argparse.Namespace) -> int:
    from .scenarios import SCENARIOS, run_scenarios

    names = list(SCENARIOS) if args.name == "all" else [args.name]
    if args.name == "all" and args.headless:
        names = [n for n in names if n != "camera"]
    summary = run_scenarios(names, args.out, headless=args.headless, screenshots=args.screenshots, seed=args.seed)
    _print(summary)
    return 0 if summary["all_passed"] else 1


def cmd_ctl(args: argparse.Namespace) -> int:
    c = RobotClient(args.api_port)
    if args.what == "status":
        _print(c.status())
    elif args.what == "telemetry":
        _print(c.telemetry())
    elif args.what == "stall":
        _print(c.stall_check())
    elif args.what == "estop":
        _print(c.emergency_stop("ctl"))
    elif args.what == "reset":
        _print(c.reset())
    elif args.what == "stop":
        _print(c.stop())
    elif args.what == "frame":
        if not args.arg:
            print("usage: ctl frame /absolute/path.png", file=sys.stderr)
            return 2
        _print(c.frame(str(Path(args.arg).resolve())))
    elif args.what == "enqueue":
        _print(c.enqueue(load_queue(args.arg or ""), source="ctl"))
    return 0


def cmd_specs(args: argparse.Namespace) -> int:
    specs = load_specs(args.specs)
    rows = audit_parameters(specs.raw)
    counts: dict[str, int] = {}
    for r in rows:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    if args.audit:
        for r in rows:
            print(f"{r['status']:<30} {r['path']:<70} {r['value']!r:<24} {r['unit'] or ''}  {','.join(r['sources'])}")
    _print({"file": str(specs.path), "parameters": len(rows), "by_status": counts})
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="abiyss-robotics", description="Abiyss robotics lab: Robot API, simulator and validation")
    sub = p.add_subparsers(dest="cmd", required=True)

    lab = sub.add_parser("lab", help="open the 3D lab and run the controller (real-time paced)")
    lab.add_argument("--queue", help="queue file or inline text, e.g. 'write(\"OI\")'")
    lab.add_argument("--tool", choices=["gripper", "pen"], default="gripper")
    lab.add_argument("--headless", action="store_true")
    lab.add_argument("--seed", type=int)
    lab.add_argument("--fps", type=int, help="render frames per simulated second (default 10)")
    lab.add_argument("--view", default="overview")
    lab.add_argument("--api-port", type=int, default=DEFAULT_API_PORT)
    lab.add_argument("--journal", default="robotics_journal.jsonl")
    lab.add_argument("--godot-log")
    lab.add_argument("--no-realtime", dest="realtime", action="store_false")
    lab.add_argument("--exit-when-idle", action="store_true")
    lab.add_argument("--duration", type=float, help="stop after this many simulated seconds")
    lab.set_defaults(fn=cmd_lab)

    run = sub.add_parser("run", help="run a queue through the simulator and print the results")
    run.add_argument("queue")
    run.add_argument("--tool", choices=["gripper", "pen"])
    run.add_argument("--headless", action="store_true", default=True)
    run.add_argument("--window", dest="headless", action="store_false")
    run.add_argument("--seed", type=int)
    run.add_argument("--timeout", type=float, default=600.0)
    run.add_argument("--journal")
    run.add_argument("--godot-log")
    run.set_defaults(fn=cmd_run)

    sc = sub.add_parser("scenario", help="run validation scenarios (TEST 1-5 and extras)")
    sc.add_argument("name", help="scenario name or 'all'")
    sc.add_argument("--out", default="artifacts/robotics")
    sc.add_argument("--headless", action="store_true", default=True)
    sc.add_argument("--window", dest="headless", action="store_false")
    sc.add_argument("--screenshots", action="store_true")
    sc.add_argument("--seed", type=int)
    sc.set_defaults(fn=cmd_scenario)

    ctl = sub.add_parser("ctl", help="talk to a running lab through the Robot API service")
    ctl.add_argument("what", choices=["status", "telemetry", "stall", "estop", "reset", "stop", "frame", "enqueue"])
    ctl.add_argument("arg", nargs="?")
    ctl.add_argument("--api-port", type=int, default=DEFAULT_API_PORT)
    ctl.set_defaults(fn=cmd_ctl)

    sp = sub.add_parser("specs", help="summarise ARM_SPECS.json verification statuses")
    sp.add_argument("--specs")
    sp.add_argument("--audit", action="store_true")
    sp.set_defaults(fn=cmd_specs)
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return int(args.fn(args))


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
