"""Evaluation wrapper for rust_full_repo_observer."""

from evals._harness import report_eval, rust_full_repo_observer


def eval_rust_full_repo_observer() -> None:
    report_eval(rust_full_repo_observer)
