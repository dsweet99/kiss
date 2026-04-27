//! Regressions for review findings against the parser-first `kiss mv` work.
//! Sibling files (split per `lines_per_file` advice in
//! `.llm_style/style.md`):
//! - `review_findings_python.rs`
//! - `review_findings_rust.rs`
//! - `review_findings_cache.rs`

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn review_python_receiver_reassigned_should_not_misclassify() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class D:
    def helper(self):
        return 0


class C:
    def helper(self):
        return 1


def caller():
    x = D()
    x = C()
    return x.helper()
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
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def renamed(self):"),
        "C.helper definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("return x.renamed()"),
        "x bound to C() at the call site must be treated as type C; got:\n{updated}"
    );
    assert!(
        updated.contains("class D:\n    def helper(self):"),
        "D.helper must remain untouched; got:\n{updated}"
    );
}

#[test]
fn review_rust_receiver_reassigned_should_not_misclassify() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
struct D;
struct C;
impl D { fn helper(&self) -> u32 { 0 } }
impl C { fn helper(&self) -> u32 { 1 } }

fn caller(c: &C) -> u32 {
    let x: &D = &D;
    let x: &C = c;
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
        updated.contains("impl C { fn renamed(&self)"),
        "C::helper definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("    x.renamed()"),
        "the most recent let-binding (x: &C) at the call site must dominate; got:\n{updated}"
    );
    assert!(
        updated.contains("impl D { fn helper(&self)"),
        "D::helper must remain untouched; got:\n{updated}"
    );
}

#[test]
fn review_owner_none_does_not_rename_bare_attribute_access() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
def helper():
    return 1


def caller(obj):
    target = obj.helper
    return helper()
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
    assert!(updated.contains("def renamed():"), "def renamed; got:\n{updated}");
    assert!(updated.contains("return renamed()"), "call site renamed; got:\n{updated}");
    assert!(
        updated.contains("target = obj.helper"),
        "owner=None bare attribute access (`obj.helper`) is by-design left intact; got:\n{updated}"
    );
}

/// Bug: `ast_definition_ident_offsets` searches the def span (which now
/// includes decorators) for the first `name` substring. A decorator name
/// containing the symbol name as a substring (`helper_dec` ⊃ `helper`)
/// shadows the real `def helper`, so the rename lands inside the decorator
/// name and the actual definition is left intact.
/// Code ref: `src/symbol_mv_support/ast_plan.rs::ast_definition_ident_offsets`.
#[test]
fn review_decorator_substring_must_not_misplace_definition_rename() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
def helper_dec(f):
    return f


@helper_dec
def helper():
    return 1


def caller():
    return helper()
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
    assert!(updated.contains("def renamed():"), "def renamed; got:\n{updated}");
    assert!(
        updated.contains("def helper_dec(f):"),
        "decorator helper_dec must remain intact; got:\n{updated}"
    );
    assert!(
        updated.contains("@helper_dec"),
        "decorator usage must remain `@helper_dec`; got:\n{updated}"
    );
}
