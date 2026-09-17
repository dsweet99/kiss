use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::rust_parsing::ParsedRustFile;

use super::extract_rust_imports;

pub struct IncludeGraph {
    pub direct: HashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
}

impl IncludeGraph {
    #[must_use]
    pub fn transitive_from(&self, root: &Path) -> Vec<std::path::PathBuf> {
        const MAX: usize = 10_000;
        let root = crate::rust_include::canonical_path(root);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(root);
        while let Some(cur) = queue.pop_front() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(children) = self.direct.get(&cur) {
                for child in children {
                    if seen.len() >= MAX {
                        return out;
                    }
                    if child != &cur {
                        out.push(child.clone());
                        queue.push_back(child.clone());
                    }
                }
            }
        }
        out
    }
}

pub fn build_include_graph(parsed_files: &[&ParsedRustFile]) -> IncludeGraph {
    let mut direct: HashMap<std::path::PathBuf, Vec<std::path::PathBuf>> = HashMap::new();
    for parsed in parsed_files {
        let parent = crate::rust_include::canonical_path(&parsed.path);
        let imports = extract_rust_imports(&parsed.ast);
        let mut children = Vec::new();
        for lit in imports.include_literals {
            let target = crate::rust_include::resolve_include_path(&parsed.path, &lit);
            if target.is_file() {
                children.push(crate::rust_include::canonical_path(&target));
            }
        }
        if !children.is_empty() {
            direct.insert(parent, children);
        }
    }
    IncludeGraph { direct }
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

pub(crate) fn source_may_have_include_macro(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        if &bytes[i..i + 7] != b"include" {
            i += 1;
            continue;
        }
        if i > 0 && is_ident_continue(bytes[i - 1]) {
            i += 7;
            continue;
        }
        let mut j = i + 7;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'!' {
            return true;
        }
        i += 7;
    }
    false
}

fn include_targets_from_path(path: &Path) -> Vec<PathBuf> {
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    if !source_may_have_include_macro(&source) {
        return Vec::new();
    }
    let Ok(ast) = syn::parse_file(&source) else {
        return Vec::new();
    };
    extract_rust_imports(&ast)
        .include_literals
        .into_iter()
        .filter_map(|lit| {
            let inc = crate::rust_include::resolve_include_path(path, &lit);
            inc.is_file()
                .then(|| crate::rust_include::canonical_path(&inc))
        })
        .collect()
}

#[must_use]
pub fn expand_rust_files(files: Vec<PathBuf>) -> Vec<PathBuf> {
    const MAX: usize = 10_000;
    let mut set: HashSet<PathBuf> = files
        .into_iter()
        .map(|p| crate::rust_include::canonical_path(&p))
        .collect();
    loop {
        let snapshot: Vec<_> = set.iter().cloned().collect();
        let found: Vec<PathBuf> = snapshot
            .par_iter()
            .flat_map(|path| include_targets_from_path(path))
            .collect();
        let mut changed = false;
        for child in found {
            if set.len() >= MAX {
                break;
            }
            if set.insert(child) {
                changed = true;
            }
        }
        if !changed || set.len() >= MAX {
            break;
        }
    }
    let mut out: Vec<_> = set.into_iter().collect();
    out.sort();
    out
}

#[cfg(test)]
mod include_graph_tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    impl IncludeGraph {
        fn witness(direct: HashMap<PathBuf, Vec<PathBuf>>) -> Self {
            Self { direct }
        }
    }

    #[test]
    fn include_graph_transitive_from_follows_direct_edges() {
        let root = PathBuf::from("/tmp/root.rs");
        let child = PathBuf::from("/tmp/child.rs");
        let mut direct = HashMap::new();
        direct.insert(root.clone(), vec![child.clone()]);
        let graph = IncludeGraph { direct };
        assert_eq!(graph.transitive_from(&root), vec![child]);
    }

    #[test]
    fn include_graph_transitive_from_skips_self_cycles() {
        let root = PathBuf::from("/tmp/root.rs");
        let child = PathBuf::from("/tmp/child.rs");
        let mut direct = HashMap::new();
        direct.insert(root.clone(), vec![root.clone(), child.clone()]);
        direct.insert(child.clone(), vec![root.clone()]);
        let graph = IncludeGraph { direct };

        assert_eq!(graph.transitive_from(&root), vec![child, root]);
    }

    #[test]
    fn expand_rust_files_keeps_unparseable_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing.rs");

        assert_eq!(
            expand_rust_files(vec![missing.clone()]),
            vec![crate::rust_include::canonical_path(&missing)]
        );
    }

    #[test]
    fn source_may_have_include_macro_accepts_include_bang() {
        assert!(source_may_have_include_macro("include!(\"child.rs\");"));
        assert!(source_may_have_include_macro("include ! (\"child.rs\");"));
        assert!(source_may_have_include_macro("::core::include!(\"child.rs\");"));
    }

    #[test]
    fn source_may_have_include_macro_rejects_other_include_forms() {
        assert!(!source_may_have_include_macro("include_str!(\"x.txt\");"));
        assert!(!source_may_have_include_macro("include_bytes!(\"x.bin\");"));
        assert!(!source_may_have_include_macro("fn include() {}"));
        assert!(!source_may_have_include_macro("let xinclude = 1;"));
    }

    #[test]
    fn expand_rust_files_keeps_sources_without_include() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("plain.rs");
        std::fs::write(&plain, "pub fn f() {}\n").unwrap();
        assert_eq!(
            expand_rust_files(vec![plain.clone()]),
            vec![crate::rust_include::canonical_path(&plain)]
        );
    }

    #[test]
    fn witness_include_graph_type() {
        let graph = IncludeGraph::witness(HashMap::new());
        assert!(graph.transitive_from(Path::new("/tmp/root.rs")).is_empty());
    }
}
