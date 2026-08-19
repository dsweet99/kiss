
from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path

import click

from python.adversarial import run_malvin_code
from python.adversarial_fix_cheat import build_fix_cheat_prompt, fix_cheat_satisfied
from python.adversarial_multi_repo import adversarial_prompt_path, normalize_repos
from python.adversarial_verify_batch import verify_fix_cheat_repos


def run_fix_cheat_session(kiss_root: Path, repos: Sequence[Path]) -> None:
    paths = normalize_repos(repos)
    prompt_path = adversarial_prompt_path(kiss_root, paths, "fix_cheat")
    prompt_path.write_text(
        build_fix_cheat_prompt(kiss_root, paths),
        encoding="utf-8",
    )

    click.echo(f"fix-cheat repos ({len(paths)}):")
    for repo in paths:
        click.echo(f"  {repo}")
    click.echo(f"prompt: {prompt_path}")

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(
            f"malvin exited {malvin_rc}; verifying cheat metrics anyway",
            err=True,
        )

    passed, results, output = verify_fix_cheat_repos(kiss_root, paths)
    click.echo(output.rstrip())
    if not passed:
        failed = [
            f"{repo} (gaps={len(metrics.gaps)}, flagged_tests={len(metrics.flagged_tests)})"
            for repo, metrics, _out in results
            if not fix_cheat_satisfied(metrics)
        ]
        raise click.ClickException(
            "fix-cheat conditions not met after malvin run; failed: " + "; ".join(failed)
        )

    click.echo(f"fix-cheat success: {len(paths)} repo(s)")
