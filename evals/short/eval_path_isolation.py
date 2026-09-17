"""Evaluation wrapper for path_isolation."""

from evals._harness import path_isolation, report_eval


def eval_path_isolation() -> None:
    report_eval(path_isolation)
