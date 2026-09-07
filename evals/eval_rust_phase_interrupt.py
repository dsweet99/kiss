"""Evaluation wrapper for rust_phase_interrupt."""

from evals._harness import report_eval, rust_phase_interrupt


def eval_rust_phase_interrupt() -> None:
    report_eval(rust_phase_interrupt)
