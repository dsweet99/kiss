"""Evaluation wrapper for timing_aggregate_parallel."""

from evals._harness import report_eval, timing_aggregate_parallel


def eval_timing_aggregate_parallel() -> None:
    report_eval(timing_aggregate_parallel)
