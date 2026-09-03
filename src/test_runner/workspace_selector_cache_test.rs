use super::{
    load_cached_python_workspace_selectors, load_cached_rust_workspace_selectors,
    load_cached_workspace_selectors, store_python_workspace_selectors,
    store_rust_workspace_selectors, store_workspace_selectors,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn store_workspace_selectors_fails_closed_when_writes_fail() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();

    fs::write(root.join(".kiss"), "not-a-directory").unwrap();
    fs::write(root.join("target"), "not-a-directory").unwrap();
    assert!(
        store_workspace_selectors(root, &[], &["tests/test_a.py::test_a".into()], &[], &[])
            .is_none(),
        "unwritable cache parents must not report a successful fingerprint"
    );
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_none());
}

#[test]
fn workspace_selector_cache_round_trips_then_misses_on_touch() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    let py = root.join("tests").join("test_a.py");
    fs::write(&py, "def test_a():\n    assert True\n").unwrap();
    let rs = root.join("lib.rs");
    fs::write(&rs, "#[test]\nfn t() {}\n").unwrap();

    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );
    assert!(
        root.join(".kiss")
            .join("python_test_selectors.json")
            .is_file()
    );
    assert!(
        root.join(".kiss")
            .join("rust_test_selectors.json")
            .is_file()
    );
    assert!(
        root.join(".kiss")
            .join("selector_source_digests.json")
            .is_file(),
        "per-file digest records must persist"
    );
    let hit = load_cached_workspace_selectors(root, &[], &[]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
    assert_eq!(hit.1, vec!["t".to_string()]);
    assert!(
        load_cached_workspace_selectors(root, &[], &["-q".into()]).is_none(),
        "python collection args must be part of the cache key"
    );
    fs::write(root.join("pytest.ini"), "[pytest]\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &[], &[]).is_none(),
        "pytest.ini must invalidate python discovery"
    );
    assert_eq!(
        load_cached_rust_workspace_selectors(root, &[]).as_deref(),
        Some(["t".to_string()].as_slice()),
        "python collection config must not drop rust selectors"
    );
    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );

    fs::write(&py, "def test_a():\n    assert True\n# touch\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_none());
    assert_eq!(
        load_cached_rust_workspace_selectors(root, &[]).as_deref(),
        Some(["t".to_string()].as_slice()),
        "python content change must not drop rust selectors"
    );

    let same_len_before = fs::read(&rs).unwrap();
    let mut same_len_after = same_len_before.clone();
    if let Some(pos) = same_len_after.iter().position(|&b| b == b't') {
        same_len_after[pos] ^= 1;
    }
    assert_eq!(same_len_before.len(), same_len_after.len());
    fs::write(&rs, same_len_after).unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_rust_workspace_selectors(root, &[]).is_none(),
        "rust content change must miss rust selectors"
    );
}

#[test]
fn literal_include_dependency_invalidates_rust_selector_cache() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join("lib.rs"), "include!(\"tests.inc\");\n").unwrap();
    fs::write(root.join("tests.inc"), "#[test]\nfn generated_a() {}\n").unwrap();
    store_rust_workspace_selectors(root, &[], &["generated_a".into()]);
    assert!(load_cached_rust_workspace_selectors(root, &[]).is_some());

    fs::write(root.join("tests.inc"), "#[test]\nfn generated_b() {}\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_rust_workspace_selectors(root, &[]).is_none(),
        "literal include contents participate in Rust selector identity"
    );
}

#[test]
fn manifest_dir_concat_include_invalidates_rust_selector_cache() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        root.join("lib.rs"),
        "include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/tests.inc\"));\n",
    )
    .unwrap();
    fs::write(root.join("tests.inc"), "#[test]\nfn generated_a() {}\n").unwrap();
    store_rust_workspace_selectors(root, &[], &["generated_a".into()]);
    assert!(load_cached_rust_workspace_selectors(root, &[]).is_some());

    fs::write(root.join("tests.inc"), "#[test]\nfn generated_b() {}\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_rust_workspace_selectors(root, &[]).is_none(),
        "CARGO_MANIFEST_DIR concat include contents participate in selector identity"
    );
}

#[test]
fn python_selector_cache_survives_rust_source_change() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    let rs = root.join("lib.rs");
    fs::write(&rs, "#[test]\nfn t() {}\n").unwrap();
    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );
    fs::write(&rs, "#[test]\nfn renamed() {}\n").unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert_eq!(
        load_cached_python_workspace_selectors(root, &[], &[]).as_deref(),
        Some(["tests/test_a.py::test_a".to_string()].as_slice()),
        "rust source change must not rediscover python selectors"
    );
    assert!(
        load_cached_rust_workspace_selectors(root, &[]).is_none(),
        "renamed rust test must miss rust selectors"
    );
}

#[test]
fn cargo_manifest_change_invalidates_rust_selector_cache() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='before'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    assert!(store_rust_workspace_selectors(root, &[], &["t".into()]));
    assert_eq!(
        load_cached_rust_workspace_selectors(root, &[]).as_deref(),
        Some(["t".to_string()].as_slice())
    );

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='after'\nversion='0.1.0'\n",
    )
    .unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_rust_workspace_selectors(root, &[]).is_none(),
        "Cargo metadata changes must rediscover Rust selectors"
    );
}

#[test]
fn cargo_discovery_inputs_invalidate_rust_selector_cache() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    assert!(
        kiss::scrubbed_git_command(root)
            .arg("init")
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    for (relative, before, after) in [
        ("Cargo.lock", "# before\n", "# after\n"),
        (".cargo/config.toml", "[build]\n", "[term]\n"),
        (
            "rust-toolchain.toml",
            "[toolchain]\n",
            "[toolchain]\nchannel='stable'\n",
        ),
        (
            "build.rs",
            "fn main() { println!(\"a\"); }\n",
            "fn main() { println!(\"b\"); }\n",
        ),
    ] {
        fs::write(root.join(relative), before).unwrap();
        assert!(store_rust_workspace_selectors(root, &[], &["t".into()]));
        fs::write(root.join(relative), after).unwrap();
        super::clear_rust_selector_memo_for_tests();
        assert!(
            load_cached_rust_workspace_selectors(root, &[]).is_none(),
            "{relative} changes must invalidate Rust selector discovery"
        );
    }
}

#[test]
fn load_workspace_selectors_for_count_requires_matching_ignore() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    store_workspace_selectors(root, &[], &["tests/test_a.py::test_a".into()], &[], &[]);
    assert!(
        super::load_workspace_selectors_for_count(
            root,
            &["tests/slow".into()],
            &[],
            super::SelectorCountNeed {
                python: true,
                rust: true,
            },
        )
        .is_none()
    );
    assert!(
        super::load_workspace_selectors_for_count(
            root,
            &[],
            &[],
            super::SelectorCountNeed {
                python: true,
                rust: true,
            },
        )
        .is_some()
    );
    fs::remove_file(super::cache_path(root, super::RUST_CACHE_FILE)).unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        super::load_workspace_selectors_for_count(
            root,
            &[],
            &[],
            super::SelectorCountNeed {
                python: true,
                rust: false,
            },
        )
        .is_some(),
        "single-language counts must not require the other cache"
    );
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_renamed():\n    assert True\n",
    )
    .unwrap();
    assert!(
        super::load_workspace_selectors_for_count(
            root,
            &[],
            &[],
            super::SelectorCountNeed {
                python: true,
                rust: true,
            },
        )
        .is_none(),
        "count cache must miss after selector source changes"
    );
}

#[test]
fn python_cache_misses_after_workspace_inventory_changes() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    assert!(store_python_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &[],
    ));
    fs::write(root.join("tests").join("extra.py"), "x = 1\n").unwrap();
    assert!(
        load_cached_python_workspace_selectors(root, &[], &[]).is_none(),
        "full fingerprint must miss after an extra file"
    );
}

#[test]
fn python_selectors_for_rel_path_keeps_only_that_file() {
    let selectors = [
        "tests/test_a.py::test_a".to_string(),
        "tests/test_a.py::test_b".to_string(),
        "tests/test_c.py::test_c".to_string(),
    ];
    assert_eq!(
        super::python_selectors_for_rel_path(&selectors, "tests/test_a.py"),
        vec![
            "tests/test_a.py::test_a".to_string(),
            "tests/test_a.py::test_b".to_string()
        ]
    );
    assert!(super::python_selectors_for_rel_path(&selectors, "tests/missing.py").is_empty());
}

#[test]
fn python_selector_cache_hits_without_rust_cache() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    assert!(
        load_cached_workspace_selectors(root, &[], &[]).is_none(),
        "combined cache must miss when rust is absent"
    );
    assert!(store_python_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &[],
    ));
    assert_eq!(
        load_cached_python_workspace_selectors(root, &[], &[]).as_deref(),
        Some(["tests/test_a.py::test_a".to_string()].as_slice())
    );
    let python_only = super::load_cached_workspace_selectors_for_lang(
        root,
        &[],
        &[],
        Some(kiss::Language::Python),
    )
    .unwrap();
    assert_eq!(python_only.0, vec!["tests/test_a.py::test_a".to_string()]);
    assert!(python_only.1.is_empty());
    assert!(
        load_cached_workspace_selectors(root, &[], &[]).is_none(),
        "combined load still requires rust"
    );
    assert!(
        load_cached_python_workspace_selectors(root, &[], &["-q".into()]).is_none(),
        "python extra args must miss the language cache"
    );
}

#[test]
fn python_collection_args_are_part_of_cache_key() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &[],
        &["-q".into()],
    );
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_none());
    let hit = load_cached_workspace_selectors(root, &[], &["-q".into()]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
}

#[test]
fn python_identity_key_separates_ignore_from_collection_args() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests/test_a.py"), "def test_a():\n    pass\n").unwrap();
    assert!(super::store_python_workspace_selectors(
        root,
        &["x".into()],
        &["from-ignore".into()],
        &[],
    ));
    assert!(super::store_python_workspace_selectors(
        root,
        &[],
        &["from-args".into()],
        &["x".into()],
    ));
    assert_eq!(
        super::load_cached_python_workspace_selectors(root, &["x".into()], &[]),
        Some(vec!["from-ignore".into()])
    );
    assert_eq!(
        super::load_cached_python_workspace_selectors(root, &[], &["x".into()]),
        Some(vec!["from-args".into()])
    );
}

#[test]
fn store_rust_selectors_persists_multiple_ignore_identities() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    fs::write(root.join("lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    store_workspace_selectors(
        root,
        &["src/main.rs".into()],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );
    store_rust_workspace_selectors(root, &[], &["other".into()]);
    let rust_only =
        super::load_cached_workspace_selectors_for_lang(root, &[], &[], Some(kiss::Language::Rust))
            .unwrap();
    assert_eq!(rust_only.1, vec!["other".to_string()]);
    let disk = super::read_language_cache(root, super::RUST_CACHE_FILE).unwrap();
    assert_eq!(rust_only.2, disk.files_fingerprint);
    super::clear_rust_selector_memo_for_tests();
    let hit = load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).unwrap();
    assert_eq!(hit.0, vec!["tests/test_a.py::test_a".to_string()]);
    assert_eq!(hit.1, vec!["t".to_string()]);
    super::clear_rust_selector_memo_for_tests();
    fs::write(root.join("lib.rs"), "#[test]\nfn t() { let _ = 1; }\n").unwrap();
    let body_only = load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).unwrap();
    assert_eq!(body_only.1, vec!["t".to_string()]);
    fs::write(
        root.join("lib.rs"),
        "#[test]\nfn t() { assert_eq!(1, 2); }\n",
    )
    .unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).is_some(),
        "expression macros inside an existing function cannot change the selector universe"
    );
    fs::write(
        root.join("lib.rs"),
        "#[test]\nfn t() { assert_eq!(1, 2); }\nexternal_test_macro!(\n    case_a,\n    case_b,\n);\n",
    )
    .unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).is_none(),
        "an imported macro invocation can generate tests and must miss the selector cache"
    );
    store_rust_workspace_selectors(root, &["src/main.rs".into()], &["t".into()]);
    fs::write(
        root.join("lib.rs"),
        "#[test]\nfn t() { assert_eq!(1, 2); }\nexternal_test_macro!(\n    case_a,\n    case_c,\n);\n",
    )
    .unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).is_none(),
        "multiline item-macro arguments can generate selectors and must be fingerprinted"
    );
    fs::write(
        root.join("lib.rs"),
        "#[test]\nfn t() {}\n#[test]\nfn u() {}\n",
    )
    .unwrap();
    super::clear_rust_selector_memo_for_tests();
    assert!(
        load_cached_workspace_selectors(root, &["src/main.rs".into()], &[]).is_none(),
        "added #[test] must miss rust selector cache"
    );
}

#[test]
fn invocation_session_reuses_one_validated_workspace_fingerprint() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests/test_a.py"), "def test_a():\n    pass\n").unwrap();
    fs::write(root.join("lib.rs"), "#[test]\nfn t() {}\n").unwrap();
    store_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &["t".into()],
        &[],
    );
    let _session = super::begin_inventory_session(root);
    super::reset_workspace_fingerprint_computation_count(root);
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_some());
    assert!(super::load_cached_rust_workspace_selectors(root, &[]).is_some());
    assert!(super::load_cached_python_workspace_selectors(root, &[], &[]).is_some());
    assert_eq!(super::workspace_fingerprint_computation_count(), 1);
}

#[test]
fn invocation_session_keeps_structural_selector_miss_authoritative() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests/test_a.py"), "def test_a():\n    pass\n").unwrap();
    assert!(super::store_python_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &[],
    ));
    fs::write(root.join("tests/test_b.py"), "def test_b():\n    pass\n").unwrap();
    let _session = super::begin_inventory_session(root);
    assert!(super::load_cached_python_workspace_selectors(root, &[], &[]).is_none());
    assert!(super::load_cached_python_workspace_selectors(root, &[], &[]).is_none());
}

#[test]
fn invocation_session_is_an_explicit_snapshot_boundary() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests/test_a.py"), "def test_a():\n    pass\n").unwrap();
    assert!(store_python_workspace_selectors(
        root,
        &[],
        &["tests/test_a.py::test_a".into()],
        &[],
    ));
    {
        let _session = super::begin_inventory_session(root);
        assert!(load_cached_python_workspace_selectors(root, &[], &[]).is_some());
        fs::write(root.join("tests/test_b.py"), "def test_b():\n    pass\n").unwrap();
        assert!(
            load_cached_python_workspace_selectors(root, &[], &[]).is_some(),
            "one planning pipeline uses one immutable inventory snapshot"
        );
    }
    assert!(
        load_cached_python_workspace_selectors(root, &[], &[]).is_none(),
        "the next pipeline invocation must observe structural changes"
    );
}

#[test]
fn invocation_session_shares_inventory_with_rust_report_id_fingerprint() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    assert!(
        kiss::scrubbed_git_command(root)
            .arg("init")
            .status()
            .unwrap()
            .success()
    );
    fs::write(root.join("lib.rs"), "#[test]\nfn a() {}\n").unwrap();
    let _session = super::begin_inventory_session(root);
    let selector_fingerprint = super::workspace_lang_fingerprints(root, &[]).unwrap().rust;
    fs::write(root.join("added.rs"), "#[test]\nfn b() {}\n").unwrap();
    let report_id_fingerprint =
        super::rust_selector_inputs_fingerprint_for_cache(root, &[]).unwrap();
    assert_eq!(
        report_id_fingerprint, selector_fingerprint,
        "one planning pipeline must use one Rust source inventory"
    );
}

#[test]
fn git_fingerprint_includes_untracked_sources() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let init = kiss::scrubbed_git_command(root)
        .args(["init"])
        .output()
        .unwrap();
    assert!(init.status.success(), "git init failed");
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let tracked = root.join("src").join("lib.rs");
    fs::write(&tracked, "pub fn a() {}\n").unwrap();
    let add = kiss::scrubbed_git_command(root)
        .args(["add", "src/lib.rs", ".gitignore"])
        .output()
        .unwrap();
    assert!(add.status.success(), "git add failed");
    let commit = kiss::scrubbed_git_command(root)
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-m",
            "t",
        ])
        .output()
        .unwrap();
    assert!(commit.status.success(), "git commit failed");

    store_workspace_selectors(root, &[], &[], &["a".into()], &[]);
    assert!(load_cached_workspace_selectors(root, &[], &[]).is_some());

    fs::write(root.join("src").join("extra.rs"), "pub fn b() {}\n").unwrap();
    assert!(
        load_cached_workspace_selectors(root, &[], &[]).is_none(),
        "untracked .rs must miss selector cache"
    );
}
