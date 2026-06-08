"""Adversarial foil: drive malvin to find coverage metric counterexamples."""

from __future__ import annotations

import random
import re
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import NamedTuple

from python.adversarial_multi_repo import format_repo_paths, normalize_repos

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


def parse_coverage_metrics_output(text: str) -> ParsedMetrics:
    mean_match = MEAN_STD_RE.search(text)
    spear_match = SPEARMAN_RE.search(text)
    mean_token = mean_match.group(1) if mean_match else None
    spear_token = spear_match.group(1) if spear_match else None
    mean_val = (
        None
        if mean_token is None or mean_token.lower() == "nan"
        else float(mean_token)
    )
    spear_val = (
        None
        if spear_token is None or spear_token.lower() == "nan"
        else float(spear_token)
    )
    return ParsedMetrics(mean_val, spear_val)


def foil_violated(metrics: ParsedMetrics) -> bool:
    if metrics.mean_plus_std is not None and metrics.mean_plus_std > MEAN_STD_THRESHOLD:
        return True
    return metrics.spearman is not None and metrics.spearman < SPEARMAN_THRESHOLD


def metrics_pass(metrics: ParsedMetrics) -> bool:
    if metrics.mean_plus_std is None or metrics.spearman is None:
        return False
    if metrics.mean_plus_std > MEAN_STD_THRESHOLD:
        return False
    return metrics.spearman >= SPEARMAN_THRESHOLD


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

Stop when either condition is satisfied. The repo must be measurable: tests must pass and coverage tools
 must succeed. Be especially sensitive to signs of overfitting in kiss code, and aim to foil them.

Also, make your repo test something different from the other repos (if there are any): {repo_dir.resolve()}/../*

Print the final `coverage_metrics` output when done.
"""


def build_fix_prompt(kiss_root: Path, repos: Sequence[Path]) -> str:
    metrics_py = (kiss_root / "ops" / "coverage_metrics.py").resolve()
    paths = normalize_repos(repos)
    repo_block = format_repo_paths(paths)
    one = len(paths) == 1
    counter_label = "counterexample repository" if one else "counterexample repositories"
    use_label = "that repo" if one else "those repos"
    measure_cmds = "\n".join(
        f"  {sys.executable} {metrics_py} {p}" for p in paths
    )
    pass_scope = "the repo" if one else "every repo"
    return f"""# Fix kiss: improve coverage estimation alignment

Edit the kiss codebase at:

  {kiss_root.resolve()}

Do **not** modify the {counter_label} at:

{repo_block}

Use {use_label} only for measurement.

## Measurement loop

From the kiss repo root, repeatedly run coverage metrics on each repo:

{measure_cmds}

Revise kiss until **both** pass conditions hold on {pass_scope}:

- mean(c_f) + std(c_f) <= {MEAN_STD_THRESHOLD}  (printed as `mean+std(c_f)`)
- spearman(coverage_true, coverage_kiss) >= {SPEARMAN_THRESHOLD}

Stop when both conditions are satisfied on {pass_scope}. Each counterexample repo must remain
measurable: its tests must pass and coverage tools must succeed.

Print the final `coverage_metrics` output for each repo when done.
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


def verify_fix(kiss_root: Path, repo: Path) -> tuple[bool, ParsedMetrics, str]:
    output = run_coverage_metrics(kiss_root, repo)
    metrics = parse_coverage_metrics_output(output)
    return metrics_pass(metrics), metrics, output
