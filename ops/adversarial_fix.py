"""Fix command for adversarial CLI."""

from __future__ import annotations

from pathlib import Path

import click

from python.adversarial_common import ensure_import_path, repo_root


@click.command()
@click.argument("repo", type=click.Path(exists=True, file_okay=False, path_type=Path))
def fix(repo: Path) -> None:
    """Run malvin to edit kiss until coverage comfort bands pass on REPO."""
    ensure_import_path()
    from python.adversarial import (
        build_fix_prompt,
        run_malvin_code,
        verify_fix,
    )

    kiss_root = repo_root()
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
