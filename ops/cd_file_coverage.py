from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

DISCREPANCY_THRESHOLD = 20


@dataclass(frozen=True)
class FileCoverage:
    path: Path
    kiss_pct: float
    runtime_pct: float

    @property
    def delta(self) -> float:
        return self.kiss_pct - self.runtime_pct

    @property
    def abs_delta(self) -> float:
        return abs(self.delta)

    @property
    def flag(self) -> str:
        if self.delta >= DISCREPANCY_THRESHOLD:
            return "inflated"
        if self.delta <= -DISCREPANCY_THRESHOLD:
            return "blind_spot"
        return ""
