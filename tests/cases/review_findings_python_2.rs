//! Fifth-pass Python regressions for review findings against parser-first
//! `kiss mv`. Sibling of `review_findings_python.rs`; split per
//! `lines_per_file` advice in `.llm_style/style.md`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

fn run_python_mv(file: &std::path::Path, query: String, new_name: &str, root: &std::path::Path) {
    let opts = MvOptions {
        query,
        new_name: new_name.to_string(),
        paths: vec![root.display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };
    let _ = file;
    assert_eq!(run_mv_command(opts), 0);
}

/// Bug: `infer_python_receiver_type_at` has no special case for the
/// receiver `self` and does not consult the surrounding class scope. For
/// `def caller(self): return self.helper()` inside `class C`, the receiver
/// `self` does not start with an uppercase letter, no `self = ...`
/// assignment exists, and there is no `(self: T)` annotation. The inferred
/// type is `None`, the AST `Method` site is dropped, and the call to
/// `self.helper()` is silently left un-renamed. This breaks the most
/// common Python method-call shape.
///
/// Code ref: `src/symbol_mv_support/reference.rs::infer_python_receiver_type_at`
/// (no `self` resolution against enclosing `class C:` block) and
/// `src/symbol_mv_support/ast_plan.rs::method_receiver_matches`.
#[test]
fn review_python_self_receiver_must_be_renamed_in_same_class() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class C:
    def helper(self):
        return 1

    def caller(self):
        return self.helper()
",
    )
    .unwrap();

    run_python_mv(&file, format!("{}::C.helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return self.renamed()"),
        "`self.helper()` inside the same class must be renamed; got:\n{updated}"
    );
}

/// Bug: `infer_python_receiver_type_at` only matches the literal pattern
/// `"{receiver} = "` (single `=` with single space). The walrus operator
/// `(x := C())` introduces `x` as a `C`, but the pattern never matches,
/// so receiver type resolution returns `None`, the AST `Method` site is
/// dropped, and `x.helper()` is not renamed.
///
/// Code ref: `src/symbol_mv_support/reference.rs::infer_python_receiver_type_at`
/// (`format!("{receiver} = ")` only).
#[test]
fn review_python_walrus_operator_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class C:
    def helper(self):
        return 1


def caller():
    if (x := C()):
        return x.helper()
    return None
",
    )
    .unwrap();

    run_python_mv(&file, format!("{}::C.helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "walrus `(x := C())` must let `x` be inferred as type C; got:\n{updated}"
    );
}

/// Bug: `infer_python_receiver_type_at` uses `rfind` over the substring
/// `"{receiver} = "` and `type_from_assignment_rhs` then takes the *whole*
/// RHS up to the first `(`. For tuple unpacking `x, y = C(), D()`, the
/// substring `", y = "` matches and the inferred RHS is `"C(), D"`, whose
/// last dotted segment starts with uppercase — yielding type `D` for `y`
/// from the substring `", y = "` (or worse, type `C` from the `x` slot
/// being matched against the start of the RHS). The result is a
/// **wrong rewrite**: a call site that is not in scope of the renamed
/// owner is silently rewritten.
///
/// Code ref: `src/symbol_mv_support/reference.rs::infer_python_receiver_type_at`
/// + `type_from_assignment_rhs` (no awareness of multi-target assignments).
#[test]
fn review_python_tuple_unpacking_must_not_misbind_receiver() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class C:
    def helper(self):
        return 1


class D:
    def helper(self):
        return 2


def caller():
    x, y = C(), D()
    return (x.helper(), y.helper())
",
    )
    .unwrap();

    run_python_mv(&file, format!("{}::C.helper", file.display()), "renamed", tmp.path());

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("class C:\n    def renamed(self):"),
        "C.helper definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("class D:\n    def helper(self):"),
        "D.helper must remain untouched; got:\n{updated}"
    );
    assert!(
        updated.contains("y.helper()"),
        "y is a D, not a C; y.helper() must NOT be renamed; got:\n{updated}"
    );
}
