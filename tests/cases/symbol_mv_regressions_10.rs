use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_rust_trait_receiver_ambiguity_causes_nonzero_exit() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
trait T { fn helper(&self) -> u32; }

struct A;
struct B;

impl T for A {
    fn helper(&self) -> u32 { 1 }
}

impl T for B {
    fn helper(&self) -> u32 { 2 }
}

fn helper() -> u32 { 0 }

fn call<C>(x: &C) -> u32
where
    C: T,
{
    x.helper()
}

fn use_types(a: A, b: B) -> u32 {
    a.helper() + b.helper() + helper() + call(&a)
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::A.helper", file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    let exit_code = run_mv_command(opts);
    assert_ne!(exit_code, 0, "ambiguous receiver resolution must not be treated as success");

    let updated = fs::read_to_string(&file).unwrap();
    assert!(!updated.contains("renamed"));
    assert!(updated.contains("a.helper() + b.helper() + helper() + call(&a)"));
    assert!(updated.contains("trait T { fn helper(&self) -> u32; }"));
}

#[test]
fn regression_python_nested_scope_shadowing_should_preserve_inner_helper() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
def helper():
    return 1

def outer():
    def helper():
        return 2
    return helper()

def caller():
    return helper() + outer()
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert_eq!(
        updated,
        "def renamed():\n    return 1\n\ndef outer():\n    def helper():\n        return 2\n    return helper()\n\ndef caller():\n    return renamed() + outer()\n"
    );
}
