
from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial as adv
import python.adversarial_fix_cheat as fix_cheat_mod
import python.adversarial_verify_batch as verify_batch


def test_verify_fix_repos_aggregates(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    repo_a = tmp_path / "a"
    repo_b = tmp_path / "b"
    repo_a.mkdir()
    repo_b.mkdir()
    calls: list[Path] = []

    def fake_verify(_k: Path, repo: Path) -> tuple[bool, adv.ParsedMetrics, str]:
        calls.append(repo)
        ok = repo.name == "a"
        return (
            ok,
            adv.ParsedMetrics(0.1 if ok else 0.9, 0.9),
            f"mean+std(c_f): {0.1 if ok else 0.9:.4f}\n",
        )

    monkeypatch.setattr(verify_batch, "verify_fix", fake_verify)
    passed, results, combined = verify_batch.verify_fix_repos(tmp_path, [repo_a, repo_b])
    assert calls == [repo_a.resolve(), repo_b.resolve()]
    assert passed is False
    assert len(results) == 2
    assert "=== " in combined


def test_verify_fix_cheat_repos_aggregates(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo_a = tmp_path / "a"
    repo_b = tmp_path / "b"
    repo_a.mkdir()
    repo_b.mkdir()

    def fake_verify(_k: Path, repo: Path) -> tuple[bool, fix_cheat_mod.FixCheatMetrics, str]:
        ok = repo.name == "a"
        gaps = () if ok else (("src/a.py", 100.0, 5.0),)
        return ok, fix_cheat_mod.FixCheatMetrics(gaps, (), True), "report\n"

    monkeypatch.setattr(verify_batch, "verify_fix_cheat", fake_verify)
    passed, results, combined = verify_batch.verify_fix_cheat_repos(
        tmp_path, [repo_a, repo_b]
    )
    assert passed is False
    assert len(results) == 2
    assert "=== " in combined
