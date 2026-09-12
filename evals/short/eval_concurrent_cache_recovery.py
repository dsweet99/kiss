"""Evaluation wrapper for concurrent_cache_recovery."""

from evals._harness import concurrent_cache_recovery, report_eval


def eval_concurrent_cache_recovery() -> None:
    report_eval(concurrent_cache_recovery)
