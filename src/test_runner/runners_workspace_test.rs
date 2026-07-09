use std::fs;

use tempfile::TempDir;

use super::runners::{enumerate_workspace_rust_selectors, rust_backer};

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
    write_demo_crate(&tmp, "fn broken(\n");

    let err = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap_err();

    assert!(err.contains("failed to parse Rust workspace file"));
    assert!(err.contains("lib.rs"));
}
