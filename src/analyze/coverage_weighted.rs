use std::collections::HashMap;
use std::path::PathBuf;

use kiss::check_universe_cache::CachedCoverageItem;

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
        }
    }
    for parsed in rs_parsed {
        if kiss::rust_test_refs::is_rust_test_file(&parsed.path) {
            weighted.insert(parsed.path.clone(), 0);
        }
    }
    weighted
}

pub(crate) fn inject_test_file_sentinels(
    definitions: &mut Vec<CachedCoverageItem>,
    unreferenced: &mut Vec<CachedCoverageItem>,
    py_parsed: &[kiss::ParsedFile],
    rs_parsed: &[kiss::ParsedRustFile],
    weighted: &HashMap<PathBuf, usize>,
) {
    for parsed in py_parsed {
        if !(kiss::test_refs::is_test_file(&parsed.path)
            || kiss::test_refs::is_in_test_directory(&parsed.path))
        {
            continue;
        }
        if weighted.get(&parsed.path).copied() != Some(0) {
            continue;
        }
        let file_str = parsed.path.to_string_lossy().to_string();
        if definitions.iter().any(|d| d.file == file_str) {
            continue;
        }
        let item = CachedCoverageItem {
            file: file_str,
            name: "__test_file__".into(),
            line: 1,
        };
        definitions.push(item.clone());
        unreferenced.push(item);
    }
    for parsed in rs_parsed {
        if !kiss::rust_test_refs::is_rust_test_file(&parsed.path) {
            continue;
        }
        if weighted.get(&parsed.path).copied() != Some(0) {
            continue;
        }
        let file_str = parsed.path.to_string_lossy().to_string();
        if definitions.iter().any(|d| d.file == file_str) {
            continue;
        }
        let item = CachedCoverageItem {
            file: file_str,
            name: "__test_file__".into(),
            line: 1,
        };
        definitions.push(item.clone());
        unreferenced.push(item);
    }
}

pub(crate) fn inject_binary_entry_sentinels(
    definitions: &mut Vec<CachedCoverageItem>,
    unreferenced: &mut Vec<CachedCoverageItem>,
    rs_files: &[PathBuf],
) {
    for path in rs_files {
        if !kiss::rust_test_refs::is_binary_entry_point(path) {
            continue;
        }
        let file_str = path.to_string_lossy().to_string();
        if definitions.iter().any(|d| d.file == file_str) {
            continue;
        }
        let item = CachedCoverageItem {
            file: file_str,
            name: "__entry_point__".into(),
            line: 1,
        };
        definitions.push(item.clone());
        unreferenced.push(item);
    }
}
