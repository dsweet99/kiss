//! Regression tests for `kiss stats --all`.
//!
//! Two defects, observed by the user when `kiss stats` (summary mode) reports
//! `concrete_types_per_file` for the kiss codebase but `kiss stats --all` does not
//! emit any STAT line for that metric:
//!
//!   1. The hand-rolled metric list in `src/bin_cli/stats/top.rs` covers only 17
//!      metrics, while the canonical registry `kiss::METRICS`
//!      (`src/stats/definitions.rs`) lists 27. The missing metrics include
//!      `concrete_types_per_file`, `interface_types_per_file`, `statements_per_file`,
//!      `functions_per_file`, `statements_per_try_block`, `boolean_parameters`,
//!      `annotations_per_function`, `calls_per_function`, and `cycle_size`.
//!      `UnitMetrics` itself also lacks fields for several of these, so the data
//!      is never plumbed through.
//!
//!   2. The metric IDs that `--all` does emit are non-canonical (`args_total`,
//!      `args_positional`, `args_keyword_only`, `indirect_deps`), where the
//!      registry uses `arguments_per_function`, `positional_args`, `keyword_only_args`,
//!      `indirect_dependencies`. Downstream tooling joining on metric IDs
//!      silently fails to match.
//!
//! Both tests below should FAIL on `dsweet/check_all` and pass once
//! `print_all_top_metrics` is driven from `kiss::METRICS` and `UnitMetrics`
//! is widened to carry every registered metric.

use kiss::METRICS;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

/// Parse the distinct metric IDs that `kiss stats --all` printed in `STAT:<id>:` lines.
fn parsed_stat_ids(stdout: &str) -> BTreeSet<String> {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("STAT:"))
        .filter_map(|tail| tail.split(':').next())
        .map(ToString::to_string)
        .collect()
}

fn registry_ids() -> BTreeSet<String> {
    METRICS.iter().map(|m| m.metric_id.to_string()).collect()
}

/// Run `kiss stats --all` against a corpus that exercises a broad set of metrics
/// (concrete types, interface types via Protocols, multi-arg functions, returns,
/// branches, locals, nested defs, imports, multiple files for `fan_in`/`fan_out`).
fn build_broad_python_corpus(dir: &std::path::Path) {
    fs::write(
        dir.join("models.py"),
        // Concrete classes -> concrete_types_per_file.
        // Multiple top-level functions -> functions_per_file, statements_per_file.
        // Method count -> methods_per_class.
        // Imports -> imported_names_per_file (and fan_out).
        r#"import os
import json
from typing import Optional

class Account:
    def __init__(self, owner: str, balance: int = 0):
        self.owner = owner
        self.balance = balance
    def deposit(self, amount: int) -> int:
        self.balance += amount
        return self.balance
    def withdraw(self, amount: int) -> int:
        if amount > self.balance:
            return -1
        self.balance -= amount
        return self.balance
    def describe(self) -> str:
        return f"{self.owner}:{self.balance}"

class Ledger:
    def __init__(self):
        self.accounts = []
    def add(self, a: Account) -> None:
        self.accounts.append(a)
    def total(self) -> int:
        s = 0
        for a in self.accounts:
            s += a.balance
        return s

def make_account(owner: str, *, opening: int = 0, *, premium: bool = False) -> Account:
    return Account(owner, opening)

def report(ledger: Ledger) -> str:
    parts = []
    for a in ledger.accounts:
        parts.append(a.describe())
    return ",".join(parts)
"#,
    )
    .unwrap();

    fs::write(
        dir.join("protocols.py"),
        // Protocols/ABCs -> interface_types_per_file (vs concrete_types_per_file).
        r"from typing import Protocol, runtime_checkable

@runtime_checkable
class Sink(Protocol):
    def write(self, s: str) -> None: ...

@runtime_checkable
class Source(Protocol):
    def read(self) -> str: ...
",
    )
    .unwrap();

    fs::write(
        dir.join("uses_models.py"),
        // Imports models -> creates a graph edge so fan_in on models.py > 0.
        // try/except block -> statements_per_try_block.
        // Boolean parameter -> boolean_parameters.
        r#"import models

def run(verbose: bool):
    try:
        l = models.Ledger()
        a = models.make_account("alice")
        l.add(a)
        return l.total()
    except Exception:
        if verbose:
            return -1
        return 0
"#,
    )
    .unwrap();
}

fn run_stats_all(corpus: &std::path::Path) -> String {
    let output = kiss_binary()
        .arg("stats")
        .arg("--all")
        .arg(corpus)
        .output()
        .expect("kiss binary should execute");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "kiss stats --all should succeed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn regression_stats_all_emits_only_canonical_metric_ids_from_registry() {
    // Restated bug: `kiss stats --all` emits metric IDs that are not present in
    // `kiss::METRICS` (e.g. `args_total`, `args_positional`, `args_keyword_only`,
    // `indirect_deps`). Anything keyed on registry IDs (the summary table,
    // `.kissconfig`, mimic) cannot join against `--all` output.
    //
    // Hypothesis: Every `STAT:<id>:` line emitted by `--all` must use an `id`
    // that is a member of `kiss::METRICS`.
    //
    // Predicted failure on `dsweet/check_all`: at least the four non-canonical
    // IDs above appear and are reported as offenders.
    let tmp = TempDir::new().unwrap();
    build_broad_python_corpus(tmp.path());

    let stdout = run_stats_all(tmp.path());
    let emitted = parsed_stat_ids(&stdout);
    let registry = registry_ids();

    let unknown: Vec<&String> = emitted.difference(&registry).collect();
    assert!(
        unknown.is_empty(),
        "kiss stats --all emitted metric IDs that are not in kiss::METRICS:\n  unknown: {unknown:?}\n  registry: {registry:?}\n  full stdout:\n{stdout}"
    );
}

#[test]
fn regression_stats_all_emits_concrete_types_per_file_when_corpus_has_classes() {
    // Restated bug (user-observed): `kiss stats` summary reports
    // `concrete_types_per_file`, but `kiss stats --all` produces zero STAT lines
    // for it on the same corpus.
    //
    // Hypothesis: When the corpus contains at least one concrete class, `--all`
    // must emit at least one `STAT:concrete_types_per_file:` line.
    //
    // Predicted failure on `dsweet/check_all`: the metric is absent from the
    // hand-rolled list in `src/bin_cli/stats/top.rs` AND missing from
    // `UnitMetrics`, so no STAT line is ever produced.
    let tmp = TempDir::new().unwrap();
    build_broad_python_corpus(tmp.path());

    let stdout = run_stats_all(tmp.path());
    assert!(
        stdout.contains("STAT:concrete_types_per_file:"),
        "expected at least one STAT line for `concrete_types_per_file` (corpus has classes Account/Ledger).\nstdout:\n{stdout}"
    );
}

#[test]
fn regression_stats_all_emits_every_file_scope_metric_with_data() {
    // Restated bug: the `--all` metric list in `src/bin_cli/stats/top.rs` is
    // a hand-rolled subset of the canonical `kiss::METRICS` registry. New
    // metrics added to the registry are silently absent from `--all` output.
    //
    // Hypothesis: For each File-scope metric in `kiss::METRICS`, the broad
    // corpus produces non-zero data, so `--all` must emit at least one STAT
    // line for it.
    //
    // Predicted failure on `dsweet/check_all`: at minimum
    // `concrete_types_per_file`, `interface_types_per_file`,
    // `statements_per_file`, and `functions_per_file` are missing from output.
    let tmp = TempDir::new().unwrap();
    build_broad_python_corpus(tmp.path());

    let stdout = run_stats_all(tmp.path());
    let emitted = parsed_stat_ids(&stdout);

    let file_scope_metrics: Vec<&str> = METRICS
        .iter()
        .filter(|m| matches!(m.scope, kiss::MetricScope::File))
        .map(|m| m.metric_id)
        .collect();

    let missing: Vec<&str> = file_scope_metrics
        .iter()
        .copied()
        .filter(|id| !emitted.contains(*id))
        .collect();

    assert!(
        missing.is_empty(),
        "kiss stats --all is missing STAT lines for these File-scope registry metrics:\n  missing: {missing:?}\n  emitted: {emitted:?}\n  full stdout:\n{stdout}"
    );
}
