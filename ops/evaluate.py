#!/usr/bin/env python3
"""List and run kiss evaluations."""

from __future__ import annotations

import argparse
import importlib
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVALS_DIR = ROOT / "evals"


def evaluation_names() -> list[str]:
    names = [path.stem[len("eval_") :] for path in EVALS_DIR.glob("eval_*.py")]
    return sorted(names)


def run_evaluation(name: str) -> None:
    available = evaluation_names()
    if name not in available:
        raise SystemExit(f"unknown evaluation: {name}")
    if str(ROOT) not in sys.path:
        sys.path.insert(0, str(ROOT))
    module = importlib.import_module(f"evals.eval_{name}")
    getattr(module, f"eval_{name}")()


def run_all_evaluations() -> None:
    for name in evaluation_names():
        run_evaluation(name)


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description="Run kiss evaluations")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list", help="List all evaluation NAMEs")
    run_parser = sub.add_parser("run", help="Run an evaluation")
    run_parser.add_argument("NAME")
    sub.add_parser("run-all", help="Run all evaluations")
    args = parser.parse_args(argv)
    if args.command == "list":
        for name in evaluation_names():
            print(name)
        return
    if args.command == "run-all":
        run_all_evaluations()
        return
    run_evaluation(args.NAME)


if __name__ == "__main__":
    main()
