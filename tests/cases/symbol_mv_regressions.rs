use kiss::symbol_mv::{
    EditKind, MvOptions, MvPlan, MvRequest, ParsedQuery, PlannedEdit, apply_plan_transactional,
    language_name, parse_mv_query, plan_edits, run_mv_command, validate_new_name,
};
use kiss::Language;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn parse_python_function_query() {
    let q: ParsedQuery = parse_mv_query("a/b.py::foo").unwrap();
    assert_eq!(q.path, PathBuf::from("a/b.py"));
    assert_eq!(q.symbol, "foo");
    assert_eq!(q.member, None);
    assert_eq!(q.language, Language::Python);
}

#[test]
fn parse_rust_method_query() {
    let q = parse_mv_query("src/lib.rs::Type.method").unwrap();
    assert_eq!(q.symbol, "Type");
    assert_eq!(q.member, Some("method".to_string()));
    assert_eq!(q.old_name(), "method");
}

#[test]
fn reject_bad_queries() {
    assert!(parse_mv_query("missing_separator").is_err());
    assert!(parse_mv_query("a.txt::foo").is_err());
    assert!(parse_mv_query("a.py::foo.bar.baz").is_err());
}

#[test]
fn validate_new_name_rules() {
    assert!(validate_new_name("new_name", Language::Python).is_ok());
    assert!(validate_new_name("new::name", Language::Rust).is_err());
    assert!(validate_new_name("", Language::Rust).is_err());
    assert!(validate_new_name("1bad", Language::Python).is_err());
    assert_eq!(language_name(Language::Python), "python");
}

#[test]
fn mv_json_mode_is_valid_json() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("a.py");
    fs::write(&source, "def foo():\n    return 1\nfoo()\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", source.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: true,
        json: true,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);
}

#[test]
fn regression_mv_json_without_dry_run_applies_edits() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("a.py");
    fs::write(&source, "def foo():\n    return 1\nfoo()\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", source.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: true,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);
    let updated = fs::read_to_string(&source).unwrap();
    assert!(
        updated.contains("def bar(") && updated.contains("bar()"),
        "expected JSON mode without --dry-run to apply renames; got:\n{updated}"
    );
}

#[test]
fn plan_edits_builds_request_from_public_types() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("a.py");
    let caller = tmp.path().join("caller.py");
    fs::write(&source, "def foo():\n    return 1\n").unwrap();
    fs::write(&caller, "from a import foo\nfoo()\n").unwrap();

    let query: ParsedQuery = parse_mv_query(&format!("{}::foo", source.display())).unwrap();
    assert_eq!(query.language_name(), "python");

    let req = MvRequest {
        query,
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        ignore: vec![],
    };

    let plan: MvPlan = plan_edits(&req);
    assert!(
        plan.edits.iter().any(|edit| edit.old_snippet == "foo"),
        "plan_edits should produce rename edits for the requested symbol"
    );
    assert!(
        plan.files.iter().any(|path| path == &source),
        "plan_edits should include the source file in the plan"
    );
}

#[test]
fn mv_rejects_mismatched_lang_filter() {
    let opts = MvOptions {
        query: "src/foo.py::bar".to_string(),
        new_name: "baz".to_string(),
        paths: vec![".".to_string()],
        to: None,
        dry_run: true,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 1);
}

#[test]
fn applies_rename() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(&file, "foo()\n").unwrap();

    let good_plan = MvPlan {
        files: vec![file.clone()],
        edits: vec![PlannedEdit {
            path: file.clone(),
            start_byte: 0,
            end_byte: 3,
            line: 1,
            old_snippet: "foo".to_string(),
            new_snippet: "bar".to_string(),
            kind: EditKind::Reference,
        }],
    };

    apply_plan_transactional(&good_plan).unwrap();
    let updated = fs::read_to_string(&file).unwrap();
    assert_eq!(updated, "bar()\n");
}

#[test]
fn overlap_fails() {
    let file = PathBuf::from("fake.py");
    let plan = MvPlan {
        files: vec![file.clone()],
        edits: vec![
            PlannedEdit {
                path: file.clone(),
                start_byte: 0,
                end_byte: 3,
                line: 1,
                old_snippet: "foo".to_string(),
                new_snippet: "bar".to_string(),
                kind: EditKind::Reference,
            },
            PlannedEdit {
                path: file,
                start_byte: 2,
                end_byte: 5,
                line: 1,
                old_snippet: "o()".to_string(),
                new_snippet: "xx".to_string(),
                kind: EditKind::Reference,
            },
        ],
    };

    assert!(apply_plan_transactional(&plan).is_err());
}

#[test]
fn regression_rust_method_query_should_not_rename_other_types() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mod.rs");

    fs::write(
        &file,
        "struct A;\nstruct B;\n\nimpl A {\n    fn foo(&self) {}\n}\n\nimpl B {\n    fn foo(&self) {}\n}\n\nfn call(a: &A, b: &B) {\n    a.foo();\n    b.foo();\n}\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::A.foo", file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);
    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("impl A {\n    fn bar(&self) {}"));
    assert!(updated.contains("a.bar();"));
    assert!(
        updated.contains("impl B {\n    fn foo(&self) {}"),
        "unrelated type method should remain unchanged"
    );
    assert!(
        updated.contains("b.foo();"),
        "unrelated method call should remain unchanged"
    );
}
