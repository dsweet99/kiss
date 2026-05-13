use kiss::METRICS;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

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

fn build_broad_python_corpus(dir: &std::path::Path) {
    fs::write(
        dir.join("models.py"),
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
    run_stats_all_with_n(corpus, None)
}

fn run_stats_all_with_n(corpus: &std::path::Path, n: Option<usize>) -> String {
    let all_arg = n.map_or_else(|| "--all".to_string(), |v| format!("--all={v}"));
    let output = kiss_binary()
        .arg("stats")
        .arg(&all_arg)
        .arg(corpus)
        .output()
        .expect("kiss binary should execute");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "kiss stats {all_arg} should succeed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn regression_stats_all_emits_only_canonical_metric_ids_from_registry() {
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

#[test]
fn regression_stats_all_ranks_uncovered_above_covered_for_inv_test_coverage() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("uncovered.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("covered.rs"),
        "pub fn gamma() {}\npub fn delta() {}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("covered_test.rs"),
        "#[test]\nfn t1() { gamma(); delta(); }\n",
    )
    .unwrap();

    let stdout = run_stats_all_with_n(tmp.path(), Some(1));
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("STAT:inv_test_coverage:"))
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("STAT:inv_test_coverage:100:") && l.contains("uncovered.rs")),
        "expected an inv_test_coverage:100 line for uncovered.rs at the top of the ranking.\n\
         lines: {lines:?}\nfull stdout:\n{stdout}"
    );
    assert!(
        lines.iter().all(|l| !l.contains("covered.rs:")
            || l.contains("uncovered.rs:")
            || !l.starts_with("STAT:inv_test_coverage:0:")),
        "with --all=1, the covered file (inv_test_coverage=0) must rank below the uncovered \
         file and be omitted.\nlines: {lines:?}"
    );
}

#[test]
fn regression_stats_all_emits_cycle_size_when_corpus_has_cycle() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("a.py"),
        "import b\ndef use_b():\n    return b.thing()\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("b.py"),
        "import c\ndef thing():\n    return c.other()\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("c.py"),
        "import a\ndef other():\n    return a.use_b()\n",
    )
    .unwrap();
    let stdout = run_stats_all(tmp.path());
    let cycle_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("STAT:cycle_size:"))
        .collect();
    assert!(
        !cycle_lines.is_empty(),
        "expected at least one STAT:cycle_size: line for a 3-module cycle.\nstdout:\n{stdout}"
    );
    assert!(
        cycle_lines.iter().any(|l| l.starts_with("STAT:cycle_size:3:")),
        "expected cycle_size value of 3 for the a → b → c → a cycle.\nlines: {cycle_lines:?}"
    );
}
