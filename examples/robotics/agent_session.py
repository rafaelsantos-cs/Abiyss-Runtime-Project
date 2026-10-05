"""Ask the robot questions while it works (simulation backend, headless).

    PYTHONPATH=src python examples/robotics/agent_session.py
"""

import json

from abiyss.robotics import Robot

with Robot.simulation(tool="pen", journal_path="agent_session.jsonl") as robot:
    robot.queue([{"action": "write", "text": "OI"}, {"action": "home"}])
    for _ in range(300):                       # 6 s of simulated time
        robot.controller.tick()
    status = robot.status()
    print("state:", status["state"], "| action:", status["active_action"])
    print("joints (deg):", status["joints_deg"])
    tel = robot.telemetry()
    for name, j in tel["joints"].items():
        print(f"{name:9s} target {j['target_deg']:7.2f}  pos {j['position_deg']:7.2f}  err {j['error_deg']:+6.2f}  "
              f"torque {j['torque_nm']:+.3f}/{j['max_torque_nm']:.3f} N*m  I {j['current_a'] * 1000:5.0f} mA  stalled {j['stalled']}")
    print("stall check:", json.dumps({k: v["load_ratio"] for k, v in robot.arm.stall_check()["joints"].items()}))
    robot.run()
    print("done:", robot.status()["state"])
