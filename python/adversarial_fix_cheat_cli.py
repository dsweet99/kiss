from __future__ import annotations

from pathlib import Path

import click

from python.adversarial_common import ensure_import_path, repo_root
from python.adversarial_multi_repo import normalize_repos


@click.command(
    "fix-cheat-verify",
    help="Print fix-cheat verification metrics for REPO(s) (used by malvin loop).",
)
@click.argument(
    "repos",
    nargs=-1,
    required=True,
    type=click.Path(exists=True, file_okay=False, path_type=Path),
)
def fix_cheat_verify(repos: tuple[Path, ...]) -> None:
    ensure_import_path()
    from python.adversarial_verify_batch import verify_fix_cheat_repos

    kiss_root = repo_root()
    paths = normalize_repos(repos)
    passed, _results, output = verify_fix_cheat_repos(kiss_root, paths)
    click.echo(output.rstrip())
    if not passed:
        raise SystemExit(1)


@click.command(
    "fix-cheat",
    help="Run malvin to edit kiss until cheat counterexamples are detected on source only.",
)
@click.argument(
    "repos",
    nargs=-1,
    required=True,
    type=click.Path(exists=True, file_okay=False, path_type=Path),
)
def fix_cheat(repos: tuple[Path, ...]) -> None:
    ensure_import_path()
    from python.adversarial_fix_cheat_session import run_fix_cheat_session

    run_fix_cheat_session(repo_root(), repos)
