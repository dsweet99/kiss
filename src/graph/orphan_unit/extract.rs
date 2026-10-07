use std::collections::HashMap;
use std::path::PathBuf;

use crate::graph::orphan_unit::UnitRef;
use crate::graph::{
    ContextDependencyGraph, extract_imports_spanned, module_name_for_path, qualified_module_name,
    resolve_import,
};
use crate::parsing::ParsedFile;
use crate::rust_graph::collect_file_use_binds;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_units::extract_rust_code_units;
use crate::units::extract_code_units;

pub(super) fn collect_units(py: &[ParsedFile], rs: &[ParsedRustFile]) -> Vec<UnitRef> {
    let mut out = Vec::new();
    for parsed in py {
        let units = extract_code_units(parsed);
        for unit in &units {
            out.push(UnitRef {
                file: parsed.path.clone(),
                name: unit.name.clone(),
                kind: unit.kind,
                start_line: unit.start_line,
                end_line: unit.end_line,
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
                is_rust: true,
                trait_impl: unit.trait_impl,
            });
        }
    }
    out
}

pub(super) struct NamedBind {
    pub file: PathBuf,
    pub line: usize,
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
    let mut bare: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for name in prod.nodes.keys() {
        let bare_name = name.rsplit('.').next().unwrap_or(name).to_string();
        bare.entry(bare_name).or_default().push(name.clone());
    }
    let mut out = Vec::new();
    for parsed in py {
        let importer = module_name_for_path(ctx, &parsed.path)
            .unwrap_or_else(|| qualified_module_name(&parsed.path));
        let parent = importer.rsplit_once('.').map(|(p, _)| p);
        let imports = extract_imports_spanned(parsed.tree.root_node(), &parsed.source);
        for (spec, span) in imports {
            let Some((prefix, last)) = split_nested(&spec) else {
                continue;
            };
            let line = span.start.line;
            for resolved in resolve_import(&prefix, parent, &bare) {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
                    target_module: resolved,
                    last: last.clone(),
                });
            }
            if prod.nodes.contains_key(&prefix) {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
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
    let last_idx = rust_last_index(&prod);
    let mut out = Vec::new();
    for parsed in rs {
        let current = module_name_for_path(ctx, &parsed.path);
        for (prefix, last, line) in collect_file_use_binds(&parsed.ast) {
            if let Some(module) =
                resolve_rust_prefix_from(&prefix, current.as_deref(), &prod, &last_idx)
            {
                out.push(NamedBind {
                    file: parsed.path.clone(),
                    line,
                    target_module: module,
                    last,
                });
            }
        }
    }
    out
}

pub(super) fn rust_last_index(
    prod: &crate::graph::DependencyGraph,
) -> HashMap<String, Vec<String>> {
    let mut idx: HashMap<String, Vec<String>> = HashMap::new();
    for name in prod.nodes.keys() {
        let last = name.rsplit('.').next().unwrap_or(name);
        idx.entry(last.to_string()).or_default().push(name.clone());
    }
    for names in idx.values_mut() {
        names.sort();
        names.dedup();
    }
    idx
}

pub(super) fn resolve_rust_prefix_from(
    prefix: &str,
    current: Option<&str>,
    prod: &crate::graph::DependencyGraph,
    last_idx: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if let Some(cur) = current {
        let rel = format!("{cur}.{prefix}");
        if prod.nodes.contains_key(&rel) {
            return Some(rel);
        }
    }
    resolve_rust_prefix_indexed(prefix, prod, last_idx)
}

fn resolve_rust_prefix_indexed(
    prefix: &str,
    prod: &crate::graph::DependencyGraph,
    last_idx: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if prod.nodes.contains_key(prefix) {
        return Some(prefix.to_string());
    }
    let last = prefix.rsplit('.').next().unwrap_or(prefix);
    let hits = last_idx.get(last)?;
    (hits.len() == 1).then(|| hits[0].clone())
}

fn split_nested(spec: &str) -> Option<(String, String)> {
    spec.rsplit_once('.')
        .map(|(prefix, last)| (prefix.to_string(), last.to_string()))
}

pub(super) fn units_by_file(units: &[UnitRef]) -> HashMap<PathBuf, Vec<usize>> {
    let mut map: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (i, unit) in units.iter().enumerate() {
        map.entry(unit.file.clone()).or_default().push(i);
    }
    let aliases: Vec<(PathBuf, Vec<usize>)> = map
        .iter()
        .filter_map(|(path, idxs)| {
            let canon = crate::rust_include::canonical_path(path);
            (!map.contains_key(&canon)).then(|| (canon, idxs.clone()))
        })
        .collect();
    map.extend(aliases);
    map
}
