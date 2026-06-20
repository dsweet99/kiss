use super::{is_binary_entry_point, RustTestRefAnalysis};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[test]
fn witness_rust_test_ref_analysis() {
    let _ = RustTestRefAnalysis {
        definitions: vec![],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        propagated_references: HashSet::new(),
        unreferenced: vec![],
        coverage_map: HashMap::new(),
    };
    assert!(!is_binary_entry_point(Path::new("tests/integration.rs")));
    assert!(is_binary_entry_point(Path::new("src/main.rs")));
}
