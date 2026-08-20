use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

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
    let output = kiss_binary()
        .arg("stats")
        .arg("--defaults")
        .arg(tmp.path())
        .output()
        .unwrap();
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
    let output = kiss_binary()
        .arg("stats")
        .arg("--defaults")
        .arg(tmp.path())
        .output()
        .unwrap();
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
fn cli_stats_summary_table_omits_coverage_rows() {
    let tmp = TempDir::new().unwrap();
    build_corpus(tmp.path());
    let output = kiss_binary()
        .arg("stats")
        .arg("--defaults")
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "kiss stats should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.lines().any(|l| l.starts_with("inv_test_coverage")),
        "stats summary must not report coverage metrics.\nfull stdout:\n{stdout}"
    );
    assert!(
        !stdout.lines().any(|l| l.starts_with("test_coverage ")),
        "stats summary must not report coverage metrics.\nfull stdout:\n{stdout}"
    );
}

#[test]
fn cli_stats_summary_respects_explicit_config_override_for_gate_behavior() {
    let tmp = TempDir::new().unwrap();
    build_simple_corpus_for_violation_comparison(tmp.path());

    let local = tmp.path().join(".kissconfig");
    fs::write(
        &local,
        "[global]\nduplication_enabled = true\norphan_module_enabled = true\nmin_similarity = 0.7\n",
    )
    .unwrap();
    let custom = tmp.path().join("custom.toml");
    fs::write(
        &custom,
        "[global]\nduplication_enabled = false\norphan_module_enabled = false\nmin_similarity = 1.0\n",
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
        "[global]\nduplication_enabled = false\norphan_module_enabled = false\n",
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
