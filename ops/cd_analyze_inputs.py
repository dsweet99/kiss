from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class AnalyzeInputs:
    repo: Path
    language: str
    runtime_map: dict[Path, float]
    runtime_total: float
    kiss_map: dict[Path, float]
    kiss_median: float
