//! Regression: `kiss mv` must rename identifier references that appear inside
//! the **code-bearing braces** of a Python f-string (PEP 498).
//!
//! `is_code_offset` in `src/symbol_mv_support/lex.rs` treats every `'…'` /
//! `"…"` as opaque string content. It never inspects the `f` / `b` / `r`
//! prefix, so it cannot tell that the contents of `f"…{expr}…"` between the
//! braces is actually executable Python code that may reference the symbol
//! being renamed.
//!
//! Result: after `kiss mv a.py::helper renamed`,
//!
//! - the definition in `a.py` is rewritten,
//! - the `from a import helper` line in `b.py` is rewritten to
//!   `from a import renamed`,
//! - but `f"value={helper()}"` in `b.py` is left literally as
//!   `f"value={helper()}"`.
//!
//! Running `b.caller()` after the rename now raises
//! `NameError: name 'helper' is not defined` because the import was rewritten
//! but the f-string body was not.
//!
//! See `_kpop/exp_log_mv_serious_bug_4.md` (H1).

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_python_fstring_braces_are_renamed() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def helper():\n    return 1\n").unwrap();
    fs::write(
        &caller_file,
        "\
from a import helper


def caller():
    return f\"value={helper()}\"
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", def_file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated_def = fs::read_to_string(&def_file).unwrap();
    let updated_caller = fs::read_to_string(&caller_file).unwrap();

    assert!(
        updated_def.contains("def renamed():"),
        "definition in source file should be renamed; got:\n{updated_def}"
    );
    assert!(
        updated_caller.contains("from a import renamed"),
        "import line in caller should be renamed; got:\n{updated_caller}"
    );

    assert!(
        updated_caller.contains("f\"value={renamed()}\""),
        "identifier reference inside f-string braces must be renamed \
         to keep the project runnable after `kiss mv`; got:\n{updated_caller}"
    );
    assert!(
        !updated_caller.contains("helper()"),
        "old name `helper` must not appear anywhere in the caller after \
         the rename (otherwise running the file raises NameError); \
         got:\n{updated_caller}"
    );
}

#[test]
fn regression_python_async_method_definition_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");

    fs::write(
        &file,
        "\
class C:
    async def helper(self):
        return 1

async def caller():
    obj = C()
    return await obj.helper()
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
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated = fs::read_to_string(&file).unwrap();

    assert!(
        !updated.contains("async def helper(self):"),
        "async method definition should be rewritten; got:\n{updated}"
    );
    assert!(
        updated.contains("async def renamed(self):"),
        "async method definition should be renamed to `renamed`; got:\n{updated}"
    );
    assert!(
        updated.contains("return await obj.renamed()"),
        "async call site should be updated to the new name; got:\n{updated}"
    );
}

#[test]
fn regression_rust_async_function_definition_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");

    fs::write(
        &file,
        "async fn helper() -> u32 {\n    1\n}\n\nasync fn caller() {\n    let _ = helper().await;\n}\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated = fs::read_to_string(&file).unwrap();

    assert!(
        !updated.contains("async fn helper() -> u32"),
        "async function definition should be rewritten; got:\n{updated}"
    );
    assert!(
        updated.contains("async fn renamed() -> u32"),
        "async function definition should be renamed to `renamed`; got:\n{updated}"
    );
    assert!(
        updated.contains("let _ = renamed().await;"),
        "async call site should be updated to the new name; got:\n{updated}"
    );
}

#[test]
fn regression_rust_method_should_ignore_shadowed_local_name() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");

    fs::write(
        &file,
        "\
trait T { fn helper(&self) -> u32; }

struct S;

impl T for S {
    fn helper(&self) -> u32 { 1 }
}

fn caller(s: &S) -> u32 {
    fn helper() -> u32 { 0 }
    s.helper() + helper()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::S.helper", file.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed(&self) -> u32 { 1 }"),
        "trait method definition should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("s.renamed()"),
        "method call on the receiver should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("fn helper() -> u32 { 0 }"),
        "shadowed local helper definition must remain unchanged; got:\n{updated}"
    );
    assert!(
        updated.contains("s.renamed() + helper()"),
        "shadowed local helper call must remain unchanged; got:\n{updated}"
    );
}

#[test]
fn regression_python_malformed_source_falls_back_to_lexical_scan() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");

    fs::write(
        &file,
        "\
def helper():
    return 1

def caller():
    return helper()

if True:
    print(\"broken\"
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
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def renamed():"),
        "definition should still be renamed via lexical fallback; got:\n{updated}"
    );
    assert!(
        updated.contains("return renamed()"),
        "call site should still be renamed via lexical fallback; got:\n{updated}"
    );
}

#[test]
fn regression_rust_malformed_source_falls_back_to_lexical_scan() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");

    fs::write(
        &file,
        "\
fn helper() -> u32 { 1 }

fn caller() -> u32 {
    helper()
}

fn broken() {
    let _ = (
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
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("fn renamed() -> u32 { 1 }"),
        "definition should still be renamed via lexical fallback; got:\n{updated}"
    );
    assert!(
        updated.contains("renamed()"),
        "call site should still be renamed via lexical fallback; got:\n{updated}"
    );
}

#[test]
fn regression_definition_prefers_ast_when_lexical_misidentifies() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
\"\"\"
Module docstring that contains a fake def line:
    def helper(self):
        return 999
\"\"\"


class C:
    def helper(self):
        return 1


def caller():
    return C().helper()
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
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def renamed(self):"),
        "AST-resolved class method should be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("    def helper(self):\n        return 999"),
        "fake def line inside the docstring must remain intact; got:\n{updated}"
    );
    assert!(
        updated.contains("return C().renamed()"),
        "method call site should be updated; got:\n{updated}"
    );
}
