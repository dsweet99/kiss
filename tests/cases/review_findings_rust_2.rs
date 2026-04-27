//! Fifth-pass Rust regressions for review findings against parser-first
//! `kiss mv`. Sibling of `review_findings_rust.rs`; split per
//! `lines_per_file` advice in `.llm_style/style.md`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

fn run_rust_mv(query: String, new_name: &str, root: &std::path::Path) {
    let opts = MvOptions {
        query,
        new_name: new_name.to_string(),
        paths: vec![root.display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);
}

/// Bug: `CallVisitor` (in `ast_rust.rs`) does not override `visit_macro` /
/// `visit_expr_macro`, and `syn` does not parse macro `TokenStream` bodies
/// into expressions. Any function call inside a macro invocation
/// (`println!`, `format!`, `assert_eq!`, `vec!`, etc.) yields zero AST
/// `Reference`s. Renaming `helper` therefore leaves
/// `println!("{}", helper())` and `vec![helper()]` un-renamed even though
/// the lexical fallback would have caught them.
///
/// Code ref: `src/symbol_mv_support/ast_rust.rs::CallVisitor` (no
/// `visit_macro` override).
#[test]
fn review_rust_macro_body_call_sites_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
fn helper() -> u32 { 1 }

fn caller() {
    println!(\"{}\", helper());
    let _v = vec![helper(), helper()];
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("fn renamed()"), "definition must be renamed; got:\n{updated}");
    assert!(
        !updated.contains("helper()"),
        "all call sites including those inside macros must be renamed; got:\n{updated}"
    );
}

/// Bug: `collect_rust_item` (in `ast_rust.rs`) only matches `Item::Fn`,
/// `Item::Impl`, `Item::Use`, `Item::Mod`. `Item::ForeignMod`
/// (`extern "C" { fn helper(); }`) is ignored, so the FFI declaration is
/// invisible to AST def discovery. AST returns no Definition, and lexical
/// fallback is bypassed because the AST result is `Some` (parse succeeded).
/// Renaming `helper` skips the `extern` declaration and the call sites
/// inside `unsafe { helper() }` end up referring to a now-undefined symbol.
///
/// Code ref: `src/symbol_mv_support/ast_rust.rs::collect_rust_item` match arms.
#[test]
fn review_rust_extern_block_fn_declaration_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
extern \"C\" {
    fn helper() -> u32;
}

fn caller() -> u32 {
    unsafe { helper() }
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed() -> u32;"),
        "extern \"C\" fn declaration must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("unsafe { renamed() }"),
        "call site referencing the extern fn must be renamed; got:\n{updated}"
    );
}

/// Bug: `impl_owner_name` (in `ast_rust.rs`) returns `None` for any
/// `self_ty` that is not `Type::Path`. `impl Trait for &X` (or `&mut X`,
/// tuples, slices, fn pointers) therefore registers methods with
/// `owner: None`, colliding with free functions of the same name and
/// preventing owner-qualified rename `kiss mv ::X.helper renamed` from
/// finding the method.
///
/// Code ref: `src/symbol_mv_support/ast_rust.rs::impl_owner_name`.
#[test]
fn review_rust_impl_for_reference_type_should_attribute_owner() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;

trait T { fn helper(&self) -> u32; }

impl T for &X {
    fn helper(&self) -> u32 { 1 }
}

fn caller(x: &X) -> u32 { x.helper() }
",
    )
    .unwrap();

    run_rust_mv(format!("{}::X.helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "method on `impl T for &X` must be attributed to owner X; got:\n{updated}"
    );
}

/// Bug: `impl_owner_name` takes `path.segments.last().ident` only and
/// ignores generic arguments. For `impl T for Box<X> { fn helper(&self) {} }`,
/// the inferred owner is `"Box"`, not `"X"`. Owner-qualified rename
/// `kiss mv ::X.helper renamed` therefore drops the method, and the
/// definition stays as `helper`.
///
/// Code ref: `src/symbol_mv_support/ast_rust.rs::impl_owner_name` (no
/// generic-argument unwrap for `Box`/`Vec`/`Arc`/`Rc`/`Pin`).
#[test]
fn review_rust_impl_for_boxed_type_should_attribute_inner_owner() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;

trait T { fn helper(&self) -> u32; }

impl T for Box<X> {
    fn helper(&self) -> u32 { 1 }
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::X.helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "method on `impl T for Box<X>` must resolve owner to inner type X; got:\n{updated}"
    );
}

/// Bug: `collect_top_fn` calls `visit_item_fn` which descends into the
/// body and records call sites of nested functions, but no walker code
/// path emits a `Definition` for a `fn` declared inside another `fn` body.
/// AST returns no Definition for `inner_helper`, lexical fallback is
/// bypassed because the AST `Some(_)`-returns of `ast_reference_offsets`
/// override it, and the `fn inner_helper` declaration is left un-renamed
/// while its call site is rewritten — silently breaking compilation.
///
/// Code ref: `src/symbol_mv_support/ast_rust.rs::collect_top_fn` (no
/// nested-fn def emission inside `CallVisitor`).
#[test]
fn review_rust_nested_function_definition_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
fn outer() -> u32 {
    fn inner_helper() -> u32 { 7 }
    inner_helper()
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::inner_helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed() -> u32 { 7 }"),
        "nested fn definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("    renamed()"),
        "nested fn call site must be renamed; got:\n{updated}"
    );
}

/// Bug: `extract_receiver` (in `reference.rs`) strips one trailing `()`
/// and one trailing `.`, then takes the trailing identifier. For
/// `x.foo().helper()`, the `before`-the-`helper` slice is `x.foo().` and
/// the extracted receiver is `"foo"` — i.e. the previous *method name*
/// rather than a variable. `infer_receiver_type_at` then looks for
/// `let foo: ...` (which never exists), returns `None`, and
/// `method_receiver_matches` filters the call out. Owner-qualified rename
/// `kiss mv ::Y.helper renamed` therefore silently misses chained method
/// calls.
///
/// Code ref: `src/symbol_mv_support/reference.rs::extract_receiver`
/// interacting with `ast_plan.rs::method_receiver_matches`.
#[test]
fn review_rust_chained_method_call_receiver_should_resolve_return_type() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;
struct Y;

impl X { fn into_y(&self) -> Y { Y } }
impl Y { fn helper(&self) -> u32 { 1 } }

fn caller(x: &X) -> u32 {
    x.into_y().helper()
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::Y.helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "Y::helper definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("x.into_y().renamed()"),
        "chained call `x.into_y().helper()` should be recognized as a Y receiver; got:\n{updated}"
    );
}
