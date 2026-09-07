"""Evaluation wrapper for kiss_test_main."""

from evals._harness import kiss_test_main, report_eval


def eval_kiss_test_main() -> None:
    report_eval(kiss_test_main)
