"""Evaluation wrapper for aggregate_coverage."""

from evals._harness import aggregate_coverage, report_eval


def eval_aggregate_coverage() -> None:
    report_eval(aggregate_coverage)
