use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

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
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "parameter-annotated receiver `x: C` must resolve to type C; got:\n{updated}"
    );
}

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
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_main = fs::read_to_string(&main).unwrap();
    assert!(
        updated_main.contains("return obj.renamed()"),
        "obj bound to pkg.C() must be inferred as type C; got:\n{updated_main}"
    );
}

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
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def renamed(f):"),
        "def renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("@renamed"),
        "`@helper` decorator usage must be renamed; got:\n{updated}"
    );
}

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
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "chained assignment `x = y = C()` must let `x` be inferred as type C; got:\n{updated}"
    );
}

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
        language_tables: Default::default(),
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def renamed(self):"), "got:\n{updated}");
    assert!(
        updated.contains("return x.renamed()"),
        "`x: Optional[C]` parameter must resolve to type C for receiver inference; got:\n{updated}"
    );
}

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
        language_tables: Default::default(),
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
        language_tables: Default::default(),
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
    assert!(
        updated.contains("    return helper()"),
        "the call inside `outer` must keep resolving to the inner shadow; got:\n{updated}"
    );
    assert!(
        updated.contains("def caller():\n    return renamed()"),
        "the top-level caller must still be renamed; got:\n{updated}"
    );
}
