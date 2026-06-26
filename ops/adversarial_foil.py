"""Foil command for adversarial CLI."""

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


def _cleanup_foil_run(repo_dir: Path, prompt_path: Path) -> None:
    if repo_dir.exists():
        shutil.rmtree(repo_dir)
    if prompt_path.exists():
        prompt_path.unlink()


def _persist_foil_repo(repo_dir: Path) -> Path:
    dest = allocate_adversarial_id("foil")
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(repo_dir), str(dest))
    return dest


def _run_foil_session(
    kiss_root: Path,
    repo_dir: Path,
    prompt_path: Path,
    *,
    lang: str | None,
) -> Path:
    from python.adversarial import (
        build_foil_prompt,
        pick_language,
        run_malvin_code,
        verify_foil,
    )

    chosen = pick_language(lang)
    prompt_path.write_text(
        build_foil_prompt(kiss_root, repo_dir, chosen),
        encoding="utf-8",
    )
    click.echo(f"foil repo: {repo_dir}")
    click.echo(f"language: {chosen}")
    click.echo(f"prompt: {prompt_path}")

    malvin_rc = run_malvin_code(kiss_root, prompt_path)
    if malvin_rc != 0:
        click.echo(
            f"malvin exited {malvin_rc}; verifying coverage metrics anyway",
            err=True,
        )

    violated, metrics, output = verify_foil(kiss_root, repo_dir)
    click.echo(output.rstrip())
    if not violated:
        raise click.ClickException(
            "foil conditions not met after malvin run "
            f"(mean+std={metrics.mean_plus_std}, spearman={metrics.spearman})"
        )
    return _persist_foil_repo(repo_dir)


@click.command()
@click.option(
    "--lang",
    type=click.Choice(["rust", "python", "both"]),
    default=None,
    help="Repo language mix (default: random per run).",
)
def foil(lang: str | None) -> None:
    """Run malvin to create a /tmp repo that violates coverage comfort bands."""
    ensure_import_path()
    kiss_root = repo_root()
    repo_dir = Path(tempfile.mkdtemp(prefix="kiss_foil_", dir="/tmp"))
    prompt_path = Path(f"{repo_dir}_prompt.md")
    dest: Path | None = None
    try:
        dest = _run_foil_session(kiss_root, repo_dir, prompt_path, lang=lang)
    except Exception:
        if dest is None:
            _cleanup_foil_run(repo_dir, prompt_path)
        raise
    finally:
        if prompt_path.exists():
            prompt_path.unlink()
    click.echo(f"foil success: {dest.resolve()}")
