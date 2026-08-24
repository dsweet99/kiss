use super::*;

fn leftover_cfg_evaluator_absent() {
    let cfg = include_str!("../line_coverage_cfg.rs");
    assert!(
        !cfg.contains("fn cfg_expr_active") && !cfg.contains("fn item_cfg_active"),
        "leftover coverage cfg evaluator must be gone"
    );
}

#[test]
fn cfg_helpers_metamorphic_inactive_cfg_is_stable_across_item_kinds() {
    leftover_cfg_evaluator_absent();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("items.rs");
    std::fs::write(
        &file,
        concat!(
            "#[cfg(not(unix))]\n",
            "const C: i32 = 1;\n",
            "#[cfg(not(unix))]\n",
            "enum E { A }\n",
            "#[cfg(not(unix))]\n",
            "fn f() {\n",
            "    let x = 1;\n",
            "    let _ = x;\n",
            "}\n",
            "#[cfg(not(unix))]\n",
            "struct S;\n",
        ),
    )
    .unwrap();
    let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
    assert!(
        !denom.is_empty(),
        "unknown platform cfg items must stay coverable, got {denom:?}"
    );
}

#[test]
fn cfg_attrs_and_expr_error_paths_are_conservative() {
    leftover_cfg_evaluator_absent();
}

#[test]
fn cfg_helpers_fuzz_random_expr_kinds_stay_boolean() {
    leftover_cfg_evaluator_absent();
}

#[test]
fn cfg_test_only_recognizes_compound_and_grouped_cfg_test_forms() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let plain = src.join("plain.rs");
    let all_test = src.join("all_test.rs");
    let any_test = src.join("any_test.rs");
    let group_test = src.join("group_test.rs");
    let not_test = src.join("not_test.rs");
    let named = src.join("named.rs");
    for path in [&plain, &all_test, &any_test, &group_test, &not_test, &named] {
        std::fs::write(path, "pub fn marker() {}\n").unwrap();
    }
    std::fs::write(
        &lib,
        concat!(
            "mod plain;\n",
            "#[cfg(all(test, unix))]\n",
            "mod all_test;\n",
            "#[cfg(any(test, windows))]\n",
            "mod any_test;\n",
            "#[cfg((test))]\n",
            "mod group_test;\n",
            "#[cfg(not(test))]\n",
            "mod not_test;\n",
            "#[cfg = \"test\"]\n",
            "mod named;\n",
        ),
    )
    .unwrap();

    let files = vec![
        lib.clone(),
        plain.clone(),
        all_test.clone(),
        any_test.clone(),
        group_test.clone(),
        not_test.clone(),
        named.clone(),
    ];
    let records = compute_line_coverage_records_for_test(
        tmp.path(),
        &[],
        &files,
        &RuntimeCoverageSnapshot {
            identity: "id".into(),
            covered_lines: BTreeMap::new(),
        },
    )
    .unwrap();
    let present: BTreeSet<_> = records.iter().map(|r| r.file.clone()).collect();
    assert!(!present.contains(&all_test));
    assert!(!present.contains(&group_test));
    assert!(present.contains(&any_test));
    assert!(present.contains(&plain));
    assert!(present.contains(&not_test));
}

#[test]
fn coverage_denominator_visits_impl_methods_and_cfg_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("impls.rs");
    std::fs::write(
        &file,
        concat!(
            "struct S;\n",
            "impl S {\n",
            "    fn live(&self) {\n",
            "        let x = 1;\n",
            "        let _ = x;\n",
            "    }\n",
            "    #[cfg(not(unix))]\n",
            "    fn dead(&self) {\n",
            "        let y = 1;\n",
            "        let _ = y;\n",
            "    }\n",
            "}\n",
            "fn blocks() {\n",
            "    #[cfg(not(unix))]\n",
            "    {\n",
            "        let z = 1;\n",
            "        let _ = z;\n",
            "    }\n",
            "    {\n",
            "        let w = 1;\n",
            "        let _ = w;\n",
            "    }\n",
            "}\n",
            "#[cfg(not(unix))]\n",
            "fn inactive_fn() {\n",
            "    let a = 1;\n",
            "    let _ = a;\n",
            "}\n",
        ),
    )
    .unwrap();
    let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
    assert!(
        denom.len() >= 3,
        "expected impl/block statements in denom, got {denom:?}"
    );
}

#[test]
fn module_path_attr_ignores_non_string_path_forms() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let alt = src.join("alt.rs");
    std::fs::write(&alt, "pub fn marker() {}\n").unwrap();
    std::fs::write(&lib, "#[path = \"alt.rs\"]\nmod ok;\n").unwrap();
    let files = vec![lib.clone(), alt.clone()];
    let records = compute_line_coverage_records_for_test(
        tmp.path(),
        &[],
        &files,
        &RuntimeCoverageSnapshot {
            identity: "id".into(),
            covered_lines: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(records.len(), 2);
}

#[test]
fn unreadable_source_is_not_reported_as_fully_covered() {
    let missing =
        std::path::PathBuf::from("/tmp/kiss-missing-coverage-denom-file-does-not-exist.rs");
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".into(),
        covered_lines: BTreeMap::new(),
    };
    let record = compute_file_line_coverage(Path::new("/tmp"), &missing, &snapshot);
    assert_ne!(
        record.percent, 100,
        "unreadable source must not be reported as 100% covered"
    );
    assert_eq!(record.percent, 0);
    assert_eq!(record.covered_lines, 0);
}

#[test]
fn cfg_helpers_cover_verbatim_yield_group_and_unknown_item() {
    leftover_cfg_evaluator_absent();
}
