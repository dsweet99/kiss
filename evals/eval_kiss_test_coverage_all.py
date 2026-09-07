"""Evaluation wrapper for kiss_test_coverage_all."""

from evals._harness import kiss_test_coverage_all, report_eval


def eval_kiss_test_coverage_all() -> None:
    report_eval(kiss_test_coverage_all)
