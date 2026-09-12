#!/usr/bin/env python3
"""List and run kiss evaluations."""

from __future__ import annotations

import argparse
import importlib
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVALS_DIR = ROOT / "evals"


def _sub_dir_evaluations(sub_dir: Path) -> list[str]:
    if not sub_dir.is_dir() or sub_dir.name.startswith(("_", ".")):
        return []
    return [
        f"{sub_dir.name}/{path.stem[len('eval_') :]}"
        for path in sorted(sub_dir.glob("eval_*.py"))
    ]


def evaluation_names() -> list[str]:
    if not EVALS_DIR.is_dir():
        return []
    names: list[str] = []
    for sub_dir in sorted(EVALS_DIR.iterdir()):
        names.extend(_sub_dir_evaluations(sub_dir))
    return sorted(names)


def _resolve_evaluation_name(name: str, available: list[str]) -> tuple[str, str]:
    if name in available:
        group, eval_name = name.split("/", 1)
        return group, eval_name
    if "/" in name:
        raise SystemExit(f"unknown evaluation: {name}")
    matches = [item for item in available if item.endswith(f"/{name}")]
    if len(matches) == 1:
        group, eval_name = matches[0].split("/", 1)
        return group, eval_name
    if len(matches) > 1:
        joined = ", ".join(matches)
        raise SystemExit(f"ambiguous evaluation: {name} (matches: {joined})")
    raise SystemExit(f"unknown evaluation: {name}")


def run_evaluation(name: str) -> None:
    available = evaluation_names()
    group, eval_name = _resolve_evaluation_name(name, available)
    if str(ROOT) not in sys.path:
        sys.path.insert(0, str(ROOT))
    module = importlib.import_module(f"evals.{group}.eval_{eval_name}")
    getattr(module, f"eval_{eval_name}")()


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
