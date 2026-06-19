#!/usr/bin/env python3
"""CLI shim for coverage metrics."""

from python.coverage_metrics import coverage_metrics_cli


def main() -> None:
    coverage_metrics_cli()


def _run_if_main(module_name: str) -> None:
    if module_name == "__main__":
        main()


_run_if_main(__name__)
