use std::collections::BTreeSet;

use super::covered_statements_digest;

const APP: &str = "def add(a, b):\n    return a + b\n\n\ndef mul(a, b):\n    return a * b\n";

fn lines(items: &[u32]) -> BTreeSet<u32> {
    items.iter().copied().collect()
}

fn same(before: &str, after: &str, covered: &[u32]) -> bool {
    covered_statements_digest(before, &lines(covered))
        == covered_statements_digest(after, &lines(covered))
}

#[test]
fn edit_to_uncovered_body_keeps_digest() {
    let edited = APP.replace("a * b", "b * a");
    assert!(same(APP, &edited, &[1, 2, 5]));
    assert!(!same(APP, &edited, &[1, 5, 6]));
}

#[test]
fn edit_to_shared_def_line_changes_digest() {
    let edited = APP.replace("def mul(a, b):", "def mul(a, b=2):");
    assert!(!same(APP, &edited, &[1, 2, 5]));
}

#[test]
fn continuation_line_of_covered_statement_counts() {
    let before = "def f():\n    return g(\n        1,\n    )\n";
    let after = before.replace("        1,", "        2,");
    assert!(!same(before, &after, &[1, 2]));
}

#[test]
fn multi_line_string_continuation_counts() {
    let before = "X = \"\"\"\nold\n\"\"\"\n";
    let after = before.replace("old", "new");
    assert!(!same(before, &after, &[1]));
}

#[test]
fn enclosing_else_header_counts() {
    let before = "def f(x, y):\n    if x:\n        return 1\n    else:\n        return 2\n";
    let after = before.replace("    else:", "    elif y:");
    assert!(!same(before, &after, &[1, 2, 5]));
}

#[test]
fn shifted_lines_and_missing_lines_change_digest() {
    let shifted = format!("import os\n{APP}");
    assert!(!same(APP, &shifted, &[1, 2]));
    assert!(!same(APP, "def add(a, b):\n", &[1, 2]));
}

#[test]
fn comments_and_hash_in_strings_do_not_break_scanning() {
    let before = "def f():\n    s = \"#(\"  # (\n    return s\n\n\ndef g():\n    return 1\n";
    let after = before.replace("return 1", "return 2");
    assert!(same(before, &after, &[1, 2, 3, 6]));
}

#[test]
fn appended_module_statement_changes_digest() {
    let rebound = format!("{APP}\n\nadd = lambda a, b: 0\n");
    assert!(!same(APP, &rebound, &[1, 2, 5]));
    let new_def = format!("{APP}\n\ndef sub(a, b):\n    return a - b\n");
    assert!(!same(APP, &new_def, &[1, 2, 5]));
}

#[test]
fn class_body_counts_but_method_body_does_not() {
    let before = "class C:\n    X = 1\n\n    def m(self):\n        return 1\n";
    let new_attr = before.replace("    X = 1\n", "    X = 1\n    Y = 2\n");
    assert!(!same(before, &new_attr, &[1, 2, 4]));
    let method_edit = before.replace("return 1", "return 2");
    assert!(same(before, &method_edit, &[1, 2, 4]));
}

#[test]
fn multi_line_docstring_in_uncovered_body_is_skipped() {
    let before = "def f():\n    \"\"\"Doc\nstill doc\n\"\"\"\n    return 1\n\n\nY = 1\n";
    let after = before.replace("still doc", "edited doc");
    assert!(same(before, &after, &[1, 8]));
}

#[test]
fn trailing_backslash_continues_statement() {
    let before = "X = 1 + \\\n    2\n";
    let after = before.replace("    2", "    3");
    assert!(!same(before, &after, &[1]));
}
