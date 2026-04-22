//! Unit tests for `symbol_mv_support` (separate file so `kiss` skips it for size rules).

use std::fs;
use std::path::Path;

use crate::Language;
use crate::symbol_mv::{EditKind, MvPlan, PlannedEdit};
use crate::symbol_mv_support::{
    DefinitionSpan, MoveEditsParams, ReferenceRenameParams, SourceRenameParams,
    apply_plan_transactional, build_move_edits, collect_reference_edits,
    collect_source_rename_edits, detect_language, find_definition_span, gather_candidate_files,
    is_valid_identifier, parse_symbol_shape, run_mv_inner,
};
use tempfile::TempDir;

#[test]
fn basics_detect_and_parse() {
    assert_eq!(
        detect_language(Path::new("x.py")).unwrap(),
        Language::Python
    );
    assert_eq!(detect_language(Path::new("x.rs")).unwrap(), Language::Rust);
    assert_eq!(
        detect_language(Path::new("x.PY")).unwrap(),
        Language::Python
    );
    assert_eq!(detect_language(Path::new("x.RS")).unwrap(), Language::Rust);
    assert!(detect_language(Path::new("x.txt")).is_err());

    let (s, m) = parse_symbol_shape("Foo.bar", Language::Rust).unwrap();
    assert_eq!(s, "Foo");
    assert_eq!(m.as_deref(), Some("bar"));
    assert!(parse_symbol_shape("bad..x", Language::Python).is_err());

    assert!(is_valid_identifier("_a1", Language::Python));
    assert!(!is_valid_identifier("1a", Language::Python));
}

#[test]
fn gather_candidate_files_respects_language() {
    let tmp = TempDir::new().unwrap();
    let py = tmp.path().join("a.py");
    let rs = tmp.path().join("b.rs");
    fs::write(&py, "").unwrap();
    fs::write(&rs, "").unwrap();
    let base = tmp.path().display().to_string();
    let py_files = gather_candidate_files(std::slice::from_ref(&base), &[], Language::Python);
    assert!(py_files.iter().any(|p| p.ends_with("a.py")));
    let rs_files = gather_candidate_files(&[base], &[], Language::Rust);
    assert!(rs_files.iter().any(|p| p.ends_with("b.rs")));
}

#[test]
fn find_python_definition_span_finds_method() {
    let src = "class C:\n    def foo(self):\n        pass\n";
    let span = find_definition_span(src, "foo", Some("C"), Language::Python).unwrap();
    assert!(src[span.start..span.end].contains("def foo"));
}

#[test]
fn find_rust_definition_span_finds_fn() {
    let src = "impl T {\n    pub fn bar(&self) {}\n}\n";
    let span = find_definition_span(src, "bar", Some("T"), Language::Rust).unwrap();
    assert!(src[span.start..span.end].contains("fn bar"));
}

#[test]
fn collect_edits_roundtrip_smoke() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("m.py");
    fs::write(&path, "def x():\n    pass\nx()\n").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    let refs = collect_reference_edits(&ReferenceRenameParams {
        path: &path,
        content: &content,
        old_name: "x",
        new_name: "y",
        owner: None,
        language: Language::Python,
    });
    assert!(!refs.is_empty());

    let renames = collect_source_rename_edits(&SourceRenameParams {
        source_path: &path,
        source_content: &content,
        old_name: "x",
        new_name: "y",
        owner: None,
        language: Language::Python,
        def_span: find_definition_span(&content, "x", None, Language::Python),
        moving: false,
    });
    assert!(
        renames
            .iter()
            .any(|e| matches!(e.kind, EditKind::Definition))
    );
}

#[test]
fn build_move_edits_inserts_at_dest() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("a.py");
    let dst = tmp.path().join("b.py");
    fs::write(&src, "def foo():\n    return 1\n").unwrap();
    fs::write(&dst, "").unwrap();
    let content = fs::read_to_string(&src).unwrap();
    let span = find_definition_span(&content, "foo", None, Language::Python).unwrap();
    let built = build_move_edits(&MoveEditsParams {
        source_path: &src,
        source_content: &content,
        old_name: "foo",
        new_name: "foo",
        def_span: Some(span),
        dest: Some(&dst),
    });
    assert!(built.is_some());
}

#[test]
fn apply_plan_transactional_writes() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("t.py");
    fs::write(&f, "abc").unwrap();
    let plan = MvPlan {
        files: vec![f.clone()],
        edits: vec![PlannedEdit {
            path: f.clone(),
            start_byte: 0,
            end_byte: 3,
            line: 1,
            old_snippet: "abc".into(),
            new_snippet: "xyz".into(),
            kind: EditKind::Reference,
        }],
    };
    apply_plan_transactional(&plan).unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "xyz");
}

#[test]
fn run_mv_inner_errors_on_empty_plan() {
    use crate::symbol_mv::MvOptions;

    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("z.py");
    fs::write(&p, "# empty\n").unwrap();
    let opts = MvOptions {
        query: format!("{}::nope", p.display()),
        new_name: "a".into(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: true,
        json: false,
        lang_filter: None,
        ignore: vec![],
    };
    assert!(run_mv_inner(opts).is_err());
}

#[test]
fn definition_span_contains() {
    let s = DefinitionSpan { start: 1, end: 4 };
    assert!(s.contains(1));
    assert!(!s.contains(4));
}
