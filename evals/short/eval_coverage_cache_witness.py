"""Evaluation wrapper for coverage_cache_witness."""

from evals._harness import coverage_cache_witness, report_eval


def eval_coverage_cache_witness() -> None:
    report_eval(coverage_cache_witness)
