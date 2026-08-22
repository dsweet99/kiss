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
    assert!(kiss::is_python_test_module_path(std::path::Path::new(
        "tests/test_x.py"
    )));
    assert!(kiss::is_binary_entry_point(std::path::Path::new(
        "src/main.rs"
    )));
}

#[test]
fn product_consumers_do_not_call_path_naming_test_predicates() {
    let forbidden = [
        "is_rust_test_file(",
        "is_coverage_gate_file(",
        "is_test_file(",
    ];
    let mut hits = Vec::new();
    for path in workspace_src_files() {
        let text = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            if contains_predicate_call(&text, needle) {
                hits.push(format!("{}: {needle}", path.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "product consumers must not call path-naming test predicates\n{}",
        hits.join("\n")
    );
}

fn contains_predicate_call(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(idx, _)| {
        idx == 0 || {
            let prev = text.as_bytes()[idx - 1];
            !prev.is_ascii_alphanumeric() && prev != b'_'
        }
    })
}
