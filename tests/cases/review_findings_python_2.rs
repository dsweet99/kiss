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
        language_tables: Default::default(),
    };
    let _ = file;
    assert_eq!(run_mv_command(opts), 0);
}

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

    run_python_mv(
        &file,
        format!("{}::C.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return self.renamed()"),
        "`self.helper()` inside the same class must be renamed; got:\n{updated}"
    );
}

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

    run_python_mv(
        &file,
        format!("{}::C.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "walrus `(x := C())` must let `x` be inferred as type C; got:\n{updated}"
    );
}

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

    run_python_mv(
        &file,
        format!("{}::C.helper", file.display()),
        "renamed",
        tmp.path(),
    );

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
        updated.contains("x.renamed()"),
        "x is a C, so x.helper() must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("y.helper()"),
        "y is a D, not a C; y.helper() must NOT be renamed; got:\n{updated}"
    );
}

#[test]
fn review_python_same_named_builders_should_use_matching_receiver() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class X:
    def helper(self):
        return 1


class Y:
    def helper(self):
        return 2


class A:
    def build(self) -> X:
        return X()


class B:
    def build(self) -> Y:
        return Y()


def caller(a: A, b: B):
    return a.build().helper() + b.build().helper()
",
    )
    .unwrap();

    run_python_mv(
        &file,
        format!("{}::X.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("a.build().renamed()"),
        "matching receiver should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("b.build().helper()"),
        "non-matching receiver must remain unchanged; got:\n{updated}"
    );
}

#[test]
fn review_python_classmethod_receiver_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class C:
    @classmethod
    def helper(cls):
        return 1

    @classmethod
    def caller(cls):
        return cls.helper()
",
    )
    .unwrap();

    run_python_mv(
        &file,
        format!("{}::C.helper", file.display()),
        "renamed",
        tmp.path(),
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(cls):"), "got:\n{updated}");
    assert!(
        updated.contains("return cls.renamed()"),
        "classmethod receiver `cls` must be renamed; got:\n{updated}"
    );
}
