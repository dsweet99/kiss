use std::path::Path;

use super::cfg_pred::{AtomInterner, CfgPred};
use super::error::RoleBuildError;
use super::rust_walk::{WalkOutput, walk_file};

fn walk_source(path: &Path, source: &str) -> Result<WalkOutput, RoleBuildError> {
    let ast = syn::parse_file(source).unwrap();
    let mut atoms = AtomInterner::new();
    walk_file(path, &ast, &CfgPred::True, true, &mut atoms)
}

#[test]
fn inline_mod_in_mod_rs_resolves_nested_child_not_sibling() {
    let tmp = tempfile::tempdir().unwrap();
    let server = tmp.path().join("src").join("server");
    std::fs::create_dir_all(server.join("tests")).unwrap();
    let mod_rs = server.join("mod.rs");
    let nested = server.join("tests").join("helper.rs");
    let decoy = server.join("helper.rs");
    std::fs::write(&nested, "pub fn nested() {}\n").unwrap();
    std::fs::write(&decoy, "pub fn decoy() {}\n").unwrap();
    let walked = walk_source(&mod_rs, "mod tests {\n    mod helper;\n}\n").unwrap();
    assert_eq!(walked.mods.len(), 1);
    assert_eq!(walked.mods[0].target, nested);
}

#[test]
fn inline_mod_in_file_module_resolves_under_child_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let server = tmp.path().join("src").join("server");
    std::fs::create_dir_all(server.join("reasoning").join("tests")).unwrap();
    let parent = server.join("reasoning.rs");
    let nested = server.join("reasoning").join("tests").join("helper.rs");
    std::fs::write(&nested, "pub fn nested() {}\n").unwrap();
    let walked = walk_source(&parent, "mod tests {\n    mod helper;\n}\n").unwrap();
    assert_eq!(walked.mods.len(), 1);
    assert_eq!(walked.mods[0].target, nested);
}

#[test]
fn path_on_inline_mod_redirects_nested_path_attribute() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("thread_files")).unwrap();
    let lib = src.join("lib.rs");
    let tls = src.join("thread_files").join("tls.rs");
    std::fs::write(&tls, "pub fn tls() {}\n").unwrap();
    let source = "#[path = \"thread_files\"]\nmod thread {\n    #[path = \"tls.rs\"]\n    mod local_data;\n}\n";
    let walked = walk_source(&lib, source).unwrap();
    assert_eq!(walked.mods.len(), 1);
    assert_eq!(walked.mods[0].target, tls);
}
