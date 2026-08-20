use kiss::symbol_mv::run_mv_command;
use std::fs;
use tempfile::TempDir;

use super::symbol_mv_regressions_11::py;

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
