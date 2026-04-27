//! Python-path regressions for review findings against parser-first `kiss mv`.
//! Sibling of `review_findings.rs`; split per `lines_per_file` advice
//! in `.llm_style/style.md`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

/// Bug: `infer_python_receiver_type_at` only matches `x = ...` assignment
/// patterns. A receiver introduced via a parameter annotation
/// (`def caller(x: C)`) yields no inferred type, so the AST `Method`
/// reference is dropped and `x.helper()` is not renamed.
/// Code ref: `src/symbol_mv_support/reference.rs::infer_python_receiver_type_at`.
#[test]
fn review_python_param_typed_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class C:
    def helper(self):
        return 1


def caller(x: C):
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
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "parameter-annotated receiver `x: C` must resolve to type C; got:\n{updated}"
    );
}

/// Bug: `type_from_assignment_rhs` requires the RHS's first character to be
/// ASCII uppercase, which rejects dotted constructors like `pkg.C()`. The
/// receiver type is therefore inferred as `None` and the method site is
/// dropped from the AST owner-qualified rename.
/// Code ref: `src/symbol_mv_support/reference.rs::type_from_assignment_rhs`.
#[test]
fn review_python_dotted_constructor_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("pkg.py");
    let main = tmp.path().join("main.py");
    fs::write(
        &pkg,
        "\
class C:
    def helper(self):
        return 1
",
    )
    .unwrap();
    fs::write(
        &main,
        "\
import pkg


def caller():
    obj = pkg.C()
    return obj.helper()
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::C.helper", pkg.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_main = fs::read_to_string(&main).unwrap();
    assert!(
        updated_main.contains("return obj.renamed()"),
        "obj bound to pkg.C() must be inferred as type C; got:\n{updated_main}"
    );
}

/// Bug: `walk_py` has no arm for the tree-sitter `decorator` node kind, so
/// `@helper` is never recorded as a reference. Renaming the top-level
/// `helper` rewrites the def + bare calls but leaves `@helper` untouched.
/// Code ref: `src/symbol_mv_support/ast_python.rs::walk_py` (match arms).
#[test]
fn review_python_decorator_call_site_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
def helper(f):
    return f


@helper
def other():
    return 1


def caller():
    return helper(other)
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
    assert!(updated.contains("def renamed(f):"), "def renamed; got:\n{updated}");
    assert!(
        updated.contains("@renamed"),
        "`@helper` decorator usage must be renamed; got:\n{updated}"
    );
}

/// Bug: `infer_python_receiver_type_at` only handles the direct `x = ...`
/// assignment pattern; in a chained/multi-target assignment
/// `x = y = C()`, the RHS after `"x = "` is `"y = C()"`, so
/// `type_from_assignment_rhs` reads up to the first `(`, sees a non-uppercase
/// leading char, and returns `None`. The `x.helper()` Method site is then
/// dropped from the owner-qualified rename.
/// Code ref: `src/symbol_mv_support/reference.rs::type_from_assignment_rhs`.
#[test]
fn review_python_chained_assignment_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class C:
    def helper(self):
        return 1


def caller():
    x = y = C()
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
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "chained assignment `x = y = C()` must let `x` be inferred as type C; got:\n{updated}"
    );
}

/// Bug: `type_from_python_param_annotation` extracts the annotation by taking
/// only `[A-Za-z0-9_.]` characters, then takes `rsplit('.').next()` on the
/// resulting bare name. For `def caller(x: Optional[C])`, extraction stops at
/// `[`, yielding `"Optional"`, and the receiver type for `x` resolves to
/// `Optional` instead of the wrapped `C`. The owner-qualified Method site is
/// dropped and `x.helper()` is not renamed.
/// Code ref: `src/symbol_mv_support/reference.rs::type_from_python_param_annotation`.
#[test]
fn review_python_optional_param_annotation_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
from typing import Optional


class C:
    def helper(self):
        return 1


def caller(x: Optional[C]):
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
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "`x: Optional[C]` parameter must resolve to type C for receiver inference; got:\n{updated}"
    );
}

/// Bug: `infer_python_receiver_type_at` searches for the substring
/// `format!("{receiver} = ")` without a left word-boundary check. For
/// receiver `x`, the substring `"x = "` also occurs inside `"prev_x = D()"`,
/// so a same-named-suffix variable appearing before the call site is
/// mistakenly read as the binding for `x`. The receiver type resolves to
/// the wrong class (D), the Method site is filtered out by
/// `method_receiver_matches`, and `x.helper()` is not renamed.
/// Code ref: `src/symbol_mv_support/reference.rs::infer_python_receiver_type_at`.
#[test]
fn review_python_receiver_substring_must_not_steal_binding() {
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


def caller(x: C):
    prev_x = D()
    use(prev_x)
    return x.helper()


def use(_v):
    return None
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
        updated.contains("class C:\n    def renamed(self):"),
        "C.helper definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("return x.renamed()"),
        "receiver `x` (typed as C via param annotation) must not be confused with `prev_x = D()`; got:\n{updated}"
    );
    assert!(
        updated.contains("class D:\n    def helper(self):"),
        "D.helper must remain untouched; got:\n{updated}"
    );
}

/// Bug: `walk_py` recurses unconditionally into function bodies, and
/// `collect_py_def` uses the `owner` of the surrounding *class* (None for
/// top-level functions) regardless of the enclosing function. An inner
/// `def helper` defined inside `def outer()` therefore registers as a
/// second top-level Definition with the same name and owner=None.
/// `ast_definition_ident_offsets` returns BOTH name spans, so renaming the
/// outer `helper` also rewrites the unrelated inner `def helper` shadow.
/// Code ref: `src/symbol_mv_support/ast_python.rs::walk_py` (function arm)
///           + `src/symbol_mv_support/ast_plan.rs::ast_definition_ident_offsets`.
#[test]
fn review_python_inner_function_shadow_must_not_be_renamed() {
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
    assert!(
        updated.contains("def renamed():\n    return 1\n"),
        "outer (top-level) helper must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("    def helper():\n        return 2\n"),
        "inner shadow `def helper` inside `outer` must NOT be renamed; got:\n{updated}"
    );
}
