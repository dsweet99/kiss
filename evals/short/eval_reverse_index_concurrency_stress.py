"""Evaluation wrapper for reverse_index_concurrency_stress."""

from evals._harness import report_eval, reverse_index_concurrency_stress


def eval_reverse_index_concurrency_stress() -> None:
    report_eval(reverse_index_concurrency_stress)
