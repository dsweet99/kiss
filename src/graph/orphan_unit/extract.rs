use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::graph::orphan_unit::UnitRef;
use crate::graph::{
    ContextDependencyGraph, extract_imports_for_cache, module_name_for_path, qualified_module_name,
    resolve_import,
};
use crate::parsing::ParsedFile;
use crate::rust_graph::collect_file_use_binds;
use crate::rust_parsing::ParsedRustFile;
use crate::units::{CodeUnitKind, extract_code_units};
use crate::rust_units::extract_rust_code_units;
use syn::visit::Visit;

pub(super) fn collect_units(py: &[ParsedFile], rs: &[ParsedRustFile]) -> Vec<UnitRef> {
    let mut out = Vec::new();
    for parsed in py {
        let units = extract_code_units(parsed);
        for unit in &units {
            let parent_type = enclosing_class(&units, unit);
            out.push(UnitRef {
                file: parsed.path.clone(),
                name: unit.name.clone(),
                kind: unit.kind,
                start_line: unit.start_line,
                end_line: unit.end_line,
                parent_type,
                is_rust: false,
                trait_impl: false,
            });
        }
    }
    for parsed in rs {
        for unit in extract_rust_code_units(parsed) {
            out.push(UnitRef {
                file: parsed.path.clone(),
                name: unit.name,
                kind: unit.kind,
                start_line: unit.start_line,
                end_line: unit.end_line,
                parent_type: unit.parent_type,
                is_rust: true,
                trait_impl: unit.trait_impl,
            });
        }
    }
    out
}

fn enclosing_class(
    units: &[crate::units::CodeUnit],
    child: &crate::units::CodeUnit,
) -> Option<String> {
    if child.kind != CodeUnitKind::Method {
        return None;
    }
    units
        .iter()
        .filter(|unit| {
            unit.kind == CodeUnitKind::Class
                && unit.start_line <= child.start_line
                && child.end_line <= unit.end_line
        })
        .min_by_key(|unit| unit.end_line.saturating_sub(unit.start_line))
        .map(|unit| unit.name.clone())
}

pub(super) struct NamedBind {
    pub target_module: String,
    pub last: String,
}

pub(super) fn collect_binds(
    py: &[ParsedFile],
    rs: &[ParsedRustFile],
    py_ctx: &ContextDependencyGraph,
    rs_ctx: &ContextDependencyGraph,
) -> Vec<NamedBind> {
    let mut out = Vec::new();
    out.extend(python_binds(py, py_ctx));
    out.extend(rust_binds(rs, rs_ctx));
    out.extend(crate::graph::orphan_unit::name_refs::collect_name_binds(
        py, rs, py_ctx, rs_ctx,
    ));
    out
}

fn python_binds(py: &[ParsedFile], ctx: &ContextDependencyGraph) -> Vec<NamedBind> {
    let prod = ctx.production_view();
    let mut bare: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for name in prod.nodes.keys() {
        let bare_name = name.rsplit('.').next().unwrap_or(name).to_string();
        bare.entry(bare_name).or_default().push(name.clone());
    }
    let mut out = Vec::new();
    for parsed in py {
        let importer = module_name_for_path(ctx, &parsed.path)
            .unwrap_or_else(|| qualified_module_name(&parsed.path));
        let parent = importer.rsplit_once('.').map(|(p, _)| p);
        let imports = extract_imports_for_cache(parsed.tree.root_node(), &parsed.source);
        for spec in imports {
            let Some((prefix, last)) = split_nested(&spec) else {
                continue;
            };
            for resolved in resolve_import(&prefix, parent, &bare) {
                out.push(NamedBind {
                    target_module: resolved,
                    last: last.clone(),
                });
            }
            if prod.nodes.contains_key(&prefix) {
                out.push(NamedBind {
                    target_module: prefix,
                    last,
                });
            }
        }
    }
    out
}

fn rust_binds(rs: &[ParsedRustFile], ctx: &ContextDependencyGraph) -> Vec<NamedBind> {
    let prod = ctx.production_view();
    let mut out = Vec::new();
    for parsed in rs {
        let current = module_name_for_path(ctx, &parsed.path);
        for (prefix, last) in collect_file_use_binds(&parsed.ast) {
            if let Some(module) = resolve_rust_prefix_from(&prefix, current.as_deref(), &prod) {
                out.push(NamedBind {
                    target_module: module,
                    last,
                });
            }
        }
    }
    out
}

pub(super) fn resolve_rust_prefix_from(
    prefix: &str,
    current: Option<&str>,
    prod: &crate::graph::DependencyGraph,
) -> Option<String> {
    if let Some(cur) = current {
        let rel = format!("{cur}.{prefix}");
        if prod.nodes.contains_key(&rel) {
            return Some(rel);
        }
    }
    resolve_rust_prefix(prefix, prod)
}

pub(super) fn resolve_rust_prefix(prefix: &str, prod: &crate::graph::DependencyGraph) -> Option<String> {
    if prod.nodes.contains_key(prefix) {
        return Some(prefix.to_string());
    }
    let last = prefix.rsplit('.').next().unwrap_or(prefix);
    let mut hits: Vec<String> = prod
        .nodes
        .keys()
        .filter(|name| *name == last || name.ends_with(&format!(".{last}")))
        .cloned()
        .collect();
    hits.sort();
    hits.dedup();
    (hits.len() == 1).then(|| hits.remove(0))
}

fn split_nested(spec: &str) -> Option<(String, String)> {
    spec.rsplit_once('.').map(|(prefix, last)| {
        (prefix.to_string(), last.to_string())
    })
}

pub(super) fn rust_coverage_off(rs: &[ParsedRustFile]) -> HashSet<(PathBuf, String, usize)> {
    let mut out = HashSet::new();
    for parsed in rs {
        let mut visitor = CovOffVisitor {
            path: parsed.path.clone(),
            out: &mut out,
        };
        visitor.visit_file(&parsed.ast);
    }
    out
}

struct CovOffVisitor<'a> {
    path: PathBuf,
    out: &'a mut HashSet<(PathBuf, String, usize)>,
}

impl CovOffVisitor<'_> {
    fn record(&mut self, name: String, line: usize, attrs: &[syn::Attribute]) {
        if crate::coverage_off_attrs(attrs) {
            self.out.insert((self.path.clone(), name, line));
        }
    }
}

impl<'ast> Visit<'ast> for CovOffVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.record(
            node.sig.ident.to_string(),
            node.sig.ident.span().start().line,
            &node.attrs,
        );
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.record(
            node.sig.ident.to_string(),
            node.sig.ident.span().start().line,
            &node.attrs,
        );
        syn::visit::visit_impl_item_fn(self, node);
    }
}

pub(super) fn file_key<'a>(
    map: &'a std::collections::BTreeMap<PathBuf, BTreeSet<usize>>,
    path: &Path,
) -> Option<&'a BTreeSet<usize>> {
    map.get(path).or_else(|| {
        map.iter()
            .find(|(p, _)| crate::rust_include::canonical_path(p) == crate::rust_include::canonical_path(path))
            .map(|(_, set)| set)
    })
}
