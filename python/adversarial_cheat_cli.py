from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

import click

from python.adversarial_common import (
    allocate_adversarial_id,
    ensure_import_path,
    repo_root,
)


def _cleanup_cheat_run(repo_dir: Path, prompt_path: Path) -> None:
    if repo_dir.exists():
        shutil.rmtree(repo_dir)
    if prompt_path.exists():
        prompt_path.unlink()


def _persist_cheat_repo(repo_dir: Path) -> Path:
    dest = allocate_adversarial_id("cheat")
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(repo_dir), str(dest))
    return dest


def _run_cheat_session(
    kiss_root: Path,
    repo_dir: Path,
    prompt_path: Path,
    *,
    lang: str | None,
) -> Path:
    from python.adversarial import pick_language, run_malvin_code
    from python.adversarial_cheat import build_cheat_prompt, verify_cheat

    chosen = pick_language(lang)
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
    return _persist_cheat_repo(repo_dir)


@click.command(
    "cheat-verify",
    help="Print cheat verification metrics for REPO (used by malvin loop).",
)
@click.argument("repo", type=click.Path(exists=True, file_okay=False, path_type=Path))
def cheat_verify(repo: Path) -> None:
    ensure_import_path()
    from python.adversarial_cheat import verify_cheat

    kiss_root = repo_root()
    passed, _metrics, output = verify_cheat(kiss_root, repo.resolve())
    click.echo(output.rstrip())
    if not passed:
        raise SystemExit(1)


@click.command(help="Run malvin to create a repo that passes kiss but not runtime coverage.")
@click.option(
    "--lang",
    type=click.Choice(["rust", "python", "both"]),
    default=None,
    help="Repo language mix (default: random per run).",
)
def cheat(lang: str | None) -> None:
    ensure_import_path()
    kiss_root = repo_root()
    repo_dir = Path(tempfile.mkdtemp(prefix="kiss_cheat_", dir="/tmp"))
    prompt_path = Path(f"{repo_dir}_prompt.md")
    dest: Path | None = None
    try:
        dest = _run_cheat_session(kiss_root, repo_dir, prompt_path, lang=lang)
    except Exception:
        if dest is None:
            _cleanup_cheat_run(repo_dir, prompt_path)
        raise
    finally:
        if prompt_path.exists():
            prompt_path.unlink()
    click.echo(f"cheat success: {dest.resolve()}")
