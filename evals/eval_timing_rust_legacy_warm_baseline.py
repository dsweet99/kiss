"""Evaluation wrapper for timing_rust_legacy_warm_baseline."""

from evals._harness import report_eval, timing_rust_legacy_warm_baseline


def eval_timing_rust_legacy_warm_baseline() -> None:
    report_eval(timing_rust_legacy_warm_baseline)
