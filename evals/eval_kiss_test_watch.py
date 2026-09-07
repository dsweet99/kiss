"""Evaluation wrapper for kiss_test_watch."""

from evals._harness import kiss_test_watch, report_eval


def eval_kiss_test_watch() -> None:
    report_eval(kiss_test_watch)
