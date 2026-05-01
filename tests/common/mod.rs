#![allow(dead_code)]

use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// `~/.cache/kiss` rooted under a given fake `HOME` (used by integration
/// tests that drive the binary with `env("HOME", ...)`).
pub fn cache_dir_under(home: &Path) -> PathBuf {
    home.join(".cache").join("kiss")
}

/// True for files matching `check_full_*.bin` (the full-check analyze
/// cache file). Used as the predicate for [`list_full_check_cache_files`].
pub fn is_full_check_cache_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("check_full_")
        && Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
}

/// Sorted list of `check_full_*.bin` files in `~/.cache/kiss` under the
/// given fake `HOME`. Returns an empty `Vec` if the cache dir does not
/// exist yet.
pub fn list_full_check_cache_files(home: &Path) -> Vec<PathBuf> {
    let dir = cache_dir_under(home);
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| is_full_check_cache_file(p))
        .collect();
    out.sort();
    out
}

pub fn parse_python_source(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, tmp.path()).expect("should parse temp source")
}

pub fn first_function_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "function_definition"
        {
            return node;
        }
    }

    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "decorated_definition"
        {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "function_definition" {
                    return child;
                }
            }
        }
    }

    panic!("function_definition");
}

pub fn first_function_or_async_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "function_definition" || n.kind() == "async_function_definition")
        .expect("function_definition")
}
