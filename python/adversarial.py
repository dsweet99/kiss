"""Adversarial foil: drive malvin to find coverage metric counterexamples."""

from __future__ import annotations

import random
import re
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple

MEAN_STD_THRESHOLD = 0.5
SPEARMAN_THRESHOLD = 0.6
LANG_CHOICES: tuple[str, ...] = ("rust", "python", "both")

MEAN_STD_RE = re.compile(r"mean\+std\(c_f\):\s*([\d.+-eE]+|nan)")
SPEARMAN_RE = re.compile(
    r"spearman\(coverage_true, coverage_kiss\):\s*([\d.+-eE]+|nan)"
)


class ParsedMetrics(NamedTuple):
    mean_plus_std: float | None
    spearman: float | None


def _parse_float(token: str | None) -> float | None:
    if token is None or token.lower() == "nan":
        return None
    return float(token)


def parse_coverage_metrics_output(text: str) -> ParsedMetrics:
    mean_match = MEAN_STD_RE.search(text)
    spear_match = SPEARMAN_RE.search(text)
    mean_token = mean_match.group(1) if mean_match else None
    spear_token = spear_match.group(1) if spear_match else None
    return ParsedMetrics(_parse_float(mean_token), _parse_float(spear_token))


def foil_violated(metrics: ParsedMetrics) -> bool:
    if metrics.mean_plus_std is not None and metrics.mean_plus_std > MEAN_STD_THRESHOLD:
        return True
    return metrics.spearman is not None and metrics.spearman < SPEARMAN_THRESHOLD


def pick_language(lang: str | None, rng: random.Random | None = None) -> str:
    if lang is not None:
        return lang
    return (rng or random.Random()).choice(LANG_CHOICES)


def build_foil_prompt(kiss_root: Path, repo_dir: Path, lang: str) -> str:
    metrics_py = (kiss_root / "ops" / "coverage_metrics.py").resolve()
    lang_instruction = {
        "rust": "Rust only (include `Cargo.toml`, tests runnable via `cargo llvm-cov nextest`).",
        "python": "Python only (include tests runnable via `pytest` with slipcover).",
        "both": "Both Rust and Python in the same repo.",
    }[lang]
    return f"""# Foil kiss: adversarial coverage counterexample

Create a self-contained repository at:

  {repo_dir.resolve()}

Use this directory exactly; do not relocate the repo.

## Language

{lang_instruction}

## Measurement loop

From the kiss repo root ({kiss_root.resolve()}), repeatedly run:

  {sys.executable} {metrics_py} {repo_dir.resolve()}

Revise the generated repo until **at least one** foil condition holds:

- mean(c_f) + std(c_f) > {MEAN_STD_THRESHOLD}  (printed as `mean+std(c_f)`)
- spearman(coverage_true, coverage_kiss) < {SPEARMAN_THRESHOLD}

Stop when either condition is satisfied. The repo must be measurable: tests must pass and coverage tools must succeed.

Print the final `coverage_metrics` output when done.
"""


def run_malvin_code(kiss_root: Path, prompt_path: Path) -> int:
    cmd = ["malvin", "code", "--tenacious", f"@{prompt_path}"]
    result = subprocess.run(cmd, cwd=kiss_root, check=False)
    return result.returncode


def run_coverage_metrics(kiss_root: Path, repo: Path) -> str:
    script = kiss_root / "ops" / "coverage_metrics.py"
    cmd = [sys.executable, str(script), str(repo.resolve())]
    result = subprocess.run(
        cmd,
        cwd=kiss_root,
        capture_output=True,
        text=True,
        check=False,
    )
    combined = result.stdout
    if result.stderr:
        combined = f"{combined}\n{result.stderr}" if combined else result.stderr
    return combined


def verify_foil(kiss_root: Path, repo: Path) -> tuple[bool, ParsedMetrics, str]:
    output = run_coverage_metrics(kiss_root, repo)
    metrics = parse_coverage_metrics_output(output)
    return foil_violated(metrics), metrics, output
