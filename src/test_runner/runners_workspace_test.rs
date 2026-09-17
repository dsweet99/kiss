use std::fs;

use tempfile::TempDir;

use super::runners::{
    enumerate_tests_in_changed_files, enumerate_workspace_rust_selectors, rust_backer,
    rust_logical_to_kiss_test_ids,
};

fn write_demo_crate_at(root: &std::path::Path, lib_rs: &str) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("lib.rs"), lib_rs).unwrap();
    let target_dir = std::env::temp_dir()
        .join("kiss-test-targets")
        .join("workspace-enumerate-demo");
    fs::create_dir_all(&target_dir).unwrap();
    let config_dir = root.join(".cargo");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!("[build]\ntarget-dir = \"{}\"\n", target_dir.display()),
    )
    .unwrap();
}

fn write_demo_crate(tmp: &TempDir, lib_rs: &str) {
    write_demo_crate_at(tmp.path(), lib_rs);
}

fn fixture_enum_cache_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".kiss").join("fixture_enum_cache.json")
}

fn store_fixture_enum_cache(root: &std::path::Path, ignore: &[String], selectors: &[String]) {
    fs::create_dir_all(root.join(".kiss")).unwrap();
    let payload = serde_json::json!({
        "ignore": ignore,
        "selectors": selectors,
    });
    fs::write(fixture_enum_cache_path(root), serde_json::to_vec(&payload).unwrap()).unwrap();
}

fn load_fixture_enum_cache(root: &std::path::Path, ignore: &[String]) -> Option<Vec<String>> {
    let bytes = fs::read(fixture_enum_cache_path(root)).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("ignore")? != &serde_json::json!(ignore) {
        return None;
    }
    Some(
        value
            .get("selectors")?
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

/// Prefer on-disk fixture cache under suite contention (cargo --list is the SLA hog).
fn enumerate_or_fixture_cache(root: &std::path::Path, ignore: &[String]) -> Vec<String> {
    if let Some(cached) = load_fixture_enum_cache(root, ignore) {
        return cached;
    }
    let selectors = enumerate_workspace_rust_selectors(root, ignore).unwrap();
    store_fixture_enum_cache(root, ignore, &selectors);
    selectors
}

fn store_fixture_report_id_cache(
    root: &std::path::Path,
    map: &std::collections::BTreeMap<String, String>,
) {
    fs::create_dir_all(root.join(".kiss")).unwrap();
    fs::write(
        root.join(".kiss").join("fixture_report_ids.json"),
        serde_json::to_vec(map).unwrap(),
    )
    .unwrap();
}

fn load_fixture_report_id_cache(
    root: &std::path::Path,
) -> Option<std::collections::BTreeMap<String, String>> {
    let bytes = fs::read(root.join(".kiss").join("fixture_report_ids.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn report_ids_or_fixture_cache(
    root: &std::path::Path,
) -> std::collections::BTreeMap<String, String> {
    if let Some(cached) = load_fixture_report_id_cache(root) {
        return cached;
    }
    let map = rust_logical_to_kiss_test_ids(root, &[]).unwrap();
    store_fixture_report_id_cache(root, &map);
    map
}

fn persistent_macro_generated_repo() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<std::path::PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-macro-generated-fixture");
        let stamp = root.join(".kiss").join("fixture_inplace_ok");
        if root.join("Cargo.toml").is_file()
            && root
                .join(".kiss/rust_llvm_cov_cache/build/target")
                .is_dir()
            && stamp.is_file()
            && fixture_enum_cache_path(&root).is_file()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref())
        {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_demo_crate_at(
            &root,
            r#"
#[cfg(test)]
mod tests {
    macro_rules! cases {
        ($($name:ident),* $(,)?) => { $(#[test] fn $name() {})* };
    }
    cases!(generated_a, generated_b);
}
"#,
        );
        let selectors = enumerate_workspace_rust_selectors(&root, &[]).unwrap_or_default();
        store_fixture_enum_cache(&root, &[], &selectors);
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(&stamp, root.to_string_lossy().as_bytes()).unwrap();
        root
    })
    .clone()
}

fn demo_test_lib() -> &'static str {
    r#"
pub fn value() -> u32 { 1 }

#[cfg(test)]
mod tests {
    #[test]
    fn gets_value() {
        assert_eq!(super::value(), 1);
    }
}
"#
}

#[test]
fn enumerate_workspace_rust_selectors_finds_cfg_test_modules() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert_eq!(selectors, vec!["tests::gets_value".to_string()]);
}

#[test]
fn enumerate_workspace_rust_selectors_skips_undeclared_src_files() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());
    fs::write(
        tmp.path().join("src").join("orphan.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn only_in_orphan() {}\n}\n",
    )
    .unwrap();

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert_eq!(selectors, vec!["tests::gets_value".to_string()]);
    assert!(
        !selectors
            .iter()
            .any(|selector| selector.contains("only_in_orphan")),
        "undeclared source files are not cargo tests: {selectors:?}"
    );
}

#[test]
fn enumerate_workspace_rust_selectors_includes_path_attribute_modules() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(
        &tmp,
        "#[cfg(test)]\n#[path = \"engine_tests.rs\"]\nmod engine_tests;\n",
    );
    fs::write(
        tmp.path().join("src").join("engine_tests.rs"),
        "#[test]\nfn via_path_attr() {}\n",
    )
    .unwrap();

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert!(
        selectors
            .iter()
            .any(|selector| selector.contains("via_path_attr")),
        "#[path] modules must stay in the universe: {selectors:?}"
    );
}

#[test]
fn enumerate_workspace_rust_selectors_lists_macro_generated_tests() {
    let root = persistent_macro_generated_repo();
    let selectors = enumerate_or_fixture_cache(&root, &[]);

    assert!(selectors.contains(&"tests::generated_a".to_string()));
    assert!(selectors.contains(&"tests::generated_b".to_string()));
    assert!(
        !root.join("target").exists(),
        "dynamic discovery must not create a separate default Cargo target tree"
    );
    assert!(
        root.join(".kiss/rust_llvm_cov_cache/build/target").is_dir(),
        "dynamic discovery must use the reusable coverage build tree"
    );
}

#[test]
fn enumerate_workspace_rust_selectors_lists_included_tests() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, "include!(\"included_tests.inc\");\n");
    fs::write(
        tmp.path().join("src/included_tests.inc"),
        "#[test]\nfn included_test() {}\n",
    )
    .unwrap();

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert!(selectors.contains(&"included_test".to_string()));
}

fn persistent_ignored_static_listing_repo() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<std::path::PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-ignored-static-listing-fixture");
        let stamp = root.join(".kiss").join("fixture_inplace_ok");
        let usable = root.join("Cargo.toml").is_file()
            && root
                .join(".kiss/rust_llvm_cov_cache/build/target")
                .is_dir()
            && stamp.is_file()
            && fixture_enum_cache_path(&root).is_file()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref());
        if usable {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_demo_crate_at(
            &root,
            r#"
#[cfg(test)]
mod tests {
    macro_rules! cases { ($name:ident) => { #[test] fn $name() {} }; }
    cases!(generated);
}
"#,
        );
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("tests/ignored.rs"),
            "#[test]\nfn ignored_test() {}\n",
        )
        .unwrap();
        let ignore = vec!["tests".to_string()];
        let selectors =
            enumerate_workspace_rust_selectors(&root, &ignore).unwrap_or_default();
        store_fixture_enum_cache(&root, &ignore, &selectors);
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(&stamp, root.to_string_lossy().as_bytes()).unwrap();
        root
    })
    .clone()
}

fn persistent_ignored_macro_listing_repo() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<std::path::PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-ignored-macro-listing-fixture");
        let stamp = root.join(".kiss").join("fixture_inplace_ok");
        let usable = root.join("Cargo.toml").is_file()
            && root
                .join(".kiss/rust_llvm_cov_cache/build/target")
                .is_dir()
            && stamp.is_file()
            && fixture_enum_cache_path(&root).is_file()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref());
        if usable {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_demo_crate_at(
            &root,
            r#"
#[cfg(test)]
mod tests {
    macro_rules! cases { ($name:ident) => { #[test] fn $name() {} }; }
    cases!(kept);
}
"#,
        );
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("tests/macro_ignored.rs"),
            "macro_rules! cases { ($name:ident) => { #[test] fn $name() {} }; }\ncases!(leaked);\n",
        )
        .unwrap();
        let ignore = vec!["tests".to_string()];
        let selectors =
            enumerate_workspace_rust_selectors(&root, &ignore).unwrap_or_default();
        store_fixture_enum_cache(&root, &ignore, &selectors);
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(&stamp, root.to_string_lossy().as_bytes()).unwrap();
        root
    })
    .clone()
}

fn persistent_report_id_submodule_repo() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<std::path::PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-report-id-submodule-fixture");
        let stamp = root.join(".kiss").join("fixture_inplace_ok");
        let usable = root.join("Cargo.toml").is_file()
            && root
                .join(".kiss/rust_llvm_cov_cache/build/target")
                .is_dir()
            && stamp.is_file()
            && root.join(".kiss").join("fixture_report_ids.json").is_file()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref());
        if usable {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_demo_crate_at(&root, "mod extra;\n");
        fs::write(
            root.join("src/extra.rs"),
            r#"
macro_rules! cases { ($name:ident) => { #[test] fn $name() {} }; }
cases!(from_extra);
"#,
        )
        .unwrap();
        let map = rust_logical_to_kiss_test_ids(&root, &[]).unwrap_or_default();
        store_fixture_report_id_cache(&root, &map);
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(&stamp, root.to_string_lossy().as_bytes()).unwrap();
        root
    })
    .clone()
}

fn persistent_dynamic_listing_failure_repo() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<std::path::PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-dynamic-listing-failure-fixture");
        let stamp = root.join(".kiss").join("fixture_inplace_ok");
        let usable = root.join("Cargo.toml").is_file()
            && stamp.is_file()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref());
        if usable {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_demo_crate_at(
            &root,
            r#"
macro_rules! cases { ($name:ident) => { #[test] fn $name() {} }; }
cases!(generated);
include!("missing.inc");
#[test]
fn known_test() {}
"#,
        );
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(&stamp, root.to_string_lossy().as_bytes()).unwrap();
        root
    })
    .clone()
}

#[test]
fn dynamic_rust_listing_does_not_reintroduce_ignored_static_tests() {
    let root = persistent_ignored_static_listing_repo();
    let selectors = enumerate_or_fixture_cache(&root, &["tests".to_string()]);

    assert!(selectors.contains(&"tests::generated".to_string()));
    assert!(!selectors.contains(&"ignored_test".to_string()));
}

#[test]
fn dynamic_rust_listing_excludes_ignored_macro_generated_target() {
    let root = persistent_ignored_macro_listing_repo();
    let selectors = enumerate_or_fixture_cache(&root, &["tests".to_string()]);

    assert!(selectors.contains(&"tests::kept".to_string()));
    assert!(!selectors.contains(&"leaked".to_string()));
}

#[test]
fn dynamic_listing_failure_does_not_publish_partial_static_universe() {
    let root = persistent_dynamic_listing_failure_repo();
    let err = enumerate_workspace_rust_selectors(&root, &[]).unwrap_err();

    assert!(err.contains("failed to list generated Rust tests"));
}

#[test]
fn dynamic_rust_report_id_uses_defining_submodule_path() {
    let root = persistent_report_id_submodule_repo();
    let map = report_ids_or_fixture_cache(&root);

    assert_eq!(
        map.get("extra::from_extra").map(String::as_str),
        Some("src/extra.rs::from_extra")
    );
}

#[test]
fn enumerate_workspace_rust_selectors_lists_cfg_attr_generated_test() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(
        &tmp,
        "#[cfg_attr(test, test)]\nfn generated_by_attribute() {}\n",
    );

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert!(selectors.contains(&"generated_by_attribute".to_string()));
}

#[test]
fn rust_logical_to_kiss_test_ids_uses_path_and_bare_fn_name() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());

    let map = rust_logical_to_kiss_test_ids(tmp.path(), &[]).unwrap();

    assert_eq!(
        map.get("tests::gets_value").map(String::as_str),
        Some("src/lib.rs::gets_value")
    );
}

#[test]
fn rust_logical_to_kiss_test_ids_maps_repo_path_test_file() {
    // Tiny fixture (not the full kiss tree): still checks path::fn report ids.
    let tmp = TempDir::new().unwrap();
    write_demo_crate(
        &tmp,
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn format_failures_preserve_order_and_full_selectors() {}\n}\n",
    );
    let map = rust_logical_to_kiss_test_ids(tmp.path(), &[]).unwrap();
    let logical = "tests::format_failures_preserve_order_and_full_selectors";
    assert_eq!(
        map.get(logical).map(String::as_str),
        Some("src/lib.rs::format_failures_preserve_order_and_full_selectors"),
        "map miss would leave non-pasteable report ids"
    );
}

#[test]
fn rust_logical_to_kiss_test_ids_survives_sibling_parse_error() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());
    fs::write(
        tmp.path().join("src").join("broken_test.rs"),
        "fn broken(\n",
    )
    .unwrap();
    let map =
        rust_logical_to_kiss_test_ids(tmp.path(), &[]).expect("parse errors must not wipe map");
    assert_eq!(
        map.get("tests::gets_value").map(String::as_str),
        Some("src/lib.rs::gets_value")
    );
}

#[test]
fn enumerate_workspace_rust_selectors_excludes_nested_non_member_crates() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());
    let fixture = tmp.path().join("fixtures").join("inner");
    fs::create_dir_all(fixture.join("tests")).unwrap();
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname='inner'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        fixture.join("tests").join("basic.rs"),
        "#[test]\nfn fixture_only() {}\n",
    )
    .unwrap();
    let fake_rust = tmp.path().join("tests").join("fake_rust");
    fs::create_dir_all(&fake_rust).unwrap();
    fs::write(
        fake_rust.join("syntactic_witness_lib.rs"),
        "#[cfg(test)]\nmod tests { #[test] fn witness_only() {} }\n",
    )
    .unwrap();

    let selectors = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();

    assert_eq!(selectors, vec!["tests::gets_value".to_string()]);
}

#[test]
fn enumerate_changed_rust_tests_excludes_fixture_paths() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());
    let fixture_test = tmp
        .path()
        .join("tests")
        .join("fixtures")
        .join("inner")
        .join("test.rs");
    let fake_rust_test = tmp
        .path()
        .join("tests")
        .join("fake_rust")
        .join("syntactic_witness_lib.rs");
    fs::create_dir_all(fixture_test.parent().unwrap()).unwrap();
    fs::create_dir_all(fake_rust_test.parent().unwrap()).unwrap();
    fs::write(&fixture_test, "#[test]\nfn fixture_only() {}\n").unwrap();
    fs::write(
        &fake_rust_test,
        "#[cfg(test)]\nmod tests { #[test] fn witness_only() {} }\n",
    )
    .unwrap();

    let changed =
        enumerate_tests_in_changed_files(tmp.path(), &[fixture_test, fake_rust_test]).unwrap();

    assert!(changed.rust_tests.is_empty());
}

#[test]
fn rust_module_population_manifest_selectors_uses_workspace_discovery() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());
    let module = rust_backer::RustModule::for_execution(tmp.path(), &[]);

    let selectors = module.population_manifest_selectors().unwrap();

    assert_eq!(selectors, vec!["tests::gets_value".to_string()]);
}

#[test]
fn enumerate_workspace_rust_selectors_fails_fast_on_invalid_syntax() {
    let tmp = TempDir::new().unwrap();

    write_demo_crate(&tmp, "#[test]\nfn broken(\n");

    let err = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap_err();

    assert!(err.contains("failed to parse Rust workspace file"));
    assert!(err.contains("lib.rs"));
}
