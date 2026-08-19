"""Tests for adversarial fix-cheat command and verification."""

from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial_fix_cheat as fix_cheat_mod


@pytest.mark.parametrize(
    ("kiss_partial", "expected"),
    [
        ({}, []),
        ({"src/a.py": 50.0}, []),
        ({"tests/test_a.py": 80.0}, ["tests/test_a.py"]),
        ({"src/a.py": 100.0, "tests/t.py": 10.0}, ["tests/t.py"]),
    ],
)
def test_cheat_test_paths_flagged(
    kiss_partial: dict[str, float], expected: list[str]
) -> None:
    assert fix_cheat_mod.cheat_test_paths_flagged(kiss_partial) == expected


@pytest.mark.parametrize(
    ("metrics", "expected"),
    [
        (fix_cheat_mod.FixCheatMetrics((), (), True), True),
        (fix_cheat_mod.FixCheatMetrics((("src/a.py", 100.0, 5.0),), (), True), False),
        (fix_cheat_mod.FixCheatMetrics((), ("tests/t.py",), True), False),
        (fix_cheat_mod.FixCheatMetrics((), (), False), True),
    ],
)
def test_fix_cheat_satisfied(
    metrics: fix_cheat_mod.FixCheatMetrics, expected: bool
) -> None:
    assert fix_cheat_mod.fix_cheat_satisfied(metrics) is expected


def test_build_fix_cheat_prompt_contains_paths_and_thresholds(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    repo = tmp_path / "repo"
    kiss.mkdir()
    (kiss / "python").mkdir()
    (kiss / "python" / "adversarial_cli.py").write_text("# stub\n", encoding="utf-8")
    repo.mkdir()
    text = fix_cheat_mod.build_fix_cheat_prompt(kiss, [repo])
    assert str(repo.resolve()) in text
    assert str(kiss.resolve()) in text
    assert "Do **not** modify the cheat counterexample repository" in text
    assert "fix-cheat-verify" in text
    assert "test modules flagged by kiss" in text
    assert "below 80%" in text.lower() or "below\n  80" in text or "80%" in text


def test_format_fix_cheat_report_lists_flagged_tests() -> None:
    metrics = fix_cheat_mod.FixCheatMetrics(
        (("src/a.py", 100.0, 5.0),),
        ("tests/t.py",),
        True,
    )
    text = fix_cheat_mod.format_fix_cheat_report(metrics)
    assert "cheat gap count: 1" in text
    assert "tests/t.py" in text


def test_build_fix_cheat_prompt_multiple_repos(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    repo_a = tmp_path / "a"
    repo_b = tmp_path / "b"
    kiss.mkdir()
    (kiss / "python").mkdir()
    (kiss / "python" / "adversarial_cli.py").write_text("# stub\n", encoding="utf-8")
    repo_a.mkdir()
    repo_b.mkdir()
    text = fix_cheat_mod.build_fix_cheat_prompt(kiss, [repo_a, repo_b])
    assert str(repo_a.resolve()) in text
    assert str(repo_b.resolve()) in text
    assert "every repo" in text


def test_verify_fix_cheat(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    monkeypatch.setattr(fix_cheat_mod, "run_kiss_check", lambda _r: (0, "kiss ok\n"))
    monkeypatch.setattr(
        fix_cheat_mod,
        "_load_coverage_maps",
        lambda _r: ({"src/a.py": 40.0}, {"src/a.py": 5.0}),
    )
    ok, metrics, combined = fix_cheat_mod.verify_fix_cheat(tmp_path, repo)
    assert ok is True
    assert metrics.gaps == ()
    assert "cheat gap count: 0" in combined

