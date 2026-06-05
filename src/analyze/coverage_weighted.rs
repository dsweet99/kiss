use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) fn merge_weighted_file_pcts(
    py_cov: &kiss::TestRefAnalysis,
    py_parsed: &[kiss::ParsedFile],
    rs_cov: &kiss::RustTestRefAnalysis,
    rs_parsed: &[kiss::ParsedRustFile],
) -> HashMap<PathBuf, usize> {
    let py_refs: Vec<&kiss::ParsedFile> = py_parsed.iter().collect();
    let mut weighted = kiss::test_refs::compute_py_weighted_file_pcts(py_cov, &py_refs);
    let rs_refs: Vec<&kiss::ParsedRustFile> = rs_parsed.iter().collect();
    weighted.extend(kiss::rust_test_refs::compute_rs_weighted_file_pcts(
        rs_cov, &rs_refs,
    ));
    for parsed in py_parsed {
        if kiss::test_refs::is_test_file(&parsed.path)
            || kiss::test_refs::is_in_test_directory(&parsed.path)
        {
            weighted.insert(parsed.path.clone(), 0);
        } else if !weighted.contains_key(&parsed.path)
            && !py_cov
                .definitions
                .iter()
                .any(|d| d.file == parsed.path)
        {
            let pct = if parsed.path.file_name().and_then(|s| s.to_str()) == Some("__init__.py") {
                kiss::test_refs::py_init_marker_pct(parsed)
            } else {
                0
            };
            weighted.insert(parsed.path.clone(), pct);
        }
    }
    for parsed in rs_parsed {
        if kiss::rust_test_refs::is_rust_test_file(&parsed.path)
            || kiss::rust_test_refs::is_binary_entry_point(&parsed.path)
        {
            weighted.insert(parsed.path.clone(), 0);
        }
    }
    weighted
}
