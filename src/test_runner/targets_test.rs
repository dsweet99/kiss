use super::*;
use kiss::Language;
use std::fs;
use tempfile::tempdir;

#[test]
fn parse_test_target_accepts_path_and_symbol_forms() {
    let path_only = parse_test_target("src/lib.rs").unwrap();
    assert_eq!(path_only.language, Language::Rust);
    assert!(path_only.symbol.is_none());

    let with_symbol = parse_test_target("tests/test_x.py::test_y").unwrap();
    assert_eq!(with_symbol.language, Language::Python);
    assert_eq!(with_symbol.symbol.as_deref(), Some("test_y"));
    assert!(with_symbol.member.is_none());
    assert!(with_symbol.python_nodeid.is_none());

    let with_member = parse_test_target("src/app.py::Foo.bar").unwrap();
    assert_eq!(with_member.symbol.as_deref(), Some("Foo"));
    assert_eq!(with_member.member.as_deref(), Some("bar"));

    let parametrized =
        parse_test_target("tests/slow/ops/test_ops_help.py::test_ops_help[observability.py]")
            .unwrap();
    assert_eq!(
        parametrized.python_nodeid.as_deref(),
        Some("tests/slow/ops/test_ops_help.py::test_ops_help[observability.py]")
    );
    assert!(parametrized.symbol.is_none());

    let class_test = parse_test_target("tests/test_x.py::TestBox::test_method").unwrap();
    assert_eq!(
        class_test.python_nodeid.as_deref(),
        Some("tests/test_x.py::TestBox::test_method")
    );
}

#[test]
fn parse_test_target_rejects_malformed_operands() {
    assert!(parse_test_target("").is_err());
    assert!(parse_test_target("::foo").is_err());
    assert!(parse_test_target("a.py::").is_err());
    assert!(parse_test_target("a.py::Foo.bar.baz").is_err());
    assert!(parse_test_target("a.rs::A::B").is_err());
    assert!(parse_test_target("readme.md").is_err());
}

#[test]
fn rust_source_model_extracts_tests_and_types() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("lib.rs");
    fs::write(
        &path,
        r#"
pub struct Config;
impl Config {
    pub fn load() {}
}
pub fn helper() {}
#[cfg(test)]
mod tests {
    #[test]
    fn covers_helper() {}
}
"#,
    )
    .unwrap();
    let model = super::model::load_source_model(&path, Language::Rust).unwrap();
    assert!(
        model
            .definitions
            .iter()
            .any(|d| d.name == "Config" && d.member.is_none())
    );
    assert!(
        model
            .definitions
            .iter()
            .any(|d| d.name == "Config" && d.member.as_deref() == Some("load"))
    );
    assert!(
        model
            .direct_tests
            .iter()
            .any(|t| t.selector.contains("covers_helper"))
    );
    assert!(!model.non_test_lines().is_empty());
}

#[test]
fn python_source_model_marks_test_and_class_spans() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("mod.py");
    fs::write(
        &path,
        "class Box:\n    def value(self):\n        return 1\n\ndef helper():\n    return 2\n\ndef test_helper():\n    assert helper() == 2\n",
    )
    .unwrap();
    let model = super::model::load_source_model(&path, Language::Python).unwrap();
    assert!(
        model
            .definitions
            .iter()
            .any(|d| d.name == "Box" && d.member.is_none())
    );
    assert!(
        model
            .definitions
            .iter()
            .any(|d| d.name == "Box" && d.member.as_deref() == Some("value"))
    );
    assert!(model.direct_tests.iter().any(|t| t.name == "test_helper"));
    let non_test = model.non_test_lines();
    assert!(non_test.contains(&1));
    assert!(!non_test.is_empty());
}

#[test]
fn python_attach_nodeids_for_function_and_class_tests() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("test_mod.py");
    fs::write(
        &path,
        "def test_top():\n    assert True\n\nclass TestBox:\n    def test_method(self):\n        assert True\n",
    )
    .unwrap();
    let mut model = super::model::load_source_model(&path, Language::Python).unwrap();
    let nodeids = vec![
        "test_mod.py::test_top".to_string(),
        "test_mod.py::test_top[case-a]".to_string(),
        "test_mod.py::TestBox::test_method".to_string(),
    ];
    super::model_python::attach_python_nodeids(&mut model, &nodeids, "test_mod.py");
    assert!(
        model
            .direct_tests
            .iter()
            .any(|t| t.selector == "test_mod.py::test_top")
    );
    assert!(
        model
            .direct_tests
            .iter()
            .any(|t| t.selector == "test_mod.py::test_top[case-a]")
    );
    assert!(
        model
            .direct_tests
            .iter()
            .any(|t| t.selector == "test_mod.py::TestBox::test_method")
    );
    assert!(
        model
            .definitions
            .iter()
            .any(|d| d.test_selector.as_deref() == Some("test_mod.py::test_top"))
    );
}

#[test]
fn python_build_model_rejects_syntax_errors() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("bad.py");
    fs::write(&path, "def broken(\n").unwrap();
    let err = super::model::load_source_model(&path, Language::Python).unwrap_err();
    assert!(
        err.contains("syntax error") || err.contains("failed to parse"),
        "{err}"
    );
}
