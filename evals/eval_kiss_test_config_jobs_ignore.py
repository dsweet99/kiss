"""Evaluation wrapper for kiss_test_config_jobs_ignore."""

from evals._harness import kiss_test_config_jobs_ignore, report_eval


def eval_kiss_test_config_jobs_ignore() -> None:
    report_eval(kiss_test_config_jobs_ignore)
