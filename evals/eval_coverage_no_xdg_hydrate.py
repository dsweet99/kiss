"""Evaluation wrapper for coverage_no_xdg_hydrate."""

from evals._harness import coverage_no_xdg_hydrate, report_eval


def eval_coverage_no_xdg_hydrate() -> None:
    report_eval(coverage_no_xdg_hydrate)
