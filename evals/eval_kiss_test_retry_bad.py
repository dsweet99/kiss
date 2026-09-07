"""Evaluation wrapper for kiss_test_retry_bad."""

from evals._harness import kiss_test_retry_bad, report_eval


def eval_kiss_test_retry_bad() -> None:
    report_eval(kiss_test_retry_bad)
