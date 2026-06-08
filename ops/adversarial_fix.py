"""Fix command for adversarial CLI."""

from __future__ import annotations

from pathlib import Path

import click

from python.adversarial_common import ensure_import_path, repo_root
from python.adversarial_multi_repo import adversarial_prompt_path, normalize_repos


@click.command()
@click.argument(
    "repos",
    nargs=-1,
    required=True,
    type=click.Path(exists=True, file_okay=False, path_type=Path),
)
def fix(repos: tuple[Path, ...]) -> None:
    """Run malvin to edit kiss until coverage comfort bands pass on REPO(s)."""
    ensure_import_path()
    from python.adversarial import build_fix_prompt, metrics_pass, run_malvin_code
    from python.adversarial_verify_batch import verify_fix_repos

    kiss_root = repo_root()
    paths = normalize_repos(repos)
    prompt_path = adversarial_prompt_path(kiss_root, paths, "fix")
    prompt_path.write_text(
        build_fix_prompt(kiss_root, paths),
        encoding="utf-8",
    )

    click.echo(f"fix repos ({len(paths)}):")
    for repo in paths:
        click.echo(f"  {repo}")
    click.echo(f"prompt: {prompt_path}")

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(f"malvin exited {malvin_rc}; verifying coverage metrics anyway", err=True)

    passed, results, output = verify_fix_repos(kiss_root, paths)
    click.echo(output.rstrip())
    if not passed:
        failed = [
            f"{repo} (mean+std={metrics.mean_plus_std}, spearman={metrics.spearman})"
            for repo, metrics, _out in results
            if not metrics_pass(metrics)
        ]
        raise click.ClickException(
            "fix conditions not met after malvin run; failed: " + "; ".join(failed)
        )

    click.echo(f"fix success: {len(paths)} repo(s)")
