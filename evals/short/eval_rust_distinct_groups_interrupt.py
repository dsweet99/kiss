"""Evaluation wrapper for rust_distinct_groups_interrupt."""

from evals._harness import report_eval, rust_distinct_groups_interrupt


def eval_rust_distinct_groups_interrupt() -> None:
    report_eval(rust_distinct_groups_interrupt)
