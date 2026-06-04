"""Cheat commands for adversarial CLI."""

from __future__ import annotations

import tempfile
from pathlib import Path

import click

from python.adversarial_common import ensure_import_path, repo_root


@click.command("cheat-verify")
@click.argument("repo", type=click.Path(exists=True, file_okay=False, path_type=Path))
def cheat_verify(repo: Path) -> None:
    """Print cheat verification metrics for REPO (used by malvin loop)."""
    ensure_import_path()
    from python.adversarial_cheat import verify_cheat

    kiss_root = repo_root()
    passed, _metrics, output = verify_cheat(kiss_root, repo.resolve())
    click.echo(output.rstrip())
    if not passed:
        raise SystemExit(1)


@click.command()
@click.option(
    "--lang",
    type=click.Choice(["rust", "python", "both"]),
    default=None,
    help="Repo language mix (default: random per run).",
)
def cheat(lang: str | None) -> None:
    """Run malvin to create a repo that passes kiss but not runtime coverage."""
    ensure_import_path()
    from python.adversarial import pick_language, run_malvin_code
    from python.adversarial_cheat import (
        build_cheat_prompt,
        verify_cheat,
    )

    kiss_root = repo_root()
    repo_dir = Path(tempfile.mkdtemp(prefix="kiss_cheat_", dir="/tmp"))
    chosen = pick_language(lang)
    prompt_path = Path(f"{repo_dir}_prompt.md")
    prompt_path.write_text(
        build_cheat_prompt(kiss_root, repo_dir, chosen),
        encoding="utf-8",
    )

    click.echo(f"cheat repo: {repo_dir}")
    click.echo(f"language: {chosen}")
    click.echo(f"prompt: {prompt_path}")

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(
            f"malvin exited {malvin_rc}; verifying cheat conditions anyway",
            err=True,
        )

    passed, metrics, output = verify_cheat(kiss_root, repo_dir)
    click.echo(output.rstrip())
    if not passed:
        raise click.ClickException(
            "cheat conditions not met after malvin run "
            f"(kiss_passes={metrics.kiss_passes}, gap_count={len(metrics.gaps)})"
        )

    click.echo(f"cheat success: {repo_dir}")
