use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::code_roles::{CodeContextSet, FileComposition, SourceRoleIndex, SourceSpan};

use super::DependencyGraph;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeOrigin {
    pub source_span: SourceSpan,
    pub contexts: CodeContextSet,
}

#[derive(Default)]
pub struct ContextDependencyGraph {
    inner: DependencyGraph,
    node_contexts: HashMap<String, CodeContextSet>,
    edge_origins: HashMap<(String, String), Vec<EdgeOrigin>>,
}

#[derive(Default)]
pub struct RoleDependencyGraphs {
    pub python: ContextDependencyGraph,
    pub rust: ContextDependencyGraph,
}

impl ContextDependencyGraph {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn record_origin(&mut self, from: &str, to: &str, origin: EdgeOrigin) {
        self.inner.add_dependency(from, to);
        self.edge_origins
            .entry((from.to_string(), to.to_string()))
            .or_default()
            .push(origin);
    }

    pub fn register_module(&mut self, name: &str, path: PathBuf, roles: &SourceRoleIndex) {
        self.inner.get_or_create_node(name);
        self.inner
            .path_to_module
            .insert(path.clone(), name.to_string());
        self.inner.paths.insert(name.to_string(), path.clone());
        self.inner
            .compositions
            .insert(name.to_string(), roles.file_composition(&path));
        self.node_contexts
            .insert(name.to_string(), composition_contexts(roles, &path));
    }

    pub fn ensure_named_node(&mut self, name: &str, path: PathBuf, contexts: CodeContextSet) {
        self.inner.get_or_create_node(name);
        self.inner.paths.entry(name.to_string()).or_insert(path);
        self.node_contexts
            .entry(name.to_string())
            .or_insert(contexts);
    }

    #[must_use]
    pub(crate) fn inner(&self) -> &DependencyGraph {
        &self.inner
    }

    #[must_use]
    pub fn production_view(&self) -> DependencyGraph {
        filtered_view(
            &self.inner,
            &self.node_contexts,
            &self.edge_origins,
            |ctx| ctx.production,
        )
    }

    #[must_use]
    pub fn test_view(&self) -> DependencyGraph {
        filtered_view(
            &self.inner,
            &self.node_contexts,
            &self.edge_origins,
            |ctx| ctx.test,
        )
    }

    #[must_use]
    pub fn test_importers_of(&self, module: &str) -> Vec<String> {
        let Some(&idx) = self.inner.nodes.get(module) else {
            return Vec::new();
        };
        self.inner
            .graph
            .neighbors_directed(idx, petgraph::Direction::Incoming)
            .map(|i| self.inner.graph[i].clone())
            .filter(|importer| edge_is_test_only(&self.edge_origins, importer, module))
            .collect()
    }
}

pub fn module_name_for_path(graph: &ContextDependencyGraph, path: &Path) -> Option<String> {
    let canon = crate::rust_include::canonical_path(path);
    graph
        .inner
        .path_to_module
        .get(&canon)
        .or_else(|| graph.inner.path_to_module.get(path))
        .cloned()
}

pub fn path_for_module_name(graph: &ContextDependencyGraph, module: &str) -> Option<PathBuf> {
    graph.inner.paths.get(module).cloned()
}

fn composition_contexts(roles: &SourceRoleIndex, path: &std::path::Path) -> CodeContextSet {
    match roles.file_composition(path) {
        FileComposition::ProductionOnly => CodeContextSet::production_only(),
        FileComposition::TestOnly => CodeContextSet::test_only(),
        FileComposition::Mixed => CodeContextSet::both(),
    }
}

fn edge_is_test_only(
    origins: &HashMap<(String, String), Vec<EdgeOrigin>>,
    from: &str,
    to: &str,
) -> bool {
    origins
        .get(&(from.to_string(), to.to_string()))
        .is_some_and(|list| list.iter().any(|origin| origin.contexts.is_test_only()))
}

fn filtered_view(
    inner: &DependencyGraph,
    node_contexts: &HashMap<String, CodeContextSet>,
    origins: &HashMap<(String, String), Vec<EdgeOrigin>>,
    keep_ctx: fn(&CodeContextSet) -> bool,
) -> DependencyGraph {
    let mut out = DependencyGraph::new();
    out.paths.clone_from(&inner.paths);
    out.path_to_module.clone_from(&inner.path_to_module);
    out.compositions.clone_from(&inner.compositions);
    for name in inner.nodes.keys() {
        let ctx = node_contexts
            .get(name)
            .copied()
            .unwrap_or_else(CodeContextSet::production_only);
        if keep_ctx(&ctx) {
            out.get_or_create_node(name);
        }
    }
    for edge in inner.graph.edge_indices() {
        let Some((from_idx, to_idx)) = inner.graph.edge_endpoints(edge) else {
            continue;
        };
        let from = inner.graph[from_idx].clone();
        let to = inner.graph[to_idx].clone();
        if keep_edge(origins, &from, &to, keep_ctx) {
            out.add_dependency(&from, &to);
        }
    }
    out
}

fn keep_edge(
    origins: &HashMap<(String, String), Vec<EdgeOrigin>>,
    from: &str,
    to: &str,
    keep_ctx: fn(&CodeContextSet) -> bool,
) -> bool {
    match origins.get(&(from.to_string(), to.to_string())) {
        None => keep_ctx(&CodeContextSet::production_only()),
        Some(list) if list.is_empty() => keep_ctx(&CodeContextSet::production_only()),
        Some(list) => list.iter().any(|origin| keep_ctx(&origin.contexts)),
    }
}

#[cfg(test)]
mod context_graph_test {
    use super::*;
    use crate::code_roles::SourcePosition;

    #[test]
    fn test_importers_use_test_only_edge_origins() {
        let mut graph = ContextDependencyGraph::empty();
        let test_span = SourceSpan::new(SourcePosition::new(1, 0), SourcePosition::new(2, 0));
        graph.record_origin(
            "tests.mod",
            "lib",
            EdgeOrigin {
                source_span: test_span,
                contexts: CodeContextSet::test_only(),
            },
        );
        graph.record_origin(
            "app",
            "lib",
            EdgeOrigin {
                source_span: SourceSpan::whole_file(""),
                contexts: CodeContextSet::production_only(),
            },
        );
        let importers = graph.test_importers_of("lib");
        assert_eq!(importers, vec!["tests.mod".to_string()]);
        assert!(graph.production_view().imports("app", "lib"));
        assert!(!graph.production_view().imports("tests.mod", "lib"));
        assert!(graph.test_view().imports("tests.mod", "lib"));
        let _ = RoleDependencyGraphs::default();
    }
}
