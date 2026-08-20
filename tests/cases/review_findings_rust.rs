use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn review_rust_let_ref_mut_annotation_should_resolve_inner_type() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct C;
impl C { fn helper(&mut self) -> u32 { 1 } }

fn caller(c: &mut C) -> u32 {
    let x: &mut C = c;
    x.helper()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::C.helper", file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("impl C { fn renamed(&mut self)"),
        "C::helper definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("    x.renamed()"),
        "`let x: &mut C` must resolve receiver type to `C` (not `mut`); got:\n{updated}"
    );
}

#[test]
fn review_rust_trait_default_method_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
trait T {
    fn helper(&self) -> u32 { 7 }
}

struct S;
impl T for S {}

fn caller(s: &S) -> u32 {
    s.helper()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::T.helper", file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self)"),
        "trait `T::helper` default-body definition must be renamed; got:\n{updated}"
    );
}

#[test]
fn review_rust_use_rename_should_update_original_name() {
    let tmp = TempDir::new().unwrap();
    let lib = tmp.path().join("lib.rs");
    let main = tmp.path().join("main.rs");
    fs::write(
        &lib,
        "\
pub fn helper() -> u32 { 1 }
",
    )
    .unwrap();
    fs::write(
        &main,
        "\
use crate::lib::{helper as aliased};

fn caller() -> u32 {
    aliased()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", lib.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_main = fs::read_to_string(&main).unwrap();
    assert!(
        updated_main.contains("use crate::lib::{renamed as aliased};"),
        "`use ... {{c as alias}}` must update the original-name `c` to the new name; got:\n{updated_main}"
    );
}
