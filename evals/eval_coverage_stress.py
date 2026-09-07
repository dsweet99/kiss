"""Evaluation wrapper for coverage_stress."""

from evals._harness import coverage_stress, report_eval


def eval_coverage_stress() -> None:
    report_eval(coverage_stress)
