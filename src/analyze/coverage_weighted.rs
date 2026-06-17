use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) fn merge_weighted_file_pcts(
    py_cov: &kiss::TestRefAnalysis,
    py_parsed: &[kiss::ParsedFile],
    rs_cov: &kiss::RustTestRefAnalysis,
    rs_parsed: &[kiss::ParsedRustFile],
) -> HashMap<PathBuf, usize> {
    merge_weighted_file_pcts_impl(py_cov, py_parsed, rs_cov, rs_parsed, true)
}

pub(crate) fn merge_weighted_file_pcts_for_runtime_py(
    py_cov: &kiss::TestRefAnalysis,
    py_parsed: &[kiss::ParsedFile],
    rs_cov: &kiss::RustTestRefAnalysis,
    rs_parsed: &[kiss::ParsedRustFile],
) -> HashMap<PathBuf, usize> {
    merge_weighted_file_pcts_impl(py_cov, py_parsed, rs_cov, rs_parsed, false)
}

fn merge_weighted_file_pcts_impl(
    py_cov: &kiss::TestRefAnalysis,
    py_parsed: &[kiss::ParsedFile],
    rs_cov: &kiss::RustTestRefAnalysis,
    rs_parsed: &[kiss::ParsedRustFile],
    include_py_weighted: bool,
) -> HashMap<PathBuf, usize> {
    let py_refs: Vec<&kiss::ParsedFile> = py_parsed.iter().collect();
    let mut weighted = if include_py_weighted {
        kiss::test_refs::compute_py_weighted_file_pcts(py_cov, &py_refs)
    } else {
        HashMap::new()
    };
    let rs_refs: Vec<&kiss::ParsedRustFile> = rs_parsed.iter().collect();
    weighted.extend(kiss::rust_test_refs::compute_rs_weighted_file_pcts(
        rs_cov, &rs_refs,
    ));
    if include_py_weighted {
        for parsed in py_parsed {
            if kiss::test_refs::is_test_file(&parsed.path)
                || kiss::test_refs::is_in_test_directory(&parsed.path)
            {
                weighted.insert(parsed.path.clone(), 0);
            } else if !weighted.contains_key(&parsed.path)
                && !py_cov.definitions.iter().any(|d| d.file == parsed.path)
            {
                let pct = if parsed.path.file_name().and_then(|s| s.to_str()) == Some("__init__.py")
                {
                    kiss::test_refs::py_init_marker_pct(parsed)
                } else {
                    0
                };
                weighted.insert(parsed.path.clone(), pct);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn parsed_py(path: PathBuf, source: &str) -> kiss::ParsedFile {
        let mut parser = kiss::create_parser().unwrap();
        let tree = parser.parse(source, None).unwrap();
        kiss::ParsedFile {
            path,
            source: source.to_string(),
            tree,
        }
    }

    fn parsed_rs(path: PathBuf, source: &str) -> kiss::ParsedRustFile {
        kiss::ParsedRustFile {
            path,
            source: source.to_string(),
            ast: syn::parse_file(source).unwrap(),
        }
    }

    fn empty_py_cov() -> kiss::TestRefAnalysis {
        kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        }
    }

    fn empty_rs_cov() -> kiss::RustTestRefAnalysis {
        kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            propagated_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        }
    }

    #[test]
    fn runtime_py_merge_does_not_invent_static_python_file_coverage() {
        let py_parsed = vec![
            parsed_py(PathBuf::from("/repo/pkg/__init__.py"), ""),
            parsed_py(
                PathBuf::from("/repo/tests/test_pkg.py"),
                "def test_pkg():\n    pass\n",
            ),
        ];
        let rs_parsed = vec![
            parsed_rs(PathBuf::from("/repo/src/main.rs"), "fn main() {}\n"),
            parsed_rs(
                PathBuf::from("/repo/tests/runtime.rs"),
                "#[test]\nfn covers_runtime() {}\n",
            ),
        ];

        let weighted = merge_weighted_file_pcts_for_runtime_py(
            &empty_py_cov(),
            &py_parsed,
            &empty_rs_cov(),
            &rs_parsed,
        );

        assert!(
            !weighted.contains_key(&PathBuf::from("/repo/pkg/__init__.py")),
            "runtime Python coverage must come from rslip, not static Python defaults"
        );
        assert!(
            !weighted.contains_key(&PathBuf::from("/repo/tests/test_pkg.py")),
            "runtime Python coverage must not synthesize Python test-file coverage"
        );
        assert_eq!(weighted.get(&PathBuf::from("/repo/src/main.rs")), Some(&0));
        assert_eq!(
            weighted.get(&PathBuf::from("/repo/tests/runtime.rs")),
            Some(&0)
        );
    }

    #[test]
    fn static_py_merge_records_python_scaffolding_and_rust_entry_points() {
        let py_parsed = vec![
            parsed_py(PathBuf::from("/repo/pkg/__init__.py"), ""),
            parsed_py(
                PathBuf::from("/repo/tests/test_pkg.py"),
                "def test_pkg():\n    pass\n",
            ),
        ];
        let rs_parsed = vec![parsed_rs(
            PathBuf::from("/repo/src/main.rs"),
            "fn main() {}\n",
        )];

        let weighted =
            merge_weighted_file_pcts(&empty_py_cov(), &py_parsed, &empty_rs_cov(), &rs_parsed);

        assert!(weighted.contains_key(&PathBuf::from("/repo/pkg/__init__.py")));
        assert_eq!(
            weighted.get(&PathBuf::from("/repo/tests/test_pkg.py")),
            Some(&0)
        );
        assert_eq!(weighted.get(&PathBuf::from("/repo/src/main.rs")), Some(&0));
    }
}
