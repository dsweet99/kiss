# Coverage discrepancy metrics

Output of `ops/coverage_discrepancy.py`. Compares **kiss static test-name-reference coverage** (per file: % of code units whose names appear in tests) against **runtime line coverage** from `cargo-llvm-cov` (Rust) or slipcover (Python).

---

## Context fields

**language** — Which runtime tool was used: `rust` (llvm-cov) or `python` (slipcover).

**aligned_files** — Number of source files present in both kiss’s per-file map and the runtime per-file report. Only these files enter the discrepancy calculations below.

---

## Headline percentages

**kiss_median_static_pct** — Median per-file static coverage across the repo, in percent. Equivalently `100 − p50(inv_test_coverage)` from `kiss stats`. Each file’s value is the share of kiss code units (functions, methods, etc.) whose names appear in test code.

**runtime_total_line_pct** — Project-wide runtime line coverage: executed lines ÷ total instrumented lines, from llvm-cov or slipcover.

**global_gap** — `|kiss_median_static_pct − runtime_total_line_pct|`. A single-number summary of how far apart the two headline percentages are. Can be small even when per-file disagreement is large.

---

## Per-file disagreement

**file_mae** — Mean absolute error across aligned files: average of `|kiss_pct − runtime_pct|`. Penalizes all gaps equally.

**file_rmse** — Root mean square error across aligned files, normalized to **[0, 1]**: `sqrt(mean((kiss_pct − runtime_pct)²)) / 100`. Penalizes large per-file gaps more than MAE. This is the primary **discrepancy score**; 0 = perfect file-level agreement, 1 = every file differs by 100 percentage points.

**spearman** — Spearman rank correlation between per-file kiss pct and runtime pct (−1 to 1). Measures whether files with low kiss coverage tend to have low runtime coverage, regardless of scale. 1 = same rank order; 0 = unrelated; needs at least two aligned files.

---

## Directional rates

Both use a fixed threshold of **20 percentage points** on per-file differences `kiss_pct − runtime_pct`.

**inflation_rate** — Fraction of aligned files where kiss overstates coverage: `kiss_pct ≥ runtime_pct + 20`. Typical cause: name-only test references (e.g. `stringify!`) that never execute the production code.

**blind_spot_rate** — Fraction of aligned files where kiss understates coverage: `runtime_pct ≥ kiss_pct + 20`. Typical cause: integration tests that run code without naming every symbol kiss tracks.

---

## Composite

**discrepancy_score (file_rmse)** — Same normalized value as **file_rmse** (0–1); repeated as the recommended single scalar for comparing repos or tracking improvement over time.

---

## Detailed output

Pass **`--detailed`** to print a per-file table after the summary (sorted by `|delta|` descending). Columns:

| Column | Meaning |
|--------|---------|
| **file** | Path relative to the repo root |
| **kiss** | Per-file static coverage (%) |
| **runtime** | Per-file line coverage (%) |
| **delta** | `kiss − runtime` (positive = kiss higher) |
| **flag** | `inflated` if delta ≥ 20; `blind_spot` if delta ≤ −20; empty otherwise |

Pass **`--report-out PATH`** to write the same summary plus the full file list as JSON (useful for diffing or spreadsheets).

Example:

```bash
python3 ops/coverage_discrepancy.py rust /path/to/repo --detailed
python3 ops/coverage_discrepancy.py rust /path/to/repo --report-out /tmp/coverage_report.json
```
