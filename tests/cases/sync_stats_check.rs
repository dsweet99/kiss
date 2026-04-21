use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

/// Metric IDs that are NOT directly comparable between `stats --all` and `check`.
///
/// Architectural asymmetries (by design):
/// - `cycle_size`, `inv_test_coverage`: aggregate-only (no per-unit STAT line).
/// - `duplication`, `orphan_module`, `test_coverage`: gate-only (check emits, stats doesn't).
/// - `fan_in`, `fan_out`: stats reports them but check never emits violations for them.
/// - `indirect_dependencies`: check only emits when `fan_in` > 0.
/// - `arguments_per_function`: stats reports total args; check splits into `positional`/`keyword_only`.
/// - `dependency_depth`: check emits with module-qualified name differing from stats' filename.
///
/// Known gaps (check omits violations that stats reports):
/// - `returns_per_function`: stats reports it, but check has no `chk!` macro for it.
/// - `positional_args`: check skips enforcement inside class methods (`!inside_class` guard).
/// - `decorators_per_function` vs `annotations_per_function`: check emits `decorators_per_function`
///   but stats uses `annotations_per_function` as the metric ID.
const NON_SHARED_METRICS: &[&str] = &[
    // Architectural
    "cycle_size",
    "inv_test_coverage",
    "duplication",
    "orphan_module",
    "test_coverage",
    "fan_in",
    "fan_out",
    "indirect_dependencies",
    "arguments_per_function",
    "dependency_depth",
    // Known gaps (remove items when the gap is fixed)
    "returns_per_function",           // no chk! macro in counts/mod.rs
    "positional_args",                // check skips inside_class methods
    "annotations_per_function",       // stats ID; check emits "decorators_per_function" instead
    "decorators_per_function",        // check ID; stats emits "annotations_per_function" instead
];

fn is_shared_metric(id: &str) -> bool {
    !NON_SHARED_METRICS.contains(&id)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MetricEntry {
    metric_id: String,
    file_stem: String,
    name: String,
    value: usize,
}

fn file_stem_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Parse `STAT:<metric_id>:<value>:<file>:<line>:<name>` lines.
fn parse_stat_lines(stdout: &str) -> Vec<MetricEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let tail = line.strip_prefix("STAT:")?;
            let parts: Vec<&str> = tail.splitn(5, ':').collect();
            if parts.len() < 5 { return None; }
            Some(MetricEntry {
                metric_id: parts[0].to_string(),
                value: parts[1].parse().ok()?,
                file_stem: file_stem_of(parts[2]),
                name: parts[4].to_string(),
            })
        })
        .collect()
}

/// Parse `VIOLATION:<metric_id>:<file>:<line>:<name>: <message>` lines.
fn parse_violation_lines(stdout: &str) -> Vec<MetricEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let tail = line.strip_prefix("VIOLATION:")?;
            let parts: Vec<&str> = tail.splitn(5, ':').collect();
            if parts.len() < 5 { return None; }
            let value: usize = parts[4]
                .split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
                .and_then(|s| s.parse().ok())?;
            Some(MetricEntry {
                metric_id: parts[0].to_string(),
                file_stem: file_stem_of(parts[1]),
                name: parts[3].to_string(),
                value,
            })
        })
        .collect()
}

fn build_sync_corpus(dir: &std::path::Path) {
    fs::write(
        dir.join("big_module.py"),
        r"import os
import json
from typing import Optional, Protocol

class Sink(Protocol):
    def write(self, s: str) -> None: ...

class DataProcessor:
    def __init__(self):
        self.data = []
    def add(self, item):
        self.data.append(item)
    def remove(self, item):
        self.data.remove(item)
    def clear(self):
        self.data.clear()
    def size(self):
        return len(self.data)

def complex_function(a, b, c, d, *, key=None, verbose: bool = False):
    x = 1
    y = 2
    z = 3
    w = 4
    result = []
    for i in range(a):
        if i > b:
            if i > c:
                result.append(i)
            elif i > d:
                result.append(i * 2)
            else:
                result.append(0)
        else:
            result.append(-1)
    try:
        val1 = int(x)
        val2 = int(y)
        val3 = int(z)
        val4 = int(w)
    except ValueError:
        return None
    if verbose:
        print(result)
    return result

def helper_a():
    return 1

def helper_b():
    return 2

def helper_c():
    return 3

def helper_d():
    return 4

def helper_e():
    return 5

def helper_f():
    return 6

def helper_g():
    return 7

def helper_h():
    return 8

def helper_i():
    return 9

def helper_j():
    return 10

def helper_k():
    return 11
",
    )
    .unwrap();

    fs::write(
        dir.join("test_big_module.py"),
        r"import big_module

def test_complex():
    big_module.complex_function(10, 5, 3, 1)

def test_helpers():
    big_module.helper_a()
    big_module.helper_b()
",
    )
    .unwrap();
}

fn write_zero_threshold_config(path: &std::path::Path) {
    fs::write(
        path,
        r"[gate]
test_coverage_threshold = 0
min_similarity = 1.0
duplication_enabled = false
orphan_module_enabled = false

[python]
statements_per_function = 0
positional_args = 0
keyword_only_args = 0
max_indentation = 0
nested_function_depth = 0
returns_per_function = 0
return_values_per_function = 0
branches_per_function = 0
local_variables = 0
statements_per_try_block = 0
boolean_parameters = 0
decorators_per_function = 0
calls_per_function = 0
methods_per_class = 0
statements_per_file = 0
lines_per_file = 0
functions_per_file = 0
interface_types_per_file = 0
concrete_types_per_file = 0
imported_names_per_file = 0
cycle_size = 0
indirect_dependencies = 0
dependency_depth = 0
",
    )
    .unwrap();
}

type MetricKey<'a> = (&'a str, &'a str, &'a str);

fn build_metric_map(entries: &[MetricEntry]) -> BTreeMap<MetricKey<'_>, usize> {
    entries
        .iter()
        .filter(|e| is_shared_metric(&e.metric_id))
        .map(|e| {
            (
                (e.metric_id.as_str(), e.file_stem.as_str(), e.name.as_str()),
                e.value,
            )
        })
        .collect()
}

fn run_stats(config: &std::path::Path, corpus: &std::path::Path, home: &std::path::Path) -> String {
    let out = kiss_binary()
        .arg("--config").arg(config)
        .arg("stats").arg("--all=999")
        .arg(corpus)
        .env("HOME", home)
        .output()
        .expect("kiss stats should run");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "kiss stats failed:\n{stdout}");
    stdout
}

fn run_check(config: &std::path::Path, corpus: &std::path::Path, home: &std::path::Path) -> String {
    let out = kiss_binary()
        .arg("--config").arg(config)
        .arg("check").arg("--lang").arg("python").arg("--all")
        .arg(corpus)
        .env("HOME", home)
        .output()
        .expect("kiss check should run");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn find_sync_failures(
    stat_map: &BTreeMap<MetricKey<'_>, usize>,
    viol_map: &BTreeMap<MetricKey<'_>, usize>,
) -> Vec<String> {
    let stat_metric_ids: BTreeSet<&str> = stat_map.keys().map(|(m, _, _)| *m).collect();
    let mut failures = Vec::new();

    let mut mismatches = Vec::new();
    let mut missing_check = Vec::new();
    for (&(metric, file, name), &stat_val) in stat_map {
        if stat_val == 0 { continue; }
        match viol_map.get(&(metric, file, name)) {
            Some(&v) if v != stat_val => mismatches.push(format!(
                "  {metric} {file}::{name}: stats={stat_val} check={v}"
            )),
            None => missing_check.push(format!(
                "  {metric} {file}::{name}: stats={stat_val}, no VIOLATION"
            )),
            _ => {}
        }
    }

    let mut missing_stats = Vec::new();
    for (&(metric, file, name), &check_val) in viol_map {
        if stat_metric_ids.contains(metric) && !stat_map.contains_key(&(metric, file, name)) {
            missing_stats.push(format!(
                "  {metric} {file}::{name}: check={check_val}, no STAT"
            ));
        }
    }

    if !mismatches.is_empty() {
        failures.push(format!("VALUE MISMATCHES:\n{}", mismatches.join("\n")));
    }
    if !missing_check.is_empty() {
        failures.push(format!("MISSING FROM CHECK:\n{}", missing_check.join("\n")));
    }
    if !missing_stats.is_empty() {
        failures.push(format!("MISSING FROM STATS:\n{}", missing_stats.join("\n")));
    }
    failures
}

#[test]
fn stats_and_check_agree_on_shared_metrics() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    build_sync_corpus(tmp.path());

    let config_path = tmp.path().join(".kissconfig");
    write_zero_threshold_config(&config_path);

    let stats_stdout = run_stats(&config_path, tmp.path(), home.path());
    let check_stdout = run_check(&config_path, tmp.path(), home.path());

    let stat_entries = parse_stat_lines(&stats_stdout);
    let viol_entries = parse_violation_lines(&check_stdout);
    assert!(!stat_entries.is_empty(), "no STAT lines:\n{stats_stdout}");
    assert!(!viol_entries.is_empty(), "no VIOLATION lines:\n{check_stdout}");

    let stat_map = build_metric_map(&stat_entries);
    let viol_map = build_metric_map(&viol_entries);
    let failures = find_sync_failures(&stat_map, &viol_map);

    assert!(
        failures.is_empty(),
        "stats/check out of sync:\n\n{}\n\n--- stats ---\n{stats_stdout}\n--- check ---\n{check_stdout}",
        failures.join("\n\n")
    );
}
