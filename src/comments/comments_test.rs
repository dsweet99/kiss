use std::path::{Path, PathBuf};

use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::test_utils::parse_python_source;

use super::{
    collect_comment_violations, collect_comment_violations_with_roles, collect_doc_violations,
    has_non_doc_comments,
};

fn parse_rs(source: &str) -> ParsedRustFile {
    ParsedRustFile {
        path: PathBuf::from("t.rs"),
        source: source.to_string(),
        ast: syn::parse_file(source).unwrap_or_else(|_| syn::parse_file("").unwrap()),
    }
}

fn py_count(source: &str) -> usize {
    let parsed: ParsedFile = parse_python_source(source);
    collect_comment_violations(&[parsed], &[]).len()
}

fn rs_count(source: &str) -> usize {
    collect_comment_violations(&[], &[parse_rs(source)]).len()
}

fn py_docs(source: &str, allowed: &[&str]) -> usize {
    let parsed: ParsedFile = parse_python_source(source);
    let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_string()).collect();
    collect_doc_violations(&[parsed], &[], &allowed, Path::new(".")).len()
}

fn rs_docs(source: &str, allowed: &[&str]) -> usize {
    let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_string()).collect();
    collect_doc_violations(&[], &[parse_rs(source)], &allowed, Path::new(".")).len()
}

#[test]
fn python_flags_hash_comments_not_docstrings_or_strings() {
    assert_eq!(py_count("x = 1\n"), 0);
    assert_eq!(py_count("# hi\nx = 1\n"), 1);
    assert_eq!(py_count("\"\"\"module doc\"\"\"\nx = 1\n"), 0);
    assert_eq!(py_count("s = '# not a comment'\n"), 0);
    assert_eq!(
        py_count("def f():\n    \"\"\"fn doc\"\"\"\n    return 1\n"),
        0
    );
    assert_eq!(
        py_count("def f():\n    \"\"\"fn doc\"\"\"\n    # n\n    return 1\n"),
        1
    );
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
fn rust_skips_comments_in_cfg_test_mod_when_roles_present() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "fn prod() {}\n#[cfg(test)]\nmod tests {\n    // hidden\n    fn t() {}\n}\n",
    )
    .unwrap();
    let path = src.join("lib.rs");
    let parsed = crate::rust_parsing::parse_rust_file(&path).unwrap();
    let roles = crate::code_roles::build_source_role_index(
        &[],
        std::slice::from_ref(&parsed),
        &[],
        std::slice::from_ref(&path),
    )
    .unwrap();
    assert_eq!(
        collect_comment_violations_with_roles(&[], &[parsed], Some(&roles)).len(),
        0
    );
}

#[test]
fn has_non_doc_comments_matches_collect() {
    let with_cmt = parse_python_source("# x\n");
    let clean = parse_python_source("x = 1\n");
    assert!(has_non_doc_comments(&[with_cmt], &[]));
    assert!(!has_non_doc_comments(&[clean], &[]));
    assert!(!has_non_doc_comments(
        &[],
        &[parse_rs("/// d\nfn f() {}\n")]
    ));
    assert!(has_non_doc_comments(&[], &[parse_rs("// n\nfn f() {}\n")]));
}

#[test]
fn empty_docs_allowed_does_not_flag_docs() {
    assert_eq!(py_docs("\"\"\"module doc\"\"\"\nx = 1\n", &[]), 1);
    assert_eq!(rs_docs("/// doc\nfn f() {}\n", &[]), 1);
}

#[test]
fn python_flags_docstrings_outside_allowed_dirs() {
    assert_eq!(py_docs("x = 1\n", &["nowhere"]), 0);
    assert_eq!(py_docs("\"\"\"module doc\"\"\"\nx = 1\n", &["nowhere"]), 1);
    assert_eq!(
        py_docs(
            "def f():\n    \"\"\"fn doc\"\"\"\n    return 1\n",
            &["nowhere"]
        ),
        1
    );
    assert_eq!(
        py_docs(
            "class C:\n    \"\"\"cls doc\"\"\"\n    pass\n",
            &["nowhere"]
        ),
        1
    );
    assert_eq!(py_docs("s = \"not a docstring\"\n", &["nowhere"]), 0);
    assert_eq!(
        py_docs(
            "def f():\n    x = \"not a docstring\"\n    return x\n",
            &["nowhere"]
        ),
        0
    );
    assert_eq!(
        py_docs(
            "class C:\n    x = 1\n    \"\"\"attr doc\"\"\"\n",
            &["nowhere"]
        ),
        1
    );
}

#[test]
fn rust_flags_doc_comments_outside_allowed_dirs() {
    assert_eq!(rs_docs("fn f() {}\n", &["nowhere"]), 0);
    assert_eq!(rs_docs("/// doc\nfn f() {}\n", &["nowhere"]), 1);
    assert_eq!(rs_docs("//! inner\nfn f() {}\n", &["nowhere"]), 1);
    assert_eq!(rs_docs("/** d */\nfn f() {}\n", &["nowhere"]), 1);
    assert_eq!(rs_docs("/*! d */\nfn f() {}\n", &["nowhere"]), 1);
    assert_eq!(rs_docs("// n\nfn f() {}\n", &["nowhere"]), 0);
    assert_eq!(rs_docs("/* n */\nfn f() {}\n", &["nowhere"]), 0);
    assert_eq!(rs_docs("#[doc = \"hidden\"]\nfn f() {}\n", &["nowhere"]), 1);
    assert_eq!(rs_docs("#![doc = \"mod\"]\nfn f() {}\n", &["nowhere"]), 1);
}

#[test]
fn docs_allowed_dot_allows_all_paths() {
    assert_eq!(py_docs("\"\"\"module doc\"\"\"\nx = 1\n", &["."]), 0);
    assert_eq!(rs_docs("/// doc\nfn f() {}\n", &["."]), 0);
}

#[test]
fn docs_allowed_prefix_matches_file_path() {
    let root = Path::new(".");
    let mut parsed = parse_python_source("\"\"\"doc\"\"\"\nx = 1\n");
    parsed.path = PathBuf::from("docs/app.py");
    let allowed = vec!["docs".to_string()];
    assert!(collect_doc_violations(&[parsed], &[], &allowed, root).is_empty());

    let mut other = parse_python_source("\"\"\"doc\"\"\"\nx = 1\n");
    other.path = PathBuf::from("src/app.py");
    assert_eq!(
        collect_doc_violations(&[other], &[], &allowed, root).len(),
        1
    );

    let mut rs = parse_rs("/// doc\nfn f() {}\n");
    rs.path = PathBuf::from("src/lib.rs");
    assert_eq!(
        collect_doc_violations(&[], &[rs], &["src".to_string()], root).len(),
        0
    );

    let mut nested = parse_rs("/// doc\nfn f() {}\n");
    nested.path = PathBuf::from("vendor/src/lib.rs");
    assert_eq!(
        collect_doc_violations(&[], &[nested], &["src".to_string()], root).len(),
        1
    );

    let mut host = parse_python_source("\"\"\"doc\"\"\"\nx = 1\n");
    host.path = PathBuf::from("/tmp/kpopdocsPXfcfR/app.py");
    assert_eq!(
        collect_doc_violations(
            &[host],
            &[],
            &["tmp".to_string()],
            Path::new("/tmp/kpopdocsPXfcfR")
        )
        .len(),
        1
    );
}

#[test]
fn rust_skips_clap_cli_help_docs() {
    let parser = "/// about\n#[derive(Parser, Debug)]\nstruct Cli {\n    /// file\n    #[arg(long)]\n    path: String,\n}\n";
    assert_eq!(rs_docs(parser, &["nowhere"]), 0);
    let sub = "/// cmds\n#[derive(Subcommand)]\nenum C {\n    /// run check\n    Check {\n        /// root\n        path: String,\n    },\n}\n";
    assert_eq!(rs_docs(sub, &["nowhere"]), 0);
    let args = "/// opts\n#[derive(Args)]\nstruct Opts {\n    /// quiet\n    quiet: bool,\n}\n";
    assert_eq!(rs_docs(args, &["nowhere"]), 0);
    let values =
        "/// mode\n#[derive(ValueEnum)]\nenum Mode {\n    /// git commit\n    Commit,\n}\n";
    assert_eq!(rs_docs(values, &["nowhere"]), 0);
}

#[test]
fn rust_flags_docs_that_are_not_clap_help() {
    let src = "/// about\n#[derive(Parser)]\nstruct Cli {}\n/// leftover\nfn f() {}\n";
    assert_eq!(rs_docs(src, &["nowhere"]), 1);
    assert_eq!(
        rs_docs(
            "/// not clap\n#[derive(Debug)]\nstruct S {}\n",
            &["nowhere"]
        ),
        1
    );
    assert_eq!(rs_count("/// about\n#[derive(Parser)]\nstruct Cli {}\n"), 0);
}
