use kiss::symbol_mv::{MvOptions, run_mv_command};
use kiss::Language;
use std::fs;
use tempfile::TempDir;

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
