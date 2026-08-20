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
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);
}

#[test]
fn review_rust_macro_body_call_sites_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
fn helper() -> u32 { 1 }

fn caller() {
    let _direct = helper();
    println!(\"{}\", helper());
    let _v = vec![helper(), helper()];
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed()"),
        "definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("let _direct = renamed();"),
        "direct call site should still be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("println!(\"{}\", renamed())"),
        "macro body call site must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("vec![renamed(), renamed()]"),
        "all macro body call sites must be renamed; got:\n{updated}"
    );
}

#[test]
fn review_rust_nested_shadowed_helper_should_not_be_touched() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
fn helper() -> u32 { 1 }

fn outer() -> u32 {
    let before = helper();
    fn helper() -> u32 { 0 }
    let after = helper();
    before + after
}

fn caller() -> u32 {
    helper()
}
",
    )
    .unwrap();

    run_rust_mv(format!("{}::helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed() -> u32 { 1 }"),
        "outer helper definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("fn outer() -> u32 {\n    let before = helper();"),
        "the call before the nested helper must remain bound to the shadow; got:\n{updated}"
    );
    assert!(
        updated.contains("fn helper() -> u32 { 0 }"),
        "nested shadowed helper definition must remain unchanged; got:\n{updated}"
    );
    assert!(
        updated.contains("    let after = helper();"),
        "the call after the nested helper must remain bound to the shadow; got:\n{updated}"
    );
    assert!(
        updated.contains("fn caller() -> u32 {\n    renamed()"),
        "the top-level caller must still be renamed; got:\n{updated}"
    );
}

#[test]
fn review_rust_same_named_builders_should_use_matching_receiver() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct X;
struct Y;
struct A;
struct B;

impl A { fn build(&self) -> X { X } }
impl B { fn build(&self) -> Y { Y } }

impl X { fn helper(&self) -> u32 { 1 } }
impl Y { fn helper(&self) -> u32 { 2 } }

fn caller(a: &A, b: &B) -> u32 {
    a.build().helper() + b.build().helper()
}
",
    )
    .unwrap();

    run_rust_mv(
        format!("{}::X.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("a.build().renamed()"),
        "matching receiver should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("b.build().helper()"),
        "non-matching receiver must remain unchanged; got:\n{updated}"
    );
}

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

    run_rust_mv(
        format!("{}::X.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "method on `impl T for &X` must be attributed to owner X; got:\n{updated}"
    );
    assert!(
        updated.contains("fn caller(x: &X) -> u32 { x.renamed() }"),
        "the owner-qualified call site must also be renamed; got:\n{updated}"
    );
}

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

    run_rust_mv(
        format!("{}::X.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "method on `impl T for Box<X>` must resolve owner to inner type X; got:\n{updated}"
    );
}

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

    run_rust_mv(
        format!("{}::inner_helper", file.display()),
        "renamed",
        tmp.path(),
    );

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

    run_rust_mv(
        format!("{}::Y.helper", file.display()),
        "renamed",
        tmp.path(),
    );

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
