//! Regression: unreferenced unit violations must not read "100% covered".
//!
//! `kiss check --all` can compute weighted file percentages that disagree with
//! definition-level unreferenced detection. Violation lines must use unweighted
//! percentages (or a distinct template) so gate and bypass modes stay interpretable.

use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

/// Canonical repro from `bug_report.md`: co-located `#[test]` functions inflate
/// weighted file coverage while helpers remain unreferenced.
#[test]
fn bug_check_all_never_claims_100_percent_on_unreferenced_unit() {
    let home = TempDir::new().unwrap();
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let target = "src/analyze/tests_coverage.rs";

    let out = kiss_binary()
        .current_dir(manifest_dir)
        .arg("check")
        .arg("--all")
        .arg(target)
        .env("HOME", home.path())
        .output()
        .expect("kiss check --all should run");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("VIOLATION:test_coverage"),
        "expected coverage violations for unreferenced helpers:\n{stdout}"
    );
    assert!(
        !stdout.contains("100% covered"),
        "unreferenced unit lines must not claim 100% covered:\n{stdout}"
    );
}
