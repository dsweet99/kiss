//! Regression: `kiss mv` must rename a Python identifier when it appears as
//! a *non-first* name on a *non-continuation* line of a multi-line
//! parenthesized `from x import (…)` statement.
//!
//! The existing regression `regression_rename_updates_multiline_parenthesized_from_import`
//! (`symbol_mv_regressions_4.rs`) only covers the case where the target name
//! is the **only** name in the parenthesized block, e.g.
//!
//! ```python
//! from a import (
//!     foo
//! )
//! ```
//!
//! The recommended PEP 8 form for non-trivial imports is
//!
//! ```python
//! from a import (
//!     other,
//!     helper,
//! )
//! ```
//!
//! Here `helper`'s reference-site detector goes through `py_import_allows`
//! in `src/symbol_mv_support/reference.rs`. For `helper` the `prefix_on_line`
//! is whitespace, so the function consults the previous line. The previous
//! line is `    other,` whose `trim_end()` is `    other,`, which matches
//! none of the four allow-cases (`import (`, `, (`, backslash-with-import).
//! `py_non_def_site` then also returns false because `after` starts with `,`,
//! not `(`. Result: `helper` in `b.py` is silently NOT renamed — `b.py` now
//! `import`s a name that no longer exists in `a.py`, raising `ImportError`
//! at runtime.
//!
//! See `_kpop/exp_log_mv_serious_bug_3.md` (H1).

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_python_multiline_paren_import_renames_non_first_name() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("a.py");
    let caller_file = tmp.path().join("b.py");

    fs::write(&def_file, "def helper():\n    return 1\n").unwrap();
    fs::write(
        &caller_file,
        "from a import (\n    other,\n    helper,\n)\n",
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
        updated_caller.contains("    renamed,\n)"),
        "non-first import name on its own line in a multi-line parenthesized \
         `from a import (...)` block should be renamed; got:\n{updated_caller}"
    );
    assert!(
        !updated_caller.contains("    helper,"),
        "old name `helper` should no longer appear in the import block; \
         got:\n{updated_caller}"
    );
    assert!(
        updated_caller.contains("    other,"),
        "unrelated sibling name `other` in the import block must remain \
         unchanged; got:\n{updated_caller}"
    );
}
