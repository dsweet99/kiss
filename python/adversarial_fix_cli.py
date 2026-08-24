from __future__ import annotations

from collections.abc import Callable, Sequence
from pathlib import Path

import click

from python.adversarial_common import ensure_import_path, repo_root
from python.adversarial_multi_repo import adversarial_prompt_path, normalize_repos

VerificationResult = dict[str, object]


def write_fix_prompt(
    kiss_root: Path,
    paths: Sequence[Path],
    *,
    build_prompt: Callable[[Path, Sequence[Path]], str],
) -> Path:
    prompt_path = adversarial_prompt_path(kiss_root, paths, "fix")
    prompt_path.write_text(build_prompt(kiss_root, paths), encoding="utf-8")
    return prompt_path


def echo_fix_start(paths: Sequence[Path], prompt_path: Path) -> None:
    click.echo(f"fix repos ({len(paths)}):")
    for repo in paths:
        click.echo(f"  {repo}")
    click.echo(f"prompt: {prompt_path}")


def verify_fix_paths(
    kiss_root: Path,
    paths: Sequence[Path],
    *,
    verify_one: Callable[[Path, Path], tuple[bool, object, str]],
) -> VerificationResult:
    results = []
    sections = []
    passed = True
    for repo in paths:
        repo_passed, metrics, repo_output = verify_one(kiss_root, repo)
        results.append((repo, metrics, repo_output))
        passed = passed and repo_passed
        sections.append(f"=== {repo} ===\n{repo_output.rstrip()}")
    return {"passed": passed, "results": results, "output": "\n\n".join(sections)}


def failed_fix_descriptions(
    results: list[tuple[Path, object, str]],
    *,
    metrics_passes: Callable[[object], bool],
) -> list[str]:
    return [
        f"{repo} (mean+std={metrics.mean_plus_std}, spearman={metrics.spearman})"
        for repo, metrics, _out in results
        if not metrics_passes(metrics)
    ]


@click.command(help="Run malvin to edit kiss until coverage comfort bands pass on REPO(s).")
@click.argument(
    "repos",
    nargs=-1,
    required=True,
    type=click.Path(exists=True, file_okay=False, path_type=Path),
)
def fix(repos: tuple[Path, ...]) -> None:
    ensure_import_path()
    from python.adversarial import (
        build_fix_prompt,
        metrics_pass,
        run_malvin_code,
        verify_fix,
    )

    kiss_root = repo_root()
    paths = normalize_repos(repos)
    prompt_path = write_fix_prompt(kiss_root, paths, build_prompt=build_fix_prompt)
    echo_fix_start(paths, prompt_path)

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(f"malvin exited {malvin_rc}; verifying coverage metrics anyway", err=True)

    verification = verify_fix_paths(kiss_root, paths, verify_one=verify_fix)
    passed = bool(verification["passed"])
    output = str(verification["output"])
    click.echo(output.rstrip())
    if not passed:
        results = verification["results"]
        assert isinstance(results, list)
        failed = failed_fix_descriptions(results, metrics_passes=metrics_pass)
        raise click.ClickException(
            "fix conditions not met after malvin run; failed: " + "; ".join(failed)
        )

    click.echo(f"fix success: {len(paths)} repo(s)")
