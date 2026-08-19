use std::path::PathBuf;

use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::test_utils::parse_python_source;

use super::{collect_comment_violations, has_non_doc_comments};

fn parse_rs(source: &str) -> ParsedRustFile {
    ParsedRustFile {
        path: PathBuf::from("t.rs"),
        source: source.to_string(),
        ast: syn::parse_file("").unwrap(),
    }
}

fn py_count(source: &str) -> usize {
    let parsed: ParsedFile = parse_python_source(source);
    collect_comment_violations(&[parsed], &[]).len()
}

fn rs_count(source: &str) -> usize {
    collect_comment_violations(&[], &[parse_rs(source)]).len()
}

#[test]
fn python_flags_hash_comments_not_docstrings_or_strings() {
    assert_eq!(py_count("x = 1\n"), 0);
    assert_eq!(py_count("# hi\nx = 1\n"), 1);
    assert_eq!(py_count("\"\"\"module doc\"\"\"\nx = 1\n"), 0);
    assert_eq!(py_count("s = '# not a comment'\n"), 0);
    assert_eq!(py_count("def f():\n    \"\"\"fn doc\"\"\"\n    return 1\n"), 0);
    assert_eq!(py_count("def f():\n    \"\"\"fn doc\"\"\"\n    # n\n    return 1\n"), 1);
}

#[test]
fn rust_flags_plain_comments_not_docs_or_strings() {
    assert_eq!(rs_count("fn f() {}\n"), 0);
    assert_eq!(rs_count("// n\nfn f() {}\n"), 1);
    assert_eq!(rs_count("/// doc\nfn f() {}\n"), 0);
    assert_eq!(rs_count("//! inner\nfn f() {}\n"), 0);
    assert_eq!(rs_count("/** d */\nfn f() {}\n"), 0);
    assert_eq!(rs_count("/*! d */\nfn f() {}\n"), 0);
    assert_eq!(rs_count("//// n\nfn f() {}\n"), 1);
    assert_eq!(rs_count("/* n */\nfn f() {}\n"), 1);
    assert_eq!(rs_count("let s = \"// n\";\n"), 0);
    assert_eq!(rs_count("let s = r#\"// n\"#;\n"), 0);
}

#[test]
fn has_non_doc_comments_matches_collect() {
    let with_cmt = parse_python_source("# x\n");
    let clean = parse_python_source("x = 1\n");
    assert!(has_non_doc_comments(&[with_cmt], &[]));
    assert!(!has_non_doc_comments(&[clean], &[]));
    assert!(!has_non_doc_comments(&[], &[parse_rs("/// d\nfn f() {}\n")]));
    assert!(has_non_doc_comments(&[], &[parse_rs("// n\nfn f() {}\n")]));
}
