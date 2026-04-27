//! Rust-path regressions for review findings against parser-first `kiss mv`.
//! Sibling of `review_findings.rs`; split per `lines_per_file` advice
//! in `.llm_style/style.md`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

/// Bug: `infer_receiver_type_at` matches `let x: ` first and only takes
/// alnum/`_` characters after it, so `let x: &mut C = ...;` yields the empty
/// string (rejected), then the `x: &` pattern matches and the inferred type is
/// taken from `mut C`, producing `"mut"` rather than `"C"`. As a result,
/// `x.helper()` is not recognized as a `C`-typed receiver and the AST drops
/// the method site (lex fallback is also bypassed because the AST returned
/// `Some`).
/// Code ref: `src/symbol_mv_support/reference.rs::infer_receiver_type_at`,
/// consumed via `ast_plan::method_receiver_matches` for Rust.
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

/// Bug: `collect_rust_item` only matches `Item::Fn`, `Item::Impl`, `Item::Use`,
/// and `Item::Mod`. Trait declarations (`Item::Trait`) — including default
/// method bodies — are never visited, so a trait method definition is not
/// emitted as an AST `Definition` and call sites inside the default body are
/// not emitted as references. Lexical fallback also misses this because
/// `find_impl_blocks("T")` only matches `impl T {`, not `trait T {`.
/// Code ref: `src/symbol_mv_support/ast_rust.rs::collect_rust_item`.
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
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self)"),
        "trait `T::helper` default-body definition must be renamed; got:\n{updated}"
    );
}

/// Bug: `CallVisitor` overrides `visit_use_path` and `visit_use_name` but not
/// `visit_use_rename`, and the default `visit_use_rename` only walks idents
/// (which we don't hook). Thus `use a::{c as alias};` produces no `Import`
/// reference for the original symbol `c`. Lexical fallback would have caught
/// it (`{` precedes `c`, in scope of `use`), but `collect_reference_sites`
/// short-circuits to AST results whenever AST returns `Some`, so renaming the
/// free function `c` leaves `use a::{c as alias};` untouched and the rebind
/// `alias` now points at a non-existent symbol.
/// Code ref: `src/symbol_mv_support/ast_rust.rs::CallVisitor` (missing
/// `visit_use_rename`); consumer at
/// `src/symbol_mv_support/edits.rs::collect_reference_sites`.
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
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_main = fs::read_to_string(&main).unwrap();
    assert!(
        updated_main.contains("use crate::lib::{renamed as aliased};"),
        "`use ... {{c as alias}}` must update the original-name `c` to the new name; got:\n{updated_main}"
    );
}
