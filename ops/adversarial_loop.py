"""Loop command for adversarial CLI."""

from __future__ import annotations

from pathlib import Path

import click
from python.adversarial_common import ensure_import_path, repo_root


@click.command()
@click.option(
    "--num-iterations",
    type=click.IntRange(min=1),
    default=1,
    show_default=True,
    help="Number of foil → fix cycles to run.",
)
@click.option(
    "--lang",
    type=click.Choice(["rust", "python", "both"]),
    default=None,
    help="Repo language mix for foil (default: random per run).",
)
def loop(num_iterations: int, lang: str | None) -> None:
    """Run one or more foil → fix calibration cycles."""
    ensure_import_path()
    from python.adversarial_loop import AdversarialLoopConfig, run_adversarial_loop

    script = Path(__file__).resolve().parent / "adversarial.py"
    try:
        run_adversarial_loop(
            AdversarialLoopConfig(
                adversarial_script=script,
                num_iterations=num_iterations,
                lang=lang,
                cwd=repo_root(),
            ),
            log_stderr=lambda msg: click.echo(msg, err=True),
        )
    except RuntimeError as exc:
        raise click.ClickException(str(exc)) from exc
