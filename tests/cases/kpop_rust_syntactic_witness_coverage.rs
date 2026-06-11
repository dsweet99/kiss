//! Regression test: kiss marks production symbols covered when tests contain
//! syntactic witnesses that never run at runtime.
//!
//! Fixture: `tests/fake_rust/syntactic_witness_lib.rs` — each `*_only` function
//! is referenced only via a non-executing witness (fn-item bind, dead branch,
//! stringify macro, uncalled helper, uninvoked closure). Only `actually_covered`
//! has a real call.
//!
//! Desired behavior (Phase 1 executable-witness mode): every `*_only` symbol
//! should appear in `unreferenced` because no test executes it.
//!
//! Executable-witness gating: only reachable call expressions in `#[test]` bodies
//! count as coverage. Syntactic-only witnesses (stringify, fn binds, dead branches,
//! uninvoked closures) do not.

use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use kiss::rust_test_refs::analyze_rust_test_refs;
use std::collections::HashSet;
use std::path::Path;

const FIXTURE: &str = "tests/fake_rust/syntactic_witness_lib.rs";

fn parse_fixture() -> ParsedRustFile {
    parse_rust_file(Path::new(FIXTURE)).expect("parse fake_rust fixture")
}

fn unreferenced_names<'a>(
    analysis: &'a kiss::RustTestRefAnalysis,
    file: &'a Path,
) -> HashSet<&'a str> {
    analysis
        .unreferenced
        .iter()
        .filter(|d| d.file == file)
        .map(|d| d.name.as_str())
        .collect()
}

/// Locks in correct behavior: a real call in a `#[test]` body marks the
/// symbol referenced.
#[test]
fn kpop_rust_syntactic_witness_actually_covered_is_referenced() {
    let parsed = parse_fixture();
    let path = parsed.path.clone();
    let refs = vec![&parsed];
    let analysis = analyze_rust_test_refs(&refs, None);
    let unref = unreferenced_names(&analysis, &path);
    assert!(!unref.contains("actually_covered"));
    assert!(
        analysis.test_references.contains("actually_covered")
            || analysis.call_references.contains("actually_covered"),
        "real call should mark actually_covered as referenced"
    );
}

/// Regression: syntactic-only witnesses must not satisfy coverage.
#[test]
fn kpop_rust_syntactic_witness_hollow_witnesses_should_be_unreferenced() {
    let parsed = parse_fixture();
    let path = parsed.path.clone();
    let refs = vec![&parsed];
    let analysis = analyze_rust_test_refs(&refs, None);
    let unref = unreferenced_names(&analysis, &path);

    let hollow_only = [
        "fn_value_only",
        "dead_branch_only",
        "stringify_only",
        "uncalled_helper_only",
        "closure_only",
    ];
    for name in hollow_only {
        assert!(
            unref.contains(name),
            "{name} should be unreferenced — witness never runs at runtime.\n\
             unreferenced: {unref:?}\n\
             test_references: {:?}\n\
             call_references: {:?}",
            analysis.test_references,
            analysis.call_references
        );
    }
}
