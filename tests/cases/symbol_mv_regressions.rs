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

#[test]
fn regression_move_to_destination_should_relocate_definition() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(&src, "def foo():\n    return 1\n\nvalue = foo()\n").unwrap();
    fs::write(&dest, "def other():\n    return 2\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", src.display()),
        new_name: "foo".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);
    let updated_src = fs::read_to_string(&src).unwrap();
    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        !updated_src.contains("def foo("),
        "source definition should be removed after move"
    );
    assert!(
        updated_dest.contains("def foo("),
        "destination should contain moved definition"
    );
}

#[test]
fn regression_python_method_should_scope_to_class() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mod.py");

    fs::write(
        &file,
        "class A:\n    def foo(self):\n        pass\n\nclass B:\n    def foo(self):\n        pass\n\ndef use_them():\n    A().foo()\n    B().foo()\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::A.foo", file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);
    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def bar(self):"),
        "A.foo should be renamed to bar"
    );
    assert!(
        updated.contains("class B:\n    def foo(self):"),
        "B.foo should remain unchanged"
    );
}

#[test]
fn regression_should_rename_references_in_other_files_within_paths() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("def.py");
    let caller_file = tmp.path().join("caller.py");

    fs::write(&def_file, "def foo():\n    return 1\n").unwrap();
    fs::write(&caller_file, "from def import foo\n\nresult = foo()\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", def_file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_def = fs::read_to_string(&def_file).unwrap();
    let updated_caller = fs::read_to_string(&caller_file).unwrap();

    assert!(
        updated_def.contains("def bar("),
        "definition should be renamed"
    );
    assert!(
        updated_caller.contains("bar()"),
        "call site in other file should be renamed to bar()"
    );
    assert!(
        updated_caller.contains("import bar"),
        "import statement should be renamed from 'import foo' to 'import bar'"
    );
}

#[test]
fn regression_move_to_destination_should_not_move_unrelated_source_statements() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(
        &src,
        "def foo():\n    return 1\n\nvalue = foo()\nother = 2\n",
    )
    .unwrap();
    fs::write(&dest, "def existing():\n    return 0\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", src.display()),
        new_name: "foo".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_src = fs::read_to_string(&src).unwrap();
    let updated_dest = fs::read_to_string(&dest).unwrap();

    assert!(
        !updated_src.contains("def foo("),
        "source definition should be removed after move"
    );
    assert!(
        updated_src.contains("value = foo()"),
        "source should still contain the call site (value = foo())"
    );
    assert!(
        updated_src.contains("other = 2"),
        "source should still contain unrelated statements"
    );
    assert!(
        updated_dest.contains("def foo("),
        "destination should contain moved definition"
    );
    assert!(
        !updated_dest.contains("value ="),
        "destination should NOT contain unrelated statements from source"
    );
    assert!(
        updated_dest.contains("def existing("),
        "destination should still contain its original content"
    );
}

#[test]
fn regression_python_method_should_not_rename_other_types_in_other_files() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("mod.py");
    let caller_file = tmp.path().join("caller.py");

    fs::write(
        &def_file,
        "class A:\n    def foo(self):\n        pass\n\nclass B:\n    def foo(self):\n        pass\n",
    )
    .unwrap();
    fs::write(
        &caller_file,
        "from mod import A, B\n\nA().foo()\nB().foo()\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::A.foo", def_file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_caller = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated_caller.contains("A().bar()"),
        "calls for the requested class should be renamed"
    );
    assert!(
        updated_caller.contains("B().foo()"),
        "calls for other classes in other files should remain unchanged"
    );
}

#[test]
fn regression_toplevel_rename_should_not_touch_method_calls() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mod.py");

    fs::write(
        &file,
        "def foo():\n    return 1\n\nclass Obj:\n    def foo(self):\n        return 2\n\nresult = foo()\nobj = Obj()\nobj.foo()\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(updated.contains("def bar():"), "top-level function should be renamed");
    assert!(updated.contains("result = bar()"), "direct call should be renamed");
    assert!(
        updated.contains("obj.foo()"),
        "method call on object should NOT be renamed when renaming top-level function"
    );
    assert!(
        updated.contains("def foo(self):"),
        "class method definition should NOT be renamed"
    );
}

#[test]
fn regression_python_method_rename_should_respect_owner() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");

    fs::write(
        &src,
        "class B:\n    def foo(self):\n        return 2\n\nclass A:\n    def foo(self):\n        return 1\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::A.foo", src.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_src = fs::read_to_string(&src).unwrap();

    assert!(
        updated_src.contains("class B:\n    def foo(self):"),
        "B.foo should remain unchanged"
    );
    assert!(
        updated_src.contains("class A:\n    def bar(self):"),
        "A.foo should be renamed to A.bar"
    );
}

#[test]
fn regression_class_scoping_should_use_exact_match() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mod.py");

    fs::write(
        &file,
        "class A2:\n    def foo(self):\n        return 1\n\nclass A:\n    def foo(self):\n        return 2\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::A.foo", file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("class A:\n    def bar(self):"),
        "A.foo should be renamed to A.bar"
    );
    assert!(
        updated.contains("class A2:\n    def foo(self):"),
        "A2.foo should NOT be renamed (A2 is not A)"
    );
}

#[test]
fn regression_method_move_should_be_rejected() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(
        &src,
        "class A:\n    def foo(self):\n        return 1\n",
    )
    .unwrap();
    fs::write(&dest, "# destination\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::A.foo", src.display()),
        new_name: "foo".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    let exit_code = run_mv_command(opts);
    assert_ne!(exit_code, 0, "method moves with --to should be rejected");

    let src_content = fs::read_to_string(&src).unwrap();
    assert!(
        src_content.contains("def foo(self):"),
        "source should be unchanged after rejected move"
    );
}

#[test]
fn regression_move_rename_should_update_recursive_calls() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(
        &src,
        "def factorial(n):\n    if n <= 1:\n        return 1\n    return n * factorial(n - 1)\n",
    )
    .unwrap();
    fs::write(&dest, "# math utils\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::factorial", src.display()),
        new_name: "fact".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        updated_dest.contains("def fact(n):"),
        "definition should be renamed to fact"
    );
    assert!(
        updated_dest.contains("return n * fact(n - 1)"),
        "recursive call inside moved definition should also be renamed to fact"
    );
    assert!(
        !updated_dest.contains("factorial"),
        "no references to old name 'factorial' should remain in moved code"
    );
}

#[test]
fn regression_move_rename_should_not_touch_comments_or_strings() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(
        &src,
        r#"def foo():
    # foo should stay in comment
    x = "foo stays literal"
    return foo()
"#,
    )
    .unwrap();
    fs::write(&dest, "# destination\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", src.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        updated_dest.contains("def bar():"),
        "definition should be renamed to bar"
    );
    assert!(
        updated_dest.contains("return bar()"),
        "recursive call should be renamed to bar"
    );
    assert!(
        updated_dest.contains("# foo should stay in comment"),
        "comment should NOT be modified"
    );
    assert!(
        updated_dest.contains("\"foo stays literal\""),
        "string literal should NOT be modified"
    );
}

#[test]
fn regression_move_rename_should_move_python_decorators_and_preserve_literals() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(
        &src,
        "@trace\n@named(\"foo\")\ndef foo():\n    # foo stays in comment\n    label = \"foo stays literal\"\n    return foo()\n",
    )
    .unwrap();
    fs::write(&dest, "# destination\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", src.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_src = fs::read_to_string(&src).unwrap();
    let updated_dest = fs::read_to_string(&dest).unwrap();

    assert!(
        !updated_src.contains("@trace"),
        "decorators should move with the definition instead of being stranded in the source"
    );
    assert!(
        updated_dest.contains("@trace\n@named(\"foo\")\ndef bar():"),
        "destination should include the full decorated definition with the new name"
    );
    assert!(
        updated_dest.contains("# foo stays in comment"),
        "comments inside moved Python code should remain unchanged"
    );
    assert!(
        updated_dest.contains("\"foo stays literal\""),
        "string literals inside moved Python code should remain unchanged"
    );
    assert!(
        updated_dest.contains("return bar()"),
        "recursive calls inside moved Python code should still be renamed"
    );
}

#[test]
fn regression_move_rename_should_keep_rust_attributes_visibility_and_comments() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.rs");
    let dest = tmp.path().join("dest.rs");

    fs::write(
        &src,
        "#[inline]\npub(crate) fn foo() {\n    // foo stays in comment\n    let label = \"foo stays literal\";\n    foo();\n}\n",
    )
    .unwrap();
    fs::write(&dest, "// destination\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", src.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_src = fs::read_to_string(&src).unwrap();
    let updated_dest = fs::read_to_string(&dest).unwrap();

    assert!(
        !updated_src.contains("#[inline]"),
        "attributes should move with the Rust definition"
    );
    assert!(
        !updated_src.contains("pub(crate)"),
        "Rust visibility should move with the definition instead of being stranded"
    );
    assert!(
        updated_dest.contains("#[inline]\npub(crate) fn bar() {"),
        "destination should preserve Rust attributes and visibility while renaming the function"
    );
    assert!(
        updated_dest.contains("// foo stays in comment"),
        "Rust line comments should remain unchanged during move+rename"
    );
    assert!(
        updated_dest.contains("\"foo stays literal\""),
        "Rust string literals should remain unchanged during move+rename"
    );
    assert!(
        updated_dest.contains("bar();"),
        "recursive Rust calls should still be renamed"
    );
}
