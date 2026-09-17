"""Evaluation wrapper for timing_rust_throughput."""

from evals._harness import report_eval, timing_rust_throughput


def eval_timing_rust_throughput() -> None:
    report_eval(timing_rust_throughput)
