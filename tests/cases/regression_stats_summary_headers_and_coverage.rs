//! Regression tests for the new `kiss stats` summary headers and `test_coverage`
//! row added in response to the request:
//!
//! > In the `kiss stats` output, could you include a header like
//! > `Analyzed: 213 files, 1967 code_units, 8880 statements, 213 graph_nodes, 335 graph_edges`
//! > and another with
//! > - number of duplicate code violations (according to `kiss check` parameters)
//! > - number of orphan code violations
//! > and in the tables, could you include coverage (which is now a per-file metric).
//!
//! Each test below pins one of those three contracts. They are intentionally
//! tolerant about whitespace and exact numeric values — only structural
//! invariants are asserted, so unrelated metric drift won't break them.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

/// Build a tiny Python corpus with: an importer, an importee, an orphan module
/// (imported by nobody), and a near-duplicate function pair so that both
/// duplicate and orphan counts have a chance to be > 0 in the summary headers.
fn build_corpus(dir: &std::path::Path) {
    fs::write(
        dir.join("importer.py"),
        "import importee\n\ndef use():\n    return importee.value()\n",
    )
    .unwrap();
    fs::write(dir.join("importee.py"), "def value():\n    return 42\n").unwrap();
    fs::write(
        dir.join("lonely_orphan.py"),
        "def nobody_calls_me():\n    x = 1\n    y = 2\n    return x + y\n",
    )
    .unwrap();
    // Two long, near-identical functions in different files so the duplicate
    // detector (default `min_similarity = 0.7`) classifies them as a cluster.
    let dup_body = (0..40)
        .map(|i| format!("    a{i} = {i} + {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        dir.join("dup_a.py"),
        format!("def dup_a():\n{dup_body}\n    return a0\n"),
    )
    .unwrap();
    fs::write(
        dir.join("dup_b.py"),
        format!("def dup_b():\n{dup_body}\n    return a0\n"),
    )
    .unwrap();
}

fn build_simple_corpus_for_violation_comparison(dir: &std::path::Path) {
    build_corpus(dir);
    fs::write(
        dir.join("configurable.py"),
        "def first():\n    return 1\n\ndef second():\n    return 2\n",
    )
    .unwrap();
}

fn parse_violation_counts(stdout: &str) -> (usize, usize) {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("Violations:"))
        .unwrap_or_else(|| panic!("missing `Violations:` line in stdout:\n{stdout}"));
    let mut values: Vec<usize> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<usize>().ok())
        .collect();
    assert!(
        values.len() >= 2,
        "expected at least 2 integers in `Violations:` line: {line}\nfull stdout:\n{stdout}"
    );
    (values.remove(0), values.remove(0))
}

#[test]
fn cli_stats_summary_emits_analyzed_header_with_five_global_metrics() {
    let tmp = TempDir::new().unwrap();
    build_corpus(tmp.path());
    let output = kiss_binary().arg("stats").arg(tmp.path()).output().unwrap();
    assert!(output.status.success(), "kiss stats should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let line = stdout
        .lines()
        .find(|l| l.starts_with("Analyzed:"))
        .unwrap_or_else(|| panic!("missing `Analyzed:` header in stdout:\n{stdout}"));

    for needle in [
        "files",
        "code_units",
        "statements",
        "graph_nodes",
        "graph_edges",
    ] {
        assert!(
            line.contains(needle),
            "Analyzed header missing `{needle}`: {line}\nfull stdout:\n{stdout}"
        );
    }
}

#[test]
fn cli_stats_summary_emits_violations_header_with_duplicate_and_orphan_counts() {
    let tmp = TempDir::new().unwrap();
    build_corpus(tmp.path());
    let output = kiss_binary().arg("stats").arg(tmp.path()).output().unwrap();
    assert!(output.status.success(), "kiss stats should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let line = stdout
        .lines()
        .find(|l| l.starts_with("Violations:"))
        .unwrap_or_else(|| panic!("missing `Violations:` header in stdout:\n{stdout}"));

    assert!(
        line.contains("duplicate"),
        "Violations header missing `duplicate`: {line}\nfull stdout:\n{stdout}"
    );
    assert!(
        line.contains("orphan"),
        "Violations header missing `orphan`: {line}\nfull stdout:\n{stdout}"
    );

    // Corpus is constructed so both counts must be > 0 — a regression where the
    // computation is silently skipped (e.g. always reporting 0) will fail here.
    let nums: Vec<usize> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    assert_eq!(
        nums.len(),
        2,
        "expected exactly 2 numbers in `Violations:` line ({line:?}); full stdout:\n{stdout}"
    );
    assert!(
        nums[0] > 0,
        "expected duplicate count > 0 (corpus has dup_a/dup_b near-clones); line: {line}\nstdout:\n{stdout}"
    );
    assert!(
        nums[1] > 0,
        "expected orphan count > 0 (corpus has lonely_orphan.py); line: {line}\nstdout:\n{stdout}"
    );
}

#[test]
fn cli_stats_summary_table_includes_inv_test_coverage_row() {
    let tmp = TempDir::new().unwrap();
    build_corpus(tmp.path());
    let output = kiss_binary().arg("stats").arg(tmp.path()).output().unwrap();
    assert!(output.status.success(), "kiss stats should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The metric stored is `inv_test_coverage` (= 100 - coverage) so that
    // higher = worse, matching every other metric in the table.
    let line = stdout
        .lines()
        .find(|l| l.starts_with("inv_test_coverage"))
        .unwrap_or_else(|| {
            panic!(
                "summary table should include an `inv_test_coverage` row.\nfull stdout:\n{stdout}"
            )
        });
    assert!(
        !stdout.lines().any(|l| l.starts_with("test_coverage ")),
        "old `test_coverage` row must be gone (replaced by `inv_test_coverage`).\nfull stdout:\n{stdout}"
    );

    // The corpus contains `lonely_orphan.py`, which has 3 definitions and zero
    // test references → 0% covered → 100% inv_test_coverage. So at least one
    // file must surface a non-trivial inv_test_coverage value, ruling out the
    // off-by-one regression where the metric is silently always 0.
    let max_col: usize = line
        .split_whitespace()
        .next_back()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("could not parse `max` column from row: {line:?}"));
    assert!(
        max_col > 0,
        "expected `inv_test_coverage` max > 0 (corpus has uncovered orphan); row: {line}\nstdout:\n{stdout}"
    );
}

#[test]
fn cli_stats_summary_respects_explicit_config_override_for_gate_behavior() {
    let tmp = TempDir::new().unwrap();
    build_simple_corpus_for_violation_comparison(tmp.path());

    let local = tmp.path().join(".kissconfig");
    fs::write(
        &local,
        "[gate]\nduplication_enabled = true\norphan_module_enabled = true\nmin_similarity = 0.7\n",
    )
    .unwrap();
    let custom = tmp.path().join("custom.toml");
    fs::write(
        &custom,
        "[gate]\nduplication_enabled = false\norphan_module_enabled = false\nmin_similarity = 1.0\n",
    )
    .unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let local_out = kiss_binary()
        .current_dir(tmp.path())
        .arg("stats")
        .arg(tmp.path())
        .env("HOME", &home)
        .output()
        .unwrap();
    let local_stdout = String::from_utf8_lossy(&local_out.stdout);
    assert!(
        local_out.status.success(),
        "local stats should succeed:\n{local_stdout}"
    );
    let (local_dup, local_orphan) = parse_violation_counts(&local_stdout);
    assert!(
        local_dup > 0 && local_orphan > 0,
        "local config enables gate checks; expected both counts > 0 in:\n{local_stdout}"
    );

    let override_out = kiss_binary()
        .current_dir(tmp.path())
        .arg("stats")
        .arg(tmp.path())
        .arg("--config")
        .arg(custom)
        .env("HOME", &home)
        .output()
        .unwrap();
    let override_stdout = String::from_utf8_lossy(&override_out.stdout);
    assert!(
        override_out.status.success(),
        "stats with --config should succeed: {override_stdout}"
    );
    let (override_dup, override_orphan) = parse_violation_counts(&override_stdout);
    assert!(
        override_dup == 0 && override_orphan == 0,
        "explicit --config should disable both checks:\n{override_stdout}"
    );
}

#[test]
fn cli_stats_summary_defaults_can_disable_local_config_and_restore_defaults() {
    let tmp = TempDir::new().unwrap();
    build_simple_corpus_for_violation_comparison(tmp.path());

    fs::write(
        tmp.path().join(".kissconfig"),
        "[gate]\nduplication_enabled = false\norphan_module_enabled = false\n",
    )
    .unwrap();

    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let local_out = kiss_binary()
        .current_dir(tmp.path())
        .arg("stats")
        .arg(tmp.path())
        .env("HOME", &home)
        .output()
        .unwrap();
    let local_stdout = String::from_utf8_lossy(&local_out.stdout);
    assert!(
        local_out.status.success(),
        "local stats should succeed:\n{local_stdout}"
    );
    let (local_dup, local_orphan) = parse_violation_counts(&local_stdout);
    assert!(
        local_dup == 0 && local_orphan == 0,
        "local config disables gate checks: expected both zero.\nstdout:\n{local_stdout}"
    );

    let default_out = kiss_binary()
        .current_dir(tmp.path())
        .arg("stats")
        .arg("--defaults")
        .arg(tmp.path())
        .env("HOME", &home)
        .output()
        .unwrap();
    let default_stdout = String::from_utf8_lossy(&default_out.stdout);
    assert!(
        default_out.status.success(),
        "stats --defaults should succeed:\n{default_stdout}"
    );
    let (default_dup, default_orphan) = parse_violation_counts(&default_stdout);
    assert!(
        default_dup > 0 && default_orphan > 0,
        "defaults should ignore local .kissconfig and re-enable checks:\n{default_stdout}"
    );
}
