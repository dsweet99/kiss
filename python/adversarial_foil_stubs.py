
from __future__ import annotations

from pathlib import Path

import pytest

import python.adversarial as adv
import python.adversarial_common as cli
import python.adversarial_foil_cli as foil_mod


def stub_foil_env(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Path]:
    kiss = tmp_path / "kiss"
    python_dir = kiss / "python"
    python_dir.mkdir(parents=True)
    (python_dir / "coverage_metrics_cli.py").write_text("# stub\n", encoding="utf-8")
    adv_root = tmp_path / "kiss-adversarial"
    monkeypatch.setattr(cli, "repo_root", lambda: kiss)
    monkeypatch.setattr(cli, "adversarial_root", lambda: adv_root)

    def fake_mkdtemp(*, prefix: str, dir: str) -> str:
        repo = tmp_path / "foil_repo"
        repo.mkdir(exist_ok=True)
        return str(repo)

    monkeypatch.setattr(foil_mod.tempfile, "mkdtemp", fake_mkdtemp)
    monkeypatch.setattr(foil_mod, "repo_root", lambda: kiss)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 0)
    return kiss, adv_root
