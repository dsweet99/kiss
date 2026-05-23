use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::rust_parsing::{ParsedRustFile, parse_rust_file};

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

#[must_use]
pub fn expand_rust_files(files: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    const MAX: usize = 10_000;
    let mut set: HashSet<std::path::PathBuf> = files
        .into_iter()
        .map(|p| crate::rust_include::canonical_path(&p))
        .collect();
    loop {
        let snapshot: Vec<_> = set.iter().cloned().collect();
        let mut changed = false;
        for path in snapshot {
            if set.len() >= MAX {
                break;
            }
            let Ok(parsed) = parse_rust_file(&path) else {
                continue;
            };
            for lit in extract_rust_imports(&parsed.ast).include_literals {
                let inc = crate::rust_include::resolve_include_path(&path, &lit);
                if inc.is_file() {
                    let c = crate::rust_include::canonical_path(&inc);
                    if set.insert(c) {
                        changed = true;
                    }
                }
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
    use std::path::PathBuf;

    #[test]
    fn include_graph_transitive_from_follows_direct_edges() {
        let root = PathBuf::from("/tmp/root.rs");
        let child = PathBuf::from("/tmp/child.rs");
        let mut direct = HashMap::new();
        direct.insert(root.clone(), vec![child.clone()]);
        let graph = IncludeGraph { direct };
        assert_eq!(graph.transitive_from(&root), vec![child]);
    }
}
