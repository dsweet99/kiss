"""Evaluation wrapper for rust_batch_e2e."""

from evals._harness import report_eval, rust_batch_e2e


def eval_rust_batch_e2e() -> None:
    report_eval(rust_batch_e2e)
