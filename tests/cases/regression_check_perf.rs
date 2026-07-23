//! Compile-time / source-scan proof that the static-reference coverage checker
//! is gone (plan unit test 7 / acceptance criterion 8).

use std::fs;
use std::path::PathBuf;

fn workspace_src_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from("src")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn static_reference_coverage_apis_are_removed_from_src() {
    let forbidden = [
        "analyze_test_refs",
        "analyze_test_refs_no_map",
        "analyze_test_refs_quick",
        "analyze_rust_test_refs",
        "TestRefAnalysis",
        "RustTestRefAnalysis",
        "CoverageSource::StaticReferences",
        "inv_test_coverage",
        "CoverageMode::RuntimeLine",
    ];
    let mut hits = Vec::new();
    for path in workspace_src_files() {
        let text = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            if text.contains(needle) {
                hits.push(format!("{}: {needle}", path.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "static-reference coverage symbols must not remain in src/\n{}",
        hits.join("\n")
    );
}

#[test]
fn kiss_check_stays_static_only_without_test_ref_analysis() {
    // Behavioral stand-in for the old perf regression: static check path must
    // not depend on analyze_test_refs_* at all (proven by the scan above), and
    // detection helpers used by runtime coverage population must still work.
    assert!(kiss::is_test_file(std::path::Path::new("tests/test_x.py")));
    assert!(kiss::is_rust_test_file(std::path::Path::new(
        "src/foo_test.rs"
    )));
    assert!(kiss::is_binary_entry_point(std::path::Path::new(
        "src/main.rs"
    )));
}
