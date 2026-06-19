use super::*;

fn parsed_rs_source(path: PathBuf, source: &str) -> ParsedRustFile {
    ParsedRustFile {
        path,
        source: source.to_string(),
        ast: syn::parse_file(source).unwrap(),
    }
}

#[test]
fn analysis_from_line_coverage_ignores_mod_only_files_without_runtime_coverage() {
    let parsed = vec![parsed_rs_source(
        PathBuf::from("src/lib.rs"),
        "mod cache;\npub use cache::value;\n",
    )];

    let analysis = analysis_from_line_coverage(&parsed, &[]);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn analysis_from_line_coverage_skips_external_cfg_test_modules() {
    let parsed = vec![
        parsed_rs_source(
            PathBuf::from("/repo/src/lib.rs"),
            "#[cfg(test)] mod helper_tests;\npub fn product() {}\n",
        ),
        parsed_rs_source(
            PathBuf::from("/repo/src/helper_tests.rs"),
            "pub fn fixture_helper() {}\n",
        ),
    ];
    let coverage = vec![RustLineCoverage {
        file: PathBuf::from("src/helper_tests.rs"),
        executable_lines: vec![1],
        missing_lines: vec![1],
    }];

    let analysis = analysis_from_line_coverage(&parsed, &coverage);

    assert!(
        analysis
            .unreferenced
            .iter()
            .all(|def| def.file != Path::new("/repo/src/helper_tests.rs"))
    );
    assert!(
        analysis
            .unreferenced
            .iter()
            .any(|def| def.file == Path::new("/repo/src/lib.rs"))
    );
}

#[test]
fn analysis_from_line_coverage_keeps_test_like_product_modules() {
    let parsed = vec![
        parsed_rs_source(PathBuf::from("/repo/src/lib.rs"), "mod cache_tests;\n"),
        parsed_rs_source(
            PathBuf::from("/repo/src/cache_tests.rs"),
            "pub fn cache_product() {}\n",
        ),
    ];
    let coverage = vec![RustLineCoverage {
        file: PathBuf::from("src/cache_tests.rs"),
        executable_lines: vec![1],
        missing_lines: vec![1],
    }];

    let analysis = analysis_from_line_coverage(&parsed, &coverage);

    assert!(
        analysis
            .unreferenced
            .iter()
            .any(|def| def.file == Path::new("/repo/src/cache_tests.rs"))
    );
}
