"""Evaluation wrapper for kiss_test_base."""

from evals._harness import kiss_test_base, report_eval


def eval_kiss_test_base() -> None:
    report_eval(kiss_test_base)
