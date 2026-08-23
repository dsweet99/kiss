use crate::code_roles::{SourceRoleIndex, SourceSpan, contexts_for_span};
use crate::graph::{ContextDependencyGraph, DependencyGraph, EdgeOrigin};
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::Path;
mod extract_imports;
mod include_graph;
mod resolve;

#[cfg(test)]
pub(crate) use resolve::{qualify_child_module, resolve_import};

#[cfg(test)]
pub(crate) use extract_imports::{
    extract_imports_from_block, extract_imports_from_expr, extract_imports_from_items,
};

pub use include_graph::{IncludeGraph, build_include_graph, expand_rust_files};

#[cfg(test)]
mod tests;

pub(crate) fn qualified_rust_module_name(path: &Path) -> String {
    use std::path::Component;

    let stem = path
        .file_stem()
        .map_or("unknown", |s| s.to_str().unwrap_or("unknown"));

    let mut dirs: Vec<String> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(os) => os.to_str().map(std::string::ToString::to_string),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(pos) = dirs.iter().rposition(|d| d == "src" || d == "tests") {
        dirs = dirs[(pos + 1)..].to_vec();
    }

    if path.is_absolute() && dirs.len() > 2 {
        dirs = dirs[(dirs.len() - 2)..].to_vec();
    }

    if stem == "mod" {
        if dirs.is_empty() {
            return "mod".to_string();
        }
        return dirs.join(".");
    }

    if dirs.is_empty() {
        stem.to_string()
    } else {
        format!("{}.{}", dirs.join("."), stem)
    }
}

pub fn build_rust_dependency_graph(parsed_files: &[&ParsedRustFile]) -> DependencyGraph {
    build_rust_dependency_graph_with_roles(parsed_files, None)
}

pub fn build_rust_dependency_graph_with_roles(
    parsed_files: &[&ParsedRustFile],
    roles: Option<&SourceRoleIndex>,
) -> DependencyGraph {
    match roles {
        Some(roles) => build_rust_context_graph(parsed_files, roles).production_view(),
        None => build_rust_context_graph(parsed_files, &SourceRoleIndex::empty()).production_view(),
    }
}

pub fn build_rust_context_graph(
    parsed_files: &[&ParsedRustFile],
    roles: &SourceRoleIndex,
) -> ContextDependencyGraph {
    let mut ctx = ContextDependencyGraph::empty();
    let mut internal_modules = HashSet::new();
    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();
    for parsed in parsed_files {
        register_parsed_module(
            &mut ctx,
            parsed,
            roles,
            &mut internal_modules,
            &mut bare_to_qualified,
        );
    }
    for parsed in parsed_files {
        add_file_origins(
            &mut ctx,
            parsed,
            roles,
            &internal_modules,
            &bare_to_qualified,
        );
    }
    ctx
}

fn register_parsed_module(
    ctx: &mut ContextDependencyGraph,
    parsed: &ParsedRustFile,
    roles: &SourceRoleIndex,
    internal_modules: &mut HashSet<String>,
    bare_to_qualified: &mut HashMap<String, Vec<String>>,
) {
    let qualified = qualified_rust_module_name(&parsed.path);
    let bare = parsed.path.file_stem().map_or_else(
        || String::from("unknown"),
        |s| s.to_string_lossy().into_owned(),
    );
    internal_modules.insert(qualified.clone());
    bare_to_qualified
        .entry(bare)
        .or_default()
        .push(qualified.clone());
    ctx.register_module(
        &qualified,
        crate::rust_include::canonical_path(&parsed.path),
        roles,
    );
}

fn add_file_origins(
    ctx: &mut ContextDependencyGraph,
    parsed: &ParsedRustFile,
    roles: &SourceRoleIndex,
    internal_modules: &HashSet<String>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
) {
    OriginEnv {
        ctx,
        parsed,
        module_name: qualified_rust_module_name(&parsed.path),
        roles,
        internal_modules,
        bare_to_qualified,
    }
    .add_imports();
}

struct OriginEnv<'a> {
    ctx: &'a mut ContextDependencyGraph,
    parsed: &'a ParsedRustFile,
    module_name: String,
    roles: &'a SourceRoleIndex,
    internal_modules: &'a HashSet<String>,
    bare_to_qualified: &'a HashMap<String, Vec<String>>,
}

impl OriginEnv<'_> {
    fn add_imports(&mut self) {
        let imports = extract_rust_imports(&self.parsed.ast);
        self.includes(&imports.include_spans);
        self.mods(&imports.mod_spans);
        self.uses(&imports.use_spans);
    }

    fn includes(&mut self, include_spans: &[(String, String, SourceSpan)]) {
        for (suffix, lit, span) in include_spans {
            let from = qualify_owner(&self.module_name, suffix);
            self.ensure(&from, *span);
            let target = crate::rust_include::resolve_include_path(&self.parsed.path, lit);
            let key = crate::rust_include::canonical_path(&target);
            if let Some(child_module) = self.ctx.inner().path_to_module.get(&key).cloned() {
                self.edge(&from, &child_module, *span);
            }
        }
    }

    fn mods(&mut self, mod_spans: &[(String, String, Option<String>, SourceSpan)]) {
        for (suffix, child, path_lit, span) in mod_spans {
            let from = qualify_owner(&self.module_name, suffix);
            self.ensure(&from, *span);
            if let Some(lit) = path_lit {
                let target = crate::rust_include::resolve_include_path(&self.parsed.path, lit);
                let key = crate::rust_include::canonical_path(&target);
                if let Some(child_module) = self.ctx.inner().path_to_module.get(&key).cloned() {
                    self.edge(&from, &child_module, *span);
                }
                continue;
            }
            let expected = resolve::qualify_child_module(&from, child);
            if self.internal_modules.contains(&expected) {
                self.edge(&from, &expected, *span);
            } else {
                self.emit_resolved(&from, child, *span);
            }
        }
    }

    fn uses(&mut self, use_spans: &[(String, String, SourceSpan)]) {
        for (suffix, import, span) in use_spans {
            let from = qualify_owner(&self.module_name, suffix);
            self.emit_resolved(&from, import, *span);
        }
    }

    fn emit_resolved(&mut self, from: &str, import: &str, span: SourceSpan) {
        self.ensure(from, span);
        for target in resolve::resolve_import_targets(
            import,
            &self.module_name,
            self.internal_modules,
            self.bare_to_qualified,
        ) {
            self.edge(from, &target, span);
        }
    }

    fn ensure(&mut self, from: &str, span: SourceSpan) {
        let contexts = contexts_for_span(self.roles, &self.parsed.path, span);
        self.ctx.ensure_named_node(
            from,
            crate::rust_include::canonical_path(&self.parsed.path),
            contexts,
        );
    }

    fn edge(&mut self, from: &str, to: &str, span: SourceSpan) {
        self.ctx.record_origin(
            from,
            to,
            EdgeOrigin {
                source_span: span,
                contexts: contexts_for_span(self.roles, &self.parsed.path, span),
            },
        );
    }
}

fn qualify_owner(file_module: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        file_module.to_string()
    } else {
        format!("{file_module}::{suffix}")
    }
}

pub(crate) struct RustImports {
    #[allow(dead_code)]
    pub(crate) use_roots: Vec<String>,
    #[allow(dead_code)]
    pub(crate) mod_decls: Vec<String>,
    pub(crate) include_literals: Vec<String>,
    pub(crate) use_spans: Vec<(String, String, SourceSpan)>,
    pub(crate) mod_spans: Vec<(String, String, Option<String>, SourceSpan)>,
    pub(crate) include_spans: Vec<(String, String, SourceSpan)>,
}

pub(crate) fn extract_rust_imports(ast: &syn::File) -> RustImports {
    let mut use_roots = Vec::new();
    let mut mod_decls = Vec::new();
    let mut include_literals = Vec::new();
    let mut use_spans = Vec::new();
    let mut mod_spans = Vec::new();
    let mut include_spans = Vec::new();
    extract_imports::extract_imports_from_items_skip(
        &ast.items,
        &mut extract_imports::ImportSink {
            use_roots: &mut use_roots,
            mod_decls: &mut mod_decls,
            include_literals: &mut include_literals,
            use_spans: &mut use_spans,
            mod_spans: &mut mod_spans,
            include_spans: &mut include_spans,
            module_suffix: String::new(),
        },
    );
    RustImports {
        use_roots,
        mod_decls,
        include_literals,
        use_spans,
        mod_spans,
        include_spans,
    }
}

pub(crate) fn collect_use_paths(tree: &syn::UseTree, imports: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            let crate_name = path.ident.to_string();
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
        }
        syn::UseTree::Name(name) => {
            let crate_name = name.ident.to_string();
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
        }
        syn::UseTree::Rename(rename) => {
            let crate_name = rename.ident.to_string();
            if !matches!(crate_name.as_str(), "self" | "super" | "crate") {
                imports.push(crate_name);
            }
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_paths(item, imports);
            }
        }
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl RustImports {
        fn witness() -> Self {
            Self {
                use_roots: vec![],
                mod_decls: vec![],
                include_literals: vec![],
                use_spans: vec![],
                mod_spans: vec![],
                include_spans: vec![],
            }
        }
    }

    #[test]
    fn witness_rust_imports() {
        let _ = RustImports::witness();
        let parsed = syn::parse_file("use std;").unwrap();
        let imports = extract_rust_imports(&parsed);
        assert_eq!(imports.use_roots, vec!["std".to_string()]);
    }
}
