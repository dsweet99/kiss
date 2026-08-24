from __future__ import annotations

import click

from python.adversarial_common import ensure_import_path


@click.command(help="Run coverage comparison on every calibration manifest repo.")
def metrics() -> None:
    ensure_import_path()
    from python.adversarial_metrics_batch import run_metrics_batch

    exit_code = run_metrics_batch()
    if exit_code != 0:
        raise SystemExit(exit_code)
