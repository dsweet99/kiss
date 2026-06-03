#!/usr/bin/env python3
"""Adversarial ops: find kiss coverage counterexamples via malvin."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

import click


def _repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def _ensure_import_path() -> None:
    root = str(_repo_root())
    if root not in sys.path:
        sys.path.insert(0, root)


@click.group()
def main() -> None:
    """Adversarial calibration utilities."""


@main.command()
@click.option(
    "--lang",
    type=click.Choice(["rust", "python", "both"]),
    default=None,
    help="Repo language mix (default: random per run).",
)
def foil(lang: str | None) -> None:
    """Run malvin to create a /tmp repo that violates coverage comfort bands."""
    _ensure_import_path()
    from python.adversarial import (
        build_foil_prompt,
        pick_language,
        run_malvin_code,
        verify_foil,
    )

    kiss_root = _repo_root()
    repo_dir = Path(tempfile.mkdtemp(prefix="kiss_foil_", dir="/tmp"))
    chosen = pick_language(lang)
    prompt_path = Path(f"{repo_dir}_prompt.md")
    prompt_path.write_text(
        build_foil_prompt(kiss_root, repo_dir, chosen),
        encoding="utf-8",
    )

    click.echo(f"foil repo: {repo_dir}")
    click.echo(f"language: {chosen}")
    click.echo(f"prompt: {prompt_path}")

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(f"malvin exited {malvin_rc}; verifying coverage metrics anyway", err=True)

    violated, metrics, output = verify_foil(kiss_root, repo_dir)
    click.echo(output.rstrip())
    if not violated:
        raise click.ClickException(
            "foil conditions not met after malvin run "
            f"(mean+std={metrics.mean_plus_std}, spearman={metrics.spearman})"
        )

    click.echo(f"foil success: {repo_dir}")


@main.command()
@click.argument("repo", type=click.Path(exists=True, file_okay=False, path_type=Path))
def fix(repo: Path) -> None:
    """Run malvin to edit kiss until coverage comfort bands pass on REPO."""
    _ensure_import_path()
    from python.adversarial import (
        build_fix_prompt,
        run_malvin_code,
        verify_fix,
    )

    kiss_root = _repo_root()
    repo = repo.resolve()
    prompt_path = Path(f"{repo}_fix_prompt.md")
    prompt_path.write_text(
        build_fix_prompt(kiss_root, repo),
        encoding="utf-8",
    )

    click.echo(f"fix repo: {repo}")
    click.echo(f"prompt: {prompt_path}")

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(f"malvin exited {malvin_rc}; verifying coverage metrics anyway", err=True)

    passed, metrics, output = verify_fix(kiss_root, repo)
    click.echo(output.rstrip())
    if not passed:
        raise click.ClickException(
            "fix conditions not met after malvin run "
            f"(mean+std={metrics.mean_plus_std}, spearman={metrics.spearman})"
        )

    click.echo(f"fix success: {repo}")


@main.command()
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
    _ensure_import_path()
    from python.adversarial_loop import AdversarialLoopConfig, run_adversarial_loop

    script = Path(__file__).resolve()
    try:
        run_adversarial_loop(
            AdversarialLoopConfig(
                adversarial_script=script,
                num_iterations=num_iterations,
                lang=lang,
                cwd=_repo_root(),
            ),
            log_stderr=lambda msg: click.echo(msg, err=True),
        )
    except RuntimeError as exc:
        raise click.ClickException(str(exc)) from exc


if __name__ == "__main__":
    main()
