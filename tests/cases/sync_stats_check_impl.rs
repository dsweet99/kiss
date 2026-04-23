use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;

pub(super) fn kiss_binary() -> Command {
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
/// Known gaps (check omits enforcement that stats still reports):
/// - `positional_args`: check skips for methods; stats still list STATs for all units. The sync
///   test cannot match method-only rows to check output, so the metric is excluded in full.
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
    "positional_args", // check skips inside_class methods
];

fn is_shared_metric(id: &str) -> bool {
    !NON_SHARED_METRICS.contains(&id)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct MetricEntry {
    pub(super) metric_id: String,
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
pub(super) fn parse_stat_lines(stdout: &str) -> Vec<MetricEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let tail = line.strip_prefix("STAT:")?;
            let parts: Vec<&str> = tail.splitn(5, ':').collect();
            if parts.len() < 5 {
                return None;
            }
            Some(MetricEntry {
                metric_id: parts[0].to_string(),
                value: parts[1].parse().ok()?,
                file_stem: file_stem_of(parts[2]),
                name: parts[4].to_string(),
            })
        })
        .collect()
}

fn violation_message_observed_value(message_and_suggestion: &str) -> Option<usize> {
    let s = message_and_suggestion.trim_start();
    if s.contains('%') && s.contains("covered") {
        let head = s.split('%').next()?;
        let digits: String = head
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            return None;
        }
        return digits.parse().ok();
    }
    let before_threshold = s.split(" (threshold: ").next().unwrap_or(s);
    for needle in [" has ", "File has "] {
        if let Some(pos) = before_threshold.find(needle) {
            let rest = &before_threshold[pos + needle.len()..];
            let digits: String = rest
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(v) = digits.parse::<usize>() {
                return Some(v);
            }
        }
    }
    None
}

fn violation_message_fallback_digits(message: &str) -> Option<usize> {
    let before_threshold = message.split(" (threshold: ").next()?;
    let s = before_threshold.trim_start();
    for needle in [" has ", "File has "] {
        if let Some(pos) = s.find(needle) {
            let rest = &s[pos + needle.len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(v) = digits.parse::<usize>() {
                return Some(v);
            }
        }
    }
    None
}

fn parse_violation_line(line: &str) -> Option<MetricEntry> {
    let tail = line.strip_prefix("VIOLATION:")?;
    let parts: Vec<&str> = tail.splitn(5, ':').collect();
    if parts.len() < 5 {
        return None;
    }
    let value: usize = violation_message_observed_value(parts[4])
        .or_else(|| violation_message_fallback_digits(parts[4]))?;
    Some(MetricEntry {
        metric_id: parts[0].to_string(),
        file_stem: file_stem_of(parts[1]),
        name: parts[3].to_string(),
        value,
    })
}

pub(super) fn parse_violation_lines(stdout: &str) -> Vec<MetricEntry> {
    stdout.lines().filter_map(parse_violation_line).collect()
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

[thresholds]
statements_per_function = 0
methods_per_class = 0
statements_per_file = 0
lines_per_file = 0
functions_per_file = 0
arguments_per_function = 0
arguments_positional = 0
arguments_keyword_only = 0
max_indentation_depth = 0
interface_types_per_file = 0
concrete_types_per_file = 0
nested_function_depth = 0
returns_per_function = 0
return_values_per_function = 0
branches_per_function = 0
local_variables_per_function = 0
imported_names_per_file = 0
statements_per_try_block = 0
boolean_parameters = 0
annotations_per_function = 0
calls_per_function = 0
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

pub(super) fn run_stats(
    config: &std::path::Path,
    corpus: &std::path::Path,
    home: &std::path::Path,
) -> String {
    let out = kiss_binary()
        .arg("--config")
        .arg(config)
        .arg("stats")
        .arg("--all=999")
        .arg(corpus)
        .env("HOME", home)
        .output()
        .expect("kiss stats should run");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "kiss stats failed:\n{stdout}");
    stdout
}

pub(super) fn run_check(
    config: &std::path::Path,
    corpus: &std::path::Path,
    home: &std::path::Path,
) -> String {
    let out = kiss_binary()
        .arg("--config")
        .arg(config)
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(corpus)
        .env("HOME", home)
        .output()
        .expect("kiss check should run");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let code = out.status.code();
    assert!(
        matches!(code, Some(0 | 1)),
        "kiss check abnormal exit ({code:?}):\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

pub(super) fn find_sync_failures(
    stat_map: &BTreeMap<MetricKey<'_>, usize>,
    viol_map: &BTreeMap<MetricKey<'_>, usize>,
) -> Vec<String> {
    let stat_metric_ids: BTreeSet<&str> = stat_map.keys().map(|(m, _, _)| *m).collect();
    let mut failures = Vec::new();

    let mut mismatches = Vec::new();
    let mut missing_check = Vec::new();
    for (&(metric, file, name), &stat_val) in stat_map {
        match viol_map.get(&(metric, file, name)) {
            Some(&v) if v == stat_val => {}
            Some(&v) => mismatches.push(format!(
                "  {metric} {file}::{name}: stats={stat_val} check={v}"
            )),
            None if stat_val > 0 => missing_check.push(format!(
                "  {metric} {file}::{name}: stats={stat_val}, no VIOLATION"
            )),
            None => {}
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

pub(super) fn sync_stats_test_body(
    tmp: &std::path::Path,
    home: &std::path::Path,
) -> (String, String, String) {
    build_sync_corpus(tmp);

    let config_path = tmp.join(".kissconfig");
    write_zero_threshold_config(&config_path);

    let stats_stdout = run_stats(&config_path, tmp, home);
    let check_stdout = run_check(&config_path, tmp, home);
    let stat_entries = parse_stat_lines(&stats_stdout);
    let viol_entries = parse_violation_lines(&check_stdout);
    let stat_map = build_metric_map(&stat_entries);
    let viol_map = build_metric_map(&viol_entries);
    let failures = find_sync_failures(&stat_map, &viol_map);
    (stats_stdout, check_stdout, failures.join("\n\n"))
}
