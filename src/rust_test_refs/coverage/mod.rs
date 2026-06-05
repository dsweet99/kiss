use super::{RustCodeDefinition, RustTestRefAnalysis};
use super::dead_region::count_rs_live_branches;
use crate::rust_fn_metrics::{compute_rust_function_metrics, count_non_doc_attrs};
use crate::rust_parsing::ParsedRustFile;
use crate::test_refs::CoveringTest;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use syn::{ImplItem, Item, UseTree};

struct LocatedFn<'a> {
    inputs: &'a syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
    block: &'a syn::Block,
    attr_count: usize,
}

fn locate_fn<'a>(parsed: &'a ParsedRustFile, def: &RustCodeDefinition) -> Option<LocatedFn<'a>> {
    locate_in_items(&parsed.ast.items, def)
}

fn locate_in_items<'a>(items: &'a [Item], def: &RustCodeDefinition) -> Option<LocatedFn<'a>> {
    for item in items {
        if let Some(found) = locate_in_item(item, def) {
            return Some(found);
        }
    }
    None
}

fn locate_in_item<'a>(item: &'a Item, def: &RustCodeDefinition) -> Option<LocatedFn<'a>> {
    match item {
        Item::Fn(f) if f.sig.ident.span().start().line == def.line => Some(LocatedFn {
            inputs: &f.sig.inputs,
            block: &f.block,
            attr_count: count_non_doc_attrs(&f.attrs),
        }),
        Item::Impl(i) => {
            for impl_item in &i.items {
                if let ImplItem::Fn(m) = impl_item
                    && m.sig.ident.span().start().line == def.line
                {
                    return Some(LocatedFn {
                        inputs: &m.sig.inputs,
                        block: &m.block,
                        attr_count: count_non_doc_attrs(&m.attrs),
                    });
                }
            }
            None
        }
        Item::Mod(m) => {
            if let Some((_, nested)) = &m.content {
                locate_in_items(nested, def)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn test_fn_branches(parsed: &ParsedRustFile, test_id: &str) -> usize {
    for item in &parsed.ast.items {
        if let Item::Fn(f) = item
            && f.sig.ident == test_id
        {
            return count_rs_live_branches(&f.block);
        }
    }
    0
}

fn rs_module_import_surface_credit(
    analysis: &RustTestRefAnalysis,
    def: &RustCodeDefinition,
    metrics: &crate::rust_fn_metrics::RustFunctionMetrics,
    covering: &[(PathBuf, String)],
    parsed_by_path: &HashMap<PathBuf, &ParsedRustFile>,
) -> Option<f64> {
    if def.impl_for_type.is_some() {
        return None;
    }
    let sibling_count = analysis
        .definitions
        .iter()
        .filter(|d| d.file == def.file && d.impl_for_type.is_none())
        .count()
        .max(1);
    let def_mass = metrics.statements.max(1) as f64;
    let b_ref = covering
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_fn_branches(p, test_id))
        .min()
        .unwrap_or(0);
    if b_ref == 0 {
        return Some(0.0);
    }
    Some((b_ref as f64 / (sibling_count as f64 * def_mass)).min(1.0))
}

fn rs_import_surface_credit(
    analysis: &RustTestRefAnalysis,
    def: &RustCodeDefinition,
    metrics: &crate::rust_fn_metrics::RustFunctionMetrics,
    covering: &[(PathBuf, String)],
    parsed_by_path: &HashMap<PathBuf, &ParsedRustFile>,
) -> f64 {
    let type_name = match def.impl_for_type.as_deref() {
        Some(t) => t,
        None => return 0.0,
    };
    let method_count = analysis
        .definitions
        .iter()
        .filter(|d| {
            d.file == def.file && d.impl_for_type.as_deref() == Some(type_name)
        })
        .count()
        .max(1);
    let def_branches = metrics.branches.max(1);
    let b_ref = covering
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_fn_branches(p, test_id))
        .min()
        .unwrap_or(0);
    if b_ref == 0 {
        return 0.0;
    }
    (b_ref as f64 / (method_count as f64 * def_branches as f64)).min(1.0)
}

fn rs_branch_credit(
    metrics: &crate::rust_fn_metrics::RustFunctionMetrics,
    covering: &[(PathBuf, String)],
    parsed_by_path: &HashMap<PathBuf, &ParsedRustFile>,
) -> f64 {
    if metrics.branches == 0 {
        return if metrics.statements > 15 { 0.15 } else { 1.0 };
    }
    let b_ref = covering
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_fn_branches(p, test_id))
        .min()
        .unwrap_or(0);
    if b_ref == 0 && covering.is_empty() {
        return 0.0;
    }
    let b_eff = b_ref.max(1);
    let branch_denom = if metrics.statements > 15 && b_eff <= 2 {
        (metrics.statements as f64 / 3.0)
            .max(metrics.branches as f64)
            .max(1.0)
    } else {
        metrics.branches.max(1) as f64
    };
    if branch_denom <= b_eff as f64 {
        1.0
    } else {
        b_eff as f64 / branch_denom
    }
}

fn impl_type_covering_tests(
    analysis: &RustTestRefAnalysis,
    unref_set: &HashSet<(&PathBuf, &str)>,
    def: &RustCodeDefinition,
) -> Option<Vec<CoveringTest>> {
    let type_name = def.impl_for_type.as_ref()?;
    for sibling in &analysis.definitions {
        if sibling.file != def.file {
            continue;
        }
        if sibling.impl_for_type.as_deref() != Some(type_name.as_str()) {
            continue;
        }
        if unref_set.contains(&(&sibling.file, sibling.name.as_str())) {
            continue;
        }
        if let Some(tests) = analysis
            .coverage_map
            .get(&(sibling.file.clone(), sibling.name.clone()))
        {
            return Some(tests.clone());
        }
    }
    None
}

fn rs_weighted_definition_credit(
    analysis: &RustTestRefAnalysis,
    unref_set: &HashSet<(&PathBuf, &str)>,
    def: &RustCodeDefinition,
    metrics: &crate::rust_fn_metrics::RustFunctionMetrics,
    parsed_by_path: &HashMap<PathBuf, &ParsedRustFile>,
) -> f64 {
    if unref_set.contains(&(&def.file, def.name.as_str())) {
        if let Some(covering) = impl_type_covering_tests(analysis, unref_set, def) {
            return rs_import_surface_credit(analysis, def, metrics, &covering, parsed_by_path);
        }
        return 0.0;
    }
    let covering = analysis
        .coverage_map
        .get(&(def.file.clone(), def.name.clone()))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if covering.is_empty() {
        return 0.0;
    }
    if def.impl_for_type.is_none()
        && !analysis.call_references.contains(&def.name)
        && !analysis.propagated_references.contains(&def.name)
    {
        return match rs_module_import_surface_credit(
            analysis,
            def,
            metrics,
            covering,
            parsed_by_path,
        ) {
            Some(c) if c > 0.0 => c,
            _ => {
                let min_b = covering
                    .iter()
                    .filter_map(|(path, test_id)| {
                        parsed_by_path.get(path).map(|p| (p, test_id.as_str()))
                    })
                    .map(|(p, test_id)| test_fn_branches(p, test_id))
                    .min()
                    .unwrap_or(0);
                let c = rs_branch_credit(metrics, covering, parsed_by_path);
                if min_b == 0 && metrics.statements <= 15 {
                    c.min(0.15)
                } else {
                    c
                }
            }
        };
    }
    rs_branch_credit(metrics, covering, parsed_by_path)
}

fn accumulate_rs_weighted_mass(
    by_file: &mut HashMap<PathBuf, (f64, f64)>,
    def: &RustCodeDefinition,
    stmts: usize,
    credit: f64,
) {
    let entry = by_file.entry(def.file.clone()).or_default();
    if credit == 0.0 {
        if def.impl_for_type.is_some() {
            entry.1 += stmts as f64;
        }
        return;
    }
    entry.0 += stmts as f64 * credit;
    entry.1 += stmts as f64;
}

fn flatten_use_tree_names(tree: &UseTree) -> Vec<String> {
    match tree {
        UseTree::Name(n) => vec![n.ident.to_string()],
        UseTree::Rename(r) => vec![r.ident.to_string()],
        UseTree::Glob(_) => Vec::new(),
        UseTree::Path(p) => flatten_use_tree_names(&p.tree),
        UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(flatten_use_tree_names)
            .collect(),
    }
}

fn export_name_covered(
    name: &str,
    analysis: &RustTestRefAnalysis,
    unref_set: &HashSet<(&PathBuf, &str)>,
) -> bool {
    let Some(def) = analysis.definitions.iter().find(|d| d.name == name) else {
        return true;
    };
    !unref_set.contains(&(&def.file, name))
}

fn accumulate_pub_use_export_mass(
    parsed: &ParsedRustFile,
    analysis: &RustTestRefAnalysis,
    unref_set: &HashSet<(&PathBuf, &str)>,
    by_file: &mut HashMap<PathBuf, (f64, f64)>,
) {
    for item in &parsed.ast.items {
        let Item::Use(u) = item else {
            continue;
        };
        if !matches!(u.vis, syn::Visibility::Public(_)) {
            continue;
        }
        for name in flatten_use_tree_names(&u.tree) {
            let credit = f64::from(export_name_covered(&name, analysis, unref_set));
            let entry = by_file.entry(parsed.path.clone()).or_default();
            entry.0 += credit;
            entry.1 += 1.0;
        }
    }
}

pub fn compute_rs_weighted_file_pcts(
    analysis: &RustTestRefAnalysis,
    parsed_files: &[&ParsedRustFile],
) -> HashMap<PathBuf, usize> {
    let parsed_by_path: HashMap<PathBuf, &ParsedRustFile> =
        parsed_files.iter().map(|p| (p.path.clone(), *p)).collect();
    let unref_set: HashSet<(&PathBuf, &str)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();

    let mut by_file: HashMap<PathBuf, (f64, f64)> = HashMap::new();
    let mut defs_per_file: HashMap<PathBuf, usize> = HashMap::new();
    for def in &analysis.definitions {
        *defs_per_file.entry(def.file.clone()).or_default() += 1;
        let Some(parsed) = parsed_by_path.get(&def.file) else {
            continue;
        };
        let Some(located) = locate_fn(parsed, def) else {
            continue;
        };
        let metrics = compute_rust_function_metrics(located.inputs, located.block, located.attr_count);
        let stmts = metrics.statements.max(1);
        let mut credit = rs_weighted_definition_credit(
            analysis,
            &unref_set,
            def,
            &metrics,
            &parsed_by_path,
        );
        if credit > 0.0 && metrics.statements > 15 && credit < 0.45 {
            credit = 0.45;
        }
        accumulate_rs_weighted_mass(&mut by_file, def, stmts, credit);
    }

    for parsed in parsed_files {
        accumulate_pub_use_export_mass(parsed, analysis, &unref_set, &mut by_file);
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let result: HashMap<PathBuf, usize> = by_file
        .into_iter()
        .map(|(file, (covered_mass, total_mass))| {
            let pct = if total_mass > 0.0 {
                ((covered_mass / total_mass) * 100.0).round() as usize
            } else {
                0
            };
            (file, pct)
        })
        .collect();
    result
}

#[cfg(test)]
mod inline_tests;
