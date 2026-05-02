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
    assert_ne!(
        exit_code, 0,
        "ambiguous receiver resolution must not be treated as success"
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(!updated.contains("renamed"));
    assert!(updated.contains("a.helper() + b.helper() + helper() + call(&a)"));
    assert!(updated.contains("trait T { fn helper(&self) -> u32; }"));
}

#[test]
fn regression_rust_trait_receiver_ambiguity_owned_query_renames_only_owner_scope() {
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

fn use_types(a: A, b: B) -> u32 {
    a.helper() + b.helper() + helper()
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

    assert_eq!(
        run_mv_command(opts),
        0,
        "owned receiver should be unambiguous and rewrite"
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "owner impl should be renamed"
    );
    assert!(updated.contains("a.renamed() + b.helper() + helper()"));
    assert!(!updated.contains("a.helper"));
    assert!(!updated.contains("trait T { fn renamed"));
    assert!(updated.contains("impl T for A"));
    assert!(updated.contains("fn helper(&self) -> u32 { 2 }"));
    assert!(updated.contains("fn helper() -> u32 { 0 }"));
}

#[test]
fn regression_python_nested_scope_shadowing_should_preserve_inner_helper() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    let source = "\
def helper():
    return 1

def outer():
    def helper():
        return 2
    return helper()

def caller():
    return helper() + outer()
";
    let expected = "\
def renamed():
    return 1

def outer():
    def helper():
        return 2
    return helper()

def caller():
    return renamed() + outer()
";
    assert_nested_python_shadowing_rename(&tmp, &file, source, expected);
}

#[test]
fn regression_python_nested_scope_shadowing_is_preserved_deeply() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    let source = "\
def helper():
    return 1

def outer():
    def helper():
        def helper():
            return 3
        return helper()
    def nested():
        return helper()

    return helper() + nested()

def caller():
    return helper() + outer()\n";
    let expected = "\
def renamed():
    return 1

def outer():
    def helper():
        def helper():
            return 3
        return helper()
    def nested():
        return helper()

    return helper() + nested()

def caller():
    return renamed() + outer()
";
    assert_nested_python_shadowing_rename(&tmp, &file, source, expected);
}

fn assert_nested_python_shadowing_rename(
    tmp: &TempDir,
    file: &std::path::Path,
    source: &str,
    expected: &str,
) {
    fs::write(file, source).unwrap();
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

    let updated = fs::read_to_string(file).unwrap();
    assert_eq!(updated, expected);
}
