"""Evaluation wrapper for record_cache_witness."""

from evals._harness import record_cache_witness, report_eval


def eval_record_cache_witness() -> None:
    report_eval(record_cache_witness)
