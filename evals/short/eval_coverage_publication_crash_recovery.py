"""Evaluation wrapper for coverage_publication_crash_recovery."""

from evals._harness import coverage_publication_crash_recovery, report_eval


def eval_coverage_publication_crash_recovery() -> None:
    report_eval(coverage_publication_crash_recovery)
