"""Evaluation wrapper for rust_retained_cache_audit."""

from evals._harness import report_eval, rust_retained_cache_audit


def eval_rust_retained_cache_audit() -> None:
    report_eval(rust_retained_cache_audit)
