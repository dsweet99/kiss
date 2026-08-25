use std::fs;
use std::path::Path;

use crate::rust_llvm_cov_runner::plan::shared_input::{
    is_cargo_config_input_path, rust_cov_input_files, workspace_input_digest,
};

#[test]
fn shared_input_helpers_are_witnessed_from_external_test_module() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".cargo")).unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join(".cargo").join("config.toml"), "").unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let config_path = tmp.path().join(".cargo").join("config.toml");
    let recognizes_config = is_cargo_config_input_path(&config_path);
    let rejects_root = is_cargo_config_input_path(Path::new("config.toml"));
    let files = rust_cov_input_files(tmp.path()).unwrap();
    let digest = workspace_input_digest(tmp.path()).unwrap();

    assert!(recognizes_config);
    assert!(!rejects_root);
    assert!(files.iter().any(|path| path.ends_with("config.toml")));
    assert!(!digest.is_empty());
}
