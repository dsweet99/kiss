"""Evaluation wrapper for profraw_discard_sink."""

from evals._harness import profraw_discard_sink, report_eval


def eval_profraw_discard_sink() -> None:
    report_eval(profraw_discard_sink)
