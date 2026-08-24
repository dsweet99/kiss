use kiss::symbol_mv::run_mv_command;
use std::fs;
use tempfile::TempDir;

use super::symbol_mv_regressions_11::py;

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
