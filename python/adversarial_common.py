
from __future__ import annotations

import secrets
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

AdversarialKind = Literal["foil", "cheat"]
CalibrationKind = Literal["repos", "foil", "cheat"]


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def ensure_import_path() -> None:
    root = str(repo_root())
    if root not in sys.path:
        sys.path.insert(0, root)


def calibration_repos_root() -> Path:
    return repo_root().parent / "repos"


def adversarial_root() -> Path:
    return repo_root().parent / "kiss-adversarial"


def _adversarial_id_suffix() -> str:
    stamp = datetime.now(UTC).strftime("%Y%m%d_%H%M%S")
    return f"{stamp}_{secrets.token_hex(2)}"


def allocate_adversarial_id(kind: AdversarialKind) -> Path:
    root = adversarial_root() / kind
    base = _adversarial_id_suffix()
    candidate = root / base
    while candidate.exists():
        candidate = root / f"{base}_{secrets.token_hex(2)}"
    return candidate


def discover_calibration_repos() -> list[tuple[CalibrationKind, Path]]:
    entries: list[tuple[CalibrationKind, Path]] = []

    repos_root = calibration_repos_root()
    if repos_root.is_dir():
        for child in sorted(repos_root.iterdir(), key=lambda p: p.name):
            if child.is_dir() and not child.name.startswith("."):
                entries.append(("repos", child))

    adv_root = adversarial_root()
    for kind in ("foil", "cheat"):
        kind_dir = adv_root / kind
        if not kind_dir.is_dir():
            continue
        for child in sorted(kind_dir.iterdir(), key=lambda p: p.name):
            if child.is_dir() and not child.name.startswith("."):
                entries.append((kind, child))

    return entries
