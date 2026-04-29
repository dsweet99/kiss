//! Failing regression tests for the bug found by KPOP round 9
//! (`_kpop/exp_log_mv_serious_bug_9.md`):
//!
//! `kiss mv FILE.py::Cls.attr NewName` silently misses every bare
//! attribute *read* of `attr`. The walker in
//! `src/symbol_mv_support/ast_python.rs::walk_py` only emits an attribute
//! identifier as a `Reference` when the attribute is the function child
//! of a `call` node (`collect_py_call`) or appears in a decorator
//! (`collect_decorator`). For any other `attribute` node the walker just
//! recurses into its children, and the trailing attribute identifier is
//! suppressed by the `"attribute" if same("attribute") => false` guard
//! in `python_identifier_is_value_reference`. Net effect: `@property`
//! reads (`b.area`), method-as-value reads (`cb = obj.handler`), and
//! attribute writes (`obj.field = …`) are all invisible to the planner.
//!
//! Post-rename the class defines the new name but every bare-attribute
//! site still reads the old one, raising `AttributeError` at runtime.

use kiss::symbol_mv::run_mv_command;
use std::fs;
use tempfile::TempDir;

use super::symbol_mv_regressions_11::py;

/// `@property` read where the receiver type is unambiguous (`self`
/// inside a method on the same class, and a constructor-call chain
/// `Box().area`). Per R3, only receiver-disambiguated reads should be
/// rewritten — but those *must* be rewritten, otherwise the rename
/// breaks the class's own internal use of its property.
#[test]
fn regression_h1_python_property_read_self_and_chain_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("prop2.py");
    fs::write(
        &file,
        "\
class Box:
    @property
    def area(self):
        return self._w * self._h

    def grow(self):
        return self.area * 2


x = Box().area
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::Box.area", file.display()),
            "surface",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def surface(self):"),
        "property definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("return self.surface * 2"),
        "intra-class `self.area` read must be rewritten; got:\n{updated}"
    );
    assert!(
        updated.contains("Box().surface"),
        "chained `Box().area` read must be rewritten; got:\n{updated}"
    );
    assert!(
        !updated.contains(".area"),
        "no `.area` attribute access should remain; got:\n{updated}"
    );
}

/// Method-as-value read (`cb = obj.field`, no call) where the receiver
/// is annotated `c: C`, and attribute write (`obj.field = …`) on the
/// same annotated parameter. Both go through `attribute` nodes that
/// are not call functions, and were silently missed by the planner
/// before KPOP round 9 H1 was fixed. With an explicit type annotation
/// the receiver is unambiguously `C`, so R3 admits both rewrites.
#[test]
fn regression_h1_python_attribute_read_and_write_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("attr.py");
    fs::write(
        &file,
        "\
class C:
    def field(self):
        return 1


def consume(c: C):
    cb = c.field
    c.field = 5
    return cb
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::C.field", file.display()),
            "renamed",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def renamed(self):"),
        "method definition must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("cb = c.renamed"),
        "annotated method-as-value read `c.field` must be rewritten to `c.renamed`; got:\n{updated}"
    );
    assert!(
        updated.contains("c.renamed = 5"),
        "annotated attribute write `c.field = 5` must be rewritten to `c.renamed = 5`; got:\n{updated}"
    );
    assert!(
        !updated.contains("c.field"),
        "no `c.field` reference should remain; got:\n{updated}"
    );
}

/// Negative half of R3 ("precision before reach"): a bare-attribute
/// read on a parameter with no type annotation has an unknown receiver
/// type; per R3 it must NOT be rewritten. This guards against an
/// over-eager fix to KPOP round 9 H1 that would silently rename
/// `b.area` everywhere `b` is unannotated and break unrelated types
/// that happen to share the attribute name.
#[test]
fn regression_h1_python_unannotated_attribute_read_must_not_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("prop.py");
    fs::write(
        &file,
        "\
class Box:
    @property
    def area(self):
        return 4


def use(b):
    return b.area + b.area
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::Box.area", file.display()),
            "surface",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def surface(self):"),
        "property definition `area` must be renamed to `surface`; got:\n{updated}"
    );
    assert!(
        updated.contains("return b.area + b.area"),
        "unannotated `b.area` reads must be left alone (R3: precision before reach); got:\n{updated}"
    );
}
