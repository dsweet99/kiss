use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

pub fn py(query: &str, new_name: &str, root: &std::path::Path) -> MvOptions {
    MvOptions {
        query: query.to_string(),
        new_name: new_name.to_string(),
        paths: vec![root.display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
        language_tables: Default::default(),
    }
}

pub fn rs(query: &str, new_name: &str, root: &std::path::Path) -> MvOptions {
    MvOptions {
        query: query.to_string(),
        new_name: new_name.to_string(),
        paths: vec![root.display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
        language_tables: Default::default(),
    }
}

#[test]
fn regression_h1_python_subclass_override_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("shapes.py");
    fs::write(
        &file,
        "\
class Shape:
    def area(self):
        return 0


class Square(Shape):
    def __init__(self, s):
        self.s = s

    def area(self):
        return self.s * self.s
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::Shape.area", file.display()),
            "compute_area",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("class Shape:\n    def compute_area(self):"),
        "Shape.area should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("    def compute_area(self):\n        return self.s * self.s"),
        "Square.area override should be renamed to keep the override; got:\n{updated}"
    );
}

#[test]
fn regression_h2_python_super_call_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class Base:
    def greet(self):
        return \"hi\"


class Child(Base):
    def greet(self):
        return super().greet() + \"!\"
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::Base.greet", file.display()),
            "hello",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("super().hello()"),
        "super().greet() should be renamed to super().hello() to keep Child working; got:\n{updated}"
    );
}

#[test]
fn regression_h5_rust_trait_default_override_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
pub trait Greet {
    fn name(&self) -> String;
    fn hello(&self) -> String {
        format!(\"hi {}\", self.name())
    }
}

pub struct Polite;

impl Greet for Polite {
    fn name(&self) -> String { \"polite\".to_string() }
    fn hello(&self) -> String {
        format!(\"good day {}\", self.name())
    }
}

fn use_it(g: &Polite) -> String {
    g.hello()
}
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(rs(
            &format!("{}::Greet.hello", file.display()),
            "greet",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn greet(&self) -> String {\n        format!(\"hi"),
        "trait default `hello` should be renamed to `greet`; got:\n{updated}"
    );
    assert!(
        updated.contains("fn greet(&self) -> String {\n        format!(\"good day"),
        "impl override `hello` should be renamed to `greet` so the impl still satisfies the trait; got:\n{updated}"
    );
    assert!(
        updated.contains("g.greet()"),
        "caller `g.hello()` should be renamed to `g.greet()`; got:\n{updated}"
    );
}

#[test]
fn regression_h6_rust_self_method_call_in_impl_must_be_renameable() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
pub struct C { n: u32 }

impl C {
    pub fn get(&self) -> u32 { self.n }
    pub fn double(&self) -> u32 { self.get() * 2 }
}

fn use_it(c: &C) -> u32 { c.get() }
",
    )
    .unwrap();

    let exit_code = run_mv_command(rs(
        &format!("{}::C.get", file.display()),
        "value",
        tmp.path(),
    ));
    assert_eq!(
        exit_code, 0,
        "rename of an inherent method whose impl contains self.method() must succeed; \
         currently refused with a bogus 'trait-receiver ambiguity'"
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("pub fn value(&self) -> u32 { self.n }"),
        "definition C.get should be renamed to value; got:\n{updated}"
    );
    assert!(
        updated.contains("self.value() * 2"),
        "intra-impl self.get() should be renamed to self.value(); got:\n{updated}"
    );
    assert!(
        updated.contains("c.value()"),
        "external caller c.get() should be renamed to c.value(); got:\n{updated}"
    );
}

#[test]
fn regression_h7_python_forward_ref_string_annotation_should_resolve_receiver() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
class Tree:
    def grow(self) -> \"Tree\":
        return self


def use_it(t: \"Tree\") -> \"Tree\":
    return t.grow()
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::Tree.grow", file.display()),
            "expand",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def expand(self)"),
        "definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("return t.expand()"),
        "caller t.grow() with forward-ref annotation `t: \"Tree\"` should be renamed; got:\n{updated}"
    );
}

#[test]
fn regression_h10_move_into_file_with_name_collision_must_refuse() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src.py");
    let dst = tmp.path().join("dst.py");
    fs::write(&src, "def widget():\n    return 1\n").unwrap();
    fs::write(
        &dst,
        "\
def widget():
    return 2


def other():
    return widget()
",
    )
    .unwrap();

    let original_dst = fs::read_to_string(&dst).unwrap();
    let original_src = fs::read_to_string(&src).unwrap();

    let opts = MvOptions {
        query: format!("{}::widget", src.display()),
        new_name: "widget".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dst.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
        language_tables: Default::default(),
    };

    let exit_code = run_mv_command(opts);
    assert_ne!(
        exit_code, 0,
        "move into a file that already defines `widget` must fail loudly"
    );

    let dst_after = fs::read_to_string(&dst).unwrap();
    let src_after = fs::read_to_string(&src).unwrap();
    assert_eq!(
        dst_after.matches("def widget(").count(),
        1,
        "destination must not end up with two `def widget(` definitions; got:\n{dst_after}"
    );
    assert_eq!(
        dst_after, original_dst,
        "on refusal, dst.py must be unchanged"
    );
    assert_eq!(
        src_after, original_src,
        "on refusal, src.py must be unchanged"
    );
}
