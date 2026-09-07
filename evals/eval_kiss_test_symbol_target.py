"""Evaluation wrapper for kiss_test_symbol_target."""

from evals._harness import kiss_test_symbol_target, report_eval


def eval_kiss_test_symbol_target() -> None:
    report_eval(kiss_test_symbol_target)
