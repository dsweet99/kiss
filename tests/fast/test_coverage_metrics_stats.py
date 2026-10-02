
from __future__ import annotations

import math
import random
from pathlib import Path

import python.coverage_stats as stats


def test_normalize_path_relative_under_repo(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "pkg").mkdir()
    rel = stats.normalize_path("pkg/mod.py", repo)
    assert rel == "pkg/mod.py"


def test_normalize_path_absolute_inside_repo(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    target = repo / "a.py"
    target.touch()
    rel = stats.normalize_path(target, repo)
    assert rel == "a.py"


def test_normalize_path_outside_repo_returns_absolute_str(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    outside = tmp_path / "elsewhere.py"
    outside.touch()
    result = stats.normalize_path(outside, repo)
    assert result == str(outside.resolve())


def test_percentile_empty_is_nan() -> None:
    assert math.isnan(stats.percentile([], 50))


def test_percentile_single_value() -> None:
    assert stats.percentile([7.0], 0) == 7.0
    assert stats.percentile([7.0], 100) == 7.0


def test_percentile_interpolates() -> None:
    assert stats.percentile([0.0, 10.0], 50) == 5.0


def test_percentile_metamorphic_invariant_under_shuffle() -> None:
    seed = 424242
    rng = random.Random(seed)
    print(f"percentile shuffle metamorphic seed={seed}")
    values = [rng.uniform(0, 100) for _ in range(20)]
    for pct in (0, 25, 50, 75, 99, 100):
        base = stats.percentile(values, pct)
        shuffled = values[:]
        rng.shuffle(shuffled)
        assert stats.percentile(shuffled, pct) == base
