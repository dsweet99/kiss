"""CLI tests for adversarial fix command."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest
from click.testing import CliRunner
from python.adversarial_fix_cli import (
    echo_fix_start,
    failed_fix_descriptions,
    fix,
    verify_fix_paths,
    write_fix_prompt,
)


def _stub_fix_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[Path, Path]:
    kiss = tmp_path / "kiss"
    python_dir = kiss / "python"
    python_dir.mkdir(parents=True)
    (python_dir / "coverage_metrics_cli.py").write_text("# stub\n", encoding="utf-8")
    repo = tmp_path / "counterexample"
    repo.mkdir()

    monkeypatch.setattr("python.adversarial_common.repo_root", lambda: kiss)
    monkeypatch.setattr("python.adversarial.run_malvin_code", lambda *_: 0)
    return kiss, repo


def test_write_fix_prompt_uses_multi_repo_path(tmp_path: Path) -> None:
    repos = [tmp_path / "a", tmp_path / "b"]
    prompt_path = write_fix_prompt(
        tmp_path,
        repos,
        build_prompt=lambda root, paths: f"{root.name}:{len(paths)}",
    )

    assert prompt_path == tmp_path / ".adversarial_fix_a_b_prompt.md"
    assert prompt_path.read_text(encoding="utf-8") == f"{tmp_path.name}:2"


def test_echo_fix_start_lists_repos(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    repos = [tmp_path / "a", tmp_path / "b"]
    prompt_path = tmp_path / "prompt.md"

    echo_fix_start(repos, prompt_path)

    output = capsys.readouterr().out
    assert f"fix repos ({len(repos)}):" in output
    assert f"  {repos[0]}" in output
    assert f"prompt: {prompt_path}" in output


def test_verify_fix_paths_aggregates_sections(tmp_path: Path) -> None:
    repos = [tmp_path / "a", tmp_path / "b"]
    metrics = SimpleNamespace(mean_plus_std=0.2, spearman=0.9)

    def verify_one(_kiss: Path, repo: Path) -> tuple[bool, object, str]:
        return repo.name == "a", metrics, f"repo={repo.name}\n"

    result = verify_fix_paths(tmp_path, repos, verify_one=verify_one)

    assert result["passed"] is False
    assert result["results"] == [
        (repos[0], metrics, "repo=a\n"),
        (repos[1], metrics, "repo=b\n"),
    ]
    assert result["output"] == f"=== {repos[0]} ===\nrepo=a\n\n=== {repos[1]} ===\nrepo=b"


def test_failed_fix_descriptions_filters_passing_metrics(tmp_path: Path) -> None:
    failed = SimpleNamespace(mean_plus_std=0.55, spearman=0.3)
    passed = SimpleNamespace(mean_plus_std=0.1, spearman=0.9)
    results = [(tmp_path / "bad", failed, ""), (tmp_path / "good", passed, "")]

    descriptions = failed_fix_descriptions(
        results,
        metrics_passes=lambda metrics: metrics.mean_plus_std < 0.2,
    )

    assert descriptions == [f"{tmp_path / 'bad'} (mean+std=0.55, spearman=0.3)"]


def test_fix_cli_success(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        "python.adversarial.verify_fix",
        lambda *_: (True, SimpleNamespace(), "mean+std(c_f): 0.1000\n"),
    )

    result = CliRunner().invoke(fix, [str(repo)])
    assert result.exit_code == 0
    assert "fix success:" in result.output
    assert Path(f"{repo.resolve()}_fix_prompt.md").is_file()


def test_fix_cli_failure_when_not_passed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    failed_metrics = SimpleNamespace(mean_plus_std=0.55, spearman=0.3)
    monkeypatch.setattr(
        "python.adversarial.verify_fix",
        lambda *_: (
            False,
            failed_metrics,
            "mean+std(c_f): 0.5500\n",
        ),
    )

    result = CliRunner().invoke(fix, [str(repo)])
    assert result.exit_code != 0
    assert "fix conditions not met" in result.output


def test_fix_cli_malvin_nonzero_still_verifies(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr("python.adversarial.run_malvin_code", lambda *_: 1)
    monkeypatch.setattr(
        "python.adversarial.verify_fix",
        lambda *_: (True, SimpleNamespace(), "mean+std(c_f): 0.1000\n"),
    )

    result = CliRunner().invoke(fix, [str(repo)])
    assert result.exit_code == 0
    assert "malvin exited 1" in result.output


def test_fix_cli_requires_at_least_one_repo() -> None:
    result = CliRunner().invoke(fix, [])
    assert result.exit_code != 0
