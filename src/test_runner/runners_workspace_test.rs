use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::runners::{
    enumerate_tests_in_changed_files, enumerate_workspace_rust_selectors, rust_backer,
    rust_logical_to_kiss_test_ids,
};

fn write_demo_crate(tmp: &TempDir, lib_rs: &str) {
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), lib_rs).unwrap();
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
    // Regression: *_test.rs fixtures must not fall back to bare logical ids.
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let map = rust_logical_to_kiss_test_ids(&repo, &[]).unwrap();
    let logical = "format_failures_preserve_order_and_full_selectors";
    assert_eq!(
        map.get(logical).map(String::as_str),
        Some("src/test_runner/final_summary_test.rs::format_failures_preserve_order_and_full_selectors"),
        "map miss would leave non-pasteable report ids"
    );
}

#[test]
fn rust_logical_to_kiss_test_ids_survives_sibling_parse_error() {
    let tmp = TempDir::new().unwrap();
    write_demo_crate(&tmp, demo_test_lib());
    fs::write(tmp.path().join("src").join("broken_test.rs"), "fn broken(\n").unwrap();
    let map = rust_logical_to_kiss_test_ids(tmp.path(), &[]).expect("parse errors must not wipe map");
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
    // Must include bare #[test] so the file is syn-parsed (non-test files are
    // skipped; they cannot contribute selectors under rust_test_functions_in).
    write_demo_crate(&tmp, "#[test]\nfn broken(\n");

    let err = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap_err();

    assert!(err.contains("failed to parse Rust workspace file"));
    assert!(err.contains("lib.rs"));
}
