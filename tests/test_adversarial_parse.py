
from __future__ import annotations

import random
from pathlib import Path

import pytest
import python.adversarial as adv
import python.adversarial_loop as adv_loop


def test_parse_coverage_metrics_output_from_fixture() -> None:
    text = (
        "files compared: 2\n"
        "mean(c_f): 0.0500\n"
        "mean+std(c_f): 0.1207\n"
        "spearman(coverage_true, coverage_kiss): 0.8500\n"
    )
    parsed = adv.parse_coverage_metrics_output(text)
    assert parsed.mean_plus_std == pytest.approx(0.1207)
    assert parsed.spearman == pytest.approx(0.8500)


def test_foil_violated_mean_std() -> None:
    assert adv.foil_violated(adv.ParsedMetrics(0.51, 0.9)) is True
    assert adv.foil_violated(adv.ParsedMetrics(0.5, 0.9)) is False


def test_foil_violated_spearman() -> None:
    assert adv.foil_violated(adv.ParsedMetrics(0.1, 0.59)) is True
    assert adv.foil_violated(adv.ParsedMetrics(0.1, 0.6)) is False


def test_foil_violated_missing_metrics() -> None:
    assert adv.foil_violated(adv.ParsedMetrics(None, None)) is False


def test_pick_language_override() -> None:
    assert adv.pick_language("rust", random.Random(0)) == "rust"


def test_pick_language_random() -> None:
    rng = random.Random(42)
    choices = {adv.pick_language(None, rng) for _ in range(30)}
    assert choices <= set(adv.LANG_CHOICES)


def test_build_foil_prompt_contains_paths_and_thresholds(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    repo = tmp_path / "repo"
    kiss.mkdir()
    repo.mkdir()
    text = adv.build_foil_prompt(kiss, repo, "both")
    assert str(repo.resolve()) in text
    assert str(kiss.resolve()) in text
    assert "mean+std(c_f)" in text
    assert "0.5" in text
    assert "0.6" in text


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        ("no success line\n", None),
        (
            "foil repo: /tmp/kiss_foil_abc\nfoil success: /tmp/kiss_foil_abc\n",
            Path("/tmp/kiss_foil_abc"),
        ),
        (
            "foil success: /tmp/first\nnoise\nfoil success: /tmp/second\n",
            Path("/tmp/second"),
        ),
        (
            "echo foil success: /fake\nfoil success: /tmp/real\n",
            Path("/tmp/real"),
        ),
        ("foil success: /tmp/path with spaces\n", Path("/tmp/path with spaces")),
    ],
)
def test_parse_foil_success_path(text: str, expected: Path | None) -> None:
    assert adv_loop.parse_foil_success_path(text) == expected
