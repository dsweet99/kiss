
use kiss::cli_output::{print_py_test_refs, print_rs_test_refs};
use kiss::config_gen::{merge_config_toml, write_mimic_config};
use kiss::{find_python_files, ParsedFile, ParsedRustFile};
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_print_test_refs_functions() {
    let py: Vec<ParsedFile> = vec![];
    let rs: Vec<ParsedRustFile> = vec![];
    assert_eq!(print_py_test_refs(&py), 0);
    assert_eq!(print_rs_test_refs(&rs), 0);
}

#[test]
fn test_write_mimic_config() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("out.toml");
    write_mimic_config(&path, "[python]\nx = 1", 1, 0);
    assert!(path.exists());
}

#[test]
fn test_merge_config_toml() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    writeln!(tmp, "[python]\nstatements_per_function = 10").unwrap();
    let merged = merge_config_toml(tmp.path(), "[rust]\nstatements_per_function = 20", false, true);
    assert!(merged.contains("[python]") || merged.contains("[rust]"));
}

#[test]
fn test_find_files_by_extension_via_python() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("x.py"), "").unwrap();
    let files = find_python_files(tmp.path());
    assert_eq!(files.len(), 1);
}

