from __future__ import annotations

import argparse
import json
import os
import platform
import sys
from pathlib import Path

from . import __version__
from .config import Config
from .errors import AbiyssError
from .gemini import GoogleGeminiProvider
from .runtime import AbiyssRuntime


def _doctor() -> dict:
    return {
        "version": __version__,
        "python": platform.python_version(),
        "platform": platform.platform(),
        "linux": sys.platform.startswith("linux"),
        "euid": os.geteuid() if hasattr(os, "geteuid") else None,
        "gemini_key_configured": bool(os.getenv("GEMINI_API_KEY")),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="abiyss")
    parser.add_argument("--config", type=Path, default=None)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("doctor")
    sub.add_parser("status")
    prompt = sub.add_parser("prompt")
    prompt.add_argument("text")
    recover = sub.add_parser("recover")
    recover.add_argument("query_id")
    recover.add_argument("action", choices=("cancel", "requeue"))
    args = parser.parse_args(argv)

    if args.command == "doctor":
        print(json.dumps(_doctor(), indent=2))
        return 0

    config = Config.load(args.config)
    runtime = AbiyssRuntime(
        config.root,
        allow_exec=config.allow_exec,
        allow_root_exec=config.allow_root_exec,
        command_allowlist=config.command_allowlist,
    )
    try:
        if args.command == "status":
            queries = runtime.qq.all_queries()
            output = {
                "version": __version__,
                "root": str(config.root),
                "active": runtime.qq.active_query_id,
                "queries": len(queries),
                "queued": sum(item.state.value == "queued" for item in queries),
                "running": sum(item.state.value == "running" for item in queries),
                "recovery": sum(item.state.value == "recovery_required" for item in queries),
            }
            print(json.dumps(output, indent=2))
            return 0
        if args.command == "recover":
            runtime.qq.resolve_recovery(args.query_id, args.action)
            return 0
        if args.command == "prompt":
            runtime.provider = GoogleGeminiProvider(model=config.model)
            turn = runtime.execute_agent_cycle(args.text, max_rounds=config.max_rounds)
            print(turn.output_text)
            return 0
    except AbiyssError as exc:
        print(f"abiyss: {exc}", file=sys.stderr)
        return 2
    finally:
        runtime.shutdown()
    return 1
