use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_rename_updates_multiline_parenthesized_from_import() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def foo():\n    return 1\n").unwrap();
    fs::write(&caller_file, "from a import (\n    foo\n)\n").unwrap();

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
        updated_caller.contains("from a import (\n    bar\n)"),
        "multiline parenthesized from-import should rename the symbol; got:\n{updated_caller}"
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

#[test]
fn regression_rust_raw_string_with_embedded_quotes_should_not_rename() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.rs");
    let dest = tmp.path().join("dest.rs");

    fs::write(
        &src,
        "fn foo() {\n    let s = r#\"has \"foo\" embedded\"#;\n    foo();\n}\n",
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

    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        updated_dest.contains("fn bar()"),
        "function should be renamed to bar"
    );
    assert!(
        updated_dest.contains("bar();"),
        "recursive call should be renamed to bar"
    );
    assert!(
        updated_dest.contains(r##"r#"has "foo" embedded"#"##),
        "raw string with embedded quotes should NOT be modified; got:\n{updated_dest}",
    );
}

#[test]
fn regression_python_triple_quoted_string_with_embedded_quote_should_not_rename() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.py");
    let dest = tmp.path().join("dest.py");

    fs::write(
        &src,
        r#"def bar():
    """This docstring mentions "bar" in quotes"""
    return bar()
"#,
    )
    .unwrap();
    fs::write(&dest, "# destination\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::bar", src.display()),
        new_name: "foo".to_string(),
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
        updated_dest.contains("def foo():"),
        "function definition should be renamed to foo"
    );
    assert!(
        updated_dest.contains("return foo()"),
        "recursive call should be renamed to foo"
    );
    assert!(
        updated_dest.contains(r#"mentions "bar" in quotes"#),
        "identifier inside triple-quoted string with embedded quotes should NOT be modified; got:\n{updated_dest}",
    );
}

#[test]
fn regression_rust_char_literal_with_double_quote_should_not_break_lexer() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.rs");
    let dest = tmp.path().join("dest.rs");

    fs::write(
        &src,
        r#"fn bar() {
    let q = '"';
    bar();
}
"#,
    )
    .unwrap();
    fs::write(&dest, "// destination\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::bar", src.display()),
        new_name: "foo".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        updated_dest.contains("fn foo()"),
        "function definition should be renamed to foo"
    );
    assert!(
        updated_dest.contains("foo();"),
        "recursive call after char literal should be renamed to foo; got:\n{updated_dest}",
    );
}

#[test]
fn regression_rust_move_should_not_break_on_brace_inside_string_literal() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source.rs");
    let dest = tmp.path().join("dest.rs");

    fs::write(
        &src,
        r#"fn foo() {
    let s = "}";
    foo();
}
"#,
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
        updated_dest.contains("fn bar()"),
        "function definition should be renamed to bar"
    );
    assert!(
        updated_dest.contains(r#"let s = "}";"#),
        "string literal containing a closing brace should remain intact after move; got:\n{updated_dest}",
    );
    assert!(
        updated_dest.contains("bar();"),
        "recursive call after brace-containing string should be renamed to bar; got:\n{updated_dest}",
    );
    assert!(
        !updated_src.contains("bar();"),
        "moved recursive call should not be left stranded in the source file; got:\n{updated_src}",
    );
}

#[test]
fn regression_rust_method_owner_scoping_should_use_exact_impl_match() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mod.rs");

    fs::write(
        &file,
        "struct A;\nstruct A2;\n\nimpl A2 {\n    fn foo(&self) {}\n}\n\nimpl A {\n    fn foo(&self) {}\n}\n\nfn call(a: &A, a2: &A2) {\n    a.foo();\n    a2.foo();\n}\n",
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
    assert!(
        updated.contains("impl A {\n    fn bar(&self) {}"),
        "requested owner method should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("a.bar();"),
        "call through the requested owner should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("impl A2 {\n    fn foo(&self) {}"),
        "similarly named owner should remain unchanged; got:\n{updated}"
    );
    assert!(
        updated.contains("a2.foo();"),
        "call through similarly named owner should remain unchanged; got:\n{updated}"
    );
}

#[test]
fn regression_rust_rename_should_update_use_imports() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("mod.rs");
    let caller_file = tmp.path().join("caller.rs");

    fs::write(&def_file, "pub fn foo() {\n    let _ = 1;\n}\n").unwrap();
    fs::write(
        &caller_file,
        "use crate::mod::foo;\n\nfn call() {\n    foo();\n}\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", def_file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_def = fs::read_to_string(&def_file).unwrap();
    let updated_caller = fs::read_to_string(&caller_file).unwrap();

    assert!(
        updated_def.contains("pub fn bar()"),
        "definition should be renamed to bar; got:\n{updated_def}"
    );
    assert!(
        updated_caller.contains("use crate::mod::bar;"),
        "Rust use-import should be renamed with the symbol; got:\n{updated_caller}"
    );
    assert!(
        updated_caller.contains("bar();"),
        "Rust call site should also be renamed; got:\n{updated_caller}"
    );
}
