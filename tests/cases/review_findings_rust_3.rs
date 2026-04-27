//! Additional Rust regressions for parser-first `kiss mv`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn review_rust_generic_impl_should_preserve_receiver_resolution() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;
struct Y;
struct A<T>(T);
struct B<T>(T);

impl<T> A<T> { fn build(&self) -> X { X } }
impl<T> B<T> { fn build(&self) -> Y { Y } }

impl X { fn helper(&self) -> u32 { 1 } }
impl Y { fn helper(&self) -> u32 { 2 } }

fn caller(a: &A<u8>, b: &B<u8>) -> u32 {
    a.build().helper() + b.build().helper()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::X.helper", file.display()),
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
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "X::helper definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("a.build().renamed()"),
        "generic impl header must still resolve the A receiver; got:\n{updated}"
    );
    assert!(
        updated.contains("b.build().helper()"),
        "generic impl header must not misattribute the B receiver; got:\n{updated}"
    );
}

#[test]
fn review_rust_associated_function_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;
impl X { fn helper() -> u32 { 1 } }
fn caller() -> u32 { X::helper() }
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::X.helper", file.display()),
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
        updated.contains("fn renamed() -> u32 { 1 }"),
        "associated function definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("X::renamed()"),
        "associated function call site should be renamed; got:\n{updated}"
    );
}

#[test]
fn review_rust_chained_method_call_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;
struct Y;

impl X { fn into_y(&self, n: i32) -> Y { let _ = n; Y } }
impl Y { fn helper(&self) -> u32 { 1 } }

fn caller(x: &X) -> u32 { x.into_y(1).helper() }
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::Y.helper", file.display()),
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
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("x.into_y(1).renamed()"),
        "chained method call should be renamed; got:\n{updated}"
    );
}

#[test]
fn review_rust_move_should_not_rename_shadowed_inner_helper() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("a.rs");
    let dest = tmp.path().join("dest.rs");
    fs::write(
        &source,
        "\
fn helper() -> u32 { 1 }

fn outer() -> u32 {
    fn helper() -> u32 { 2 }
    helper()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", source.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_source = fs::read_to_string(&source).unwrap();
    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        updated_dest.contains("fn renamed() -> u32 { 1 }"),
        "moved helper should be renamed in destination; got:\n{updated_dest}"
    );
    assert!(
        updated_source.contains("    fn helper() -> u32 { 2 }"),
        "shadowed inner helper must remain unchanged in source; got:\n{updated_source}"
    );
    assert!(
        updated_source.contains("    helper()"),
        "shadowed inner call must remain unchanged in source; got:\n{updated_source}"
    );
}
