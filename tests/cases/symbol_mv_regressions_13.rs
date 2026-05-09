//! Failing regression tests for the bug found by KPOP round 7
//! (`_kpop/exp_log_mv_serious_bug_7.md`):
//!
//! `kiss mv FILE.py::ClassName NewName` silently leaves the
//! `class ClassName:` definition unchanged while still rewriting every
//! reference to the class to `NewName`. The result is guaranteed broken
//! code (`NameError` at runtime).
//!
//! Root cause: `SymbolKind` (`src/symbol_mv_support/ast_models.rs`) only
//! models `Function` and `Method` — there is no `Class` variant. The
//! `class_definition` arm in `src/symbol_mv_support/ast_python.rs::walk_py`
//! only walks the body and never calls `collect_py_def`, so the class
//! is never recorded as a `Definition`. Meanwhile the value-reference
//! identifier branch happily emits `Reference`s for every occurrence of
//! the class name (constructor calls, base-class lists, `case Foo(...)`
//! patterns, type annotations, …), so every use is renamed against a
//! definition that was never anchored.

use kiss::symbol_mv::run_mv_command;
use std::fs;
use tempfile::TempDir;

use super::symbol_mv_regressions_11::py;

/// Single-file Python class rename.
///
/// `class Circle:` must be renamed to `class Disk:` along with the
/// constructor call `Circle(3)`. Currently the `class` line is silently
/// skipped, so the file post-rename references an undefined name.
#[test]
fn regression_h1_python_class_definition_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class Circle:
    def __init__(self, r):
        self.r = r


def make():
    return Circle(3)
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::Circle", file.display()),
            "Disk",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("class Disk:"),
        "class definition `class Circle:` must be renamed to `class Disk:`; got:\n{updated}"
    );
    assert!(
        updated.contains("return Disk(3)"),
        "constructor call `Circle(3)` must be rewritten to `Disk(3)`; got:\n{updated}"
    );
    assert!(
        !updated.contains("Circle"),
        "no occurrence of the old class name `Circle` should remain; got:\n{updated}"
    );
}

/// Cross-file blast-radius guard.
///
/// When the user asks to rename `Circle` defined in `a.py`, an unrelated
/// file `b.py` that has its own independent `class Circle` must NOT have
/// its references silently rewritten to a `Disk` name that does not
/// exist in `b.py`. (Currently `kiss mv` rewrites uses of `Circle` in
/// `b.py` while leaving `b.py`'s own `class Circle:` definition intact,
/// breaking a file the user did not ask to edit.)
#[test]
fn regression_h1_python_class_rename_must_not_corrupt_unrelated_file() {
    let tmp = TempDir::new().unwrap();
    let a = tmp.path().join("a.py");
    let b = tmp.path().join("b.py");
    fs::write(
        &a,
        "\
class Circle:
    def __init__(self, r):
        self.r = r


def make():
    return Circle(3)
",
    )
    .unwrap();
    fs::write(
        &b,
        "\
class Circle:
    def __init__(self, r):
        self.r = r


def use_local():
    return Circle(7)
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(&format!("{}::Circle", a.display()), "Disk", tmp.path(),)),
        0,
    );

    let updated_b = fs::read_to_string(&b).unwrap();
    assert!(
        updated_b.contains("class Circle:") && updated_b.contains("return Circle(7)"),
        "unrelated file b.py must be left untouched when renaming a.py::Circle; got:\n{updated_b}"
    );
}
