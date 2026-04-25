use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_rust_second_impl_block_method_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mod.rs");

    fs::write(
        &file,
        "\
struct Foo;

impl Foo {
    fn alpha(&self) -> i32 { 1 }
}

impl Foo {
    fn beta(&self) -> i32 { 2 }
}

fn call(f: &Foo) {
    f.alpha();
    f.beta();
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::Foo.beta", file.display()),
        new_name: "gamma".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");
    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn gamma("),
        "method definition in second impl block should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("f.gamma()"),
        "call site should be updated to new name; got:\n{updated}"
    );
    assert!(
        updated.contains("fn alpha("),
        "unrelated method in first impl block should remain unchanged; got:\n{updated}"
    );
    assert!(
        updated.contains("f.alpha()"),
        "unrelated call site should remain unchanged; got:\n{updated}"
    );
}
