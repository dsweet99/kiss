use kiss::symbol_mv::{MvOptions, run_mv_command};
use kiss::Language;
use std::fs;
use tempfile::TempDir;

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
fn regression_rename_updates_parenthesized_from_import() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def foo():\n    return 1\n").unwrap();
    fs::write(&caller_file, "from a import (foo)\n").unwrap();

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

    let updated_caller = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated_caller.contains("from a import (bar)"),
        "parenthesized from-import should rename the symbol; got:\n{updated_caller}"
    );
}

#[test]
fn regression_rename_updates_backslash_continued_from_import() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def foo():\n    return 1\n").unwrap();
    fs::write(&caller_file, "from a import \\\n    foo\n").unwrap();

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

    let updated_caller = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated_caller.contains("from a import \\\n    bar"),
        "backslash-continued from-import should rename; got:\n{updated_caller}"
    );
}

#[test]
fn regression_rename_nonlocal_binding() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def spam():\n    return 1\n").unwrap();
    fs::write(
        &caller_file,
        "def outer():\n    def spam():\n        return 2\n    def inner():\n        nonlocal spam\n        return spam()\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::spam", def_file.display()),
        new_name: "eggs".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated.contains("nonlocal eggs"),
        "`nonlocal` target should match nested def rename; got:\n{updated}"
    );
}

#[test]
fn regression_rename_global_binding_in_same_file() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("m.py");

    fs::write(
        &file,
        "def spam():\n    return 1\n\ndef g():\n    global spam\n    return spam()\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::spam", file.display()),
        new_name: "eggs".to_string(),
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
        updated.contains("global eggs"),
        "`global` should rename with the symbol; got:\n{updated}"
    );
}

#[test]
fn regression_rename_await_expression() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "async def spam():\n    return 1\n").unwrap();
    fs::write(&caller_file, "async def g():\n    return await spam\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::spam", def_file.display()),
        new_name: "eggs".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated.contains("return await eggs"),
        "`await` operand should rename; got:\n{updated}"
    );
}

#[test]
fn regression_rename_del_statement() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def spam():\n    pass\n").unwrap();
    fs::write(&caller_file, "def g():\n    del spam\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::spam", def_file.display()),
        new_name: "eggs".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated.contains("del eggs"),
        "`del` target should rename; got:\n{updated}"
    );
}

#[test]
fn regression_rename_raise_from_exception_cause() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def foo():\n    pass\n").unwrap();
    fs::write(
        &caller_file,
        "def g():\n    try:\n        1 / 0\n    except ZeroDivisionError as e:\n        raise RuntimeError('x') from foo\n",
    )
    .unwrap();

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

    let updated_caller = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated_caller.contains("raise RuntimeError('x') from bar"),
        "`raise ... from <exc>` should rename the chained exception; got:\n{updated_caller}"
    );
}

#[test]
fn regression_rename_updates_backslash_inside_parenthesized_from_import() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def bar():\n    return 1\n").unwrap();
    fs::write(&caller_file, "from a import (foo, \\\n    bar)\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::bar", def_file.display()),
        new_name: "baz".to_string(),
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
        updated_caller.contains("from a import (foo, \\\n    baz)"),
        "backslash after comma in parenthesized import should rename; got:\n{updated_caller}"
    );
}

