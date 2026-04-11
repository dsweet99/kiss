use crate::graph::DependencyGraph;
use crate::rust_fn_metrics::{
    compute_rust_file_metrics, compute_rust_function_metrics, count_non_doc_attrs, is_cfg_test_mod,
    RustFunctionMetrics,
};
use crate::rust_parsing::ParsedRustFile;
use syn::{ImplItem, Item};

use super::types::UnitMetrics;
use super::file_unit_metrics;

pub(crate) struct RustFnMethodPush<'a> {
    pub file: &'a str,
    pub name: String,
    pub line: usize,
    pub kind: &'static str,
    pub m: &'a RustFunctionMetrics,
}

pub(crate) fn push_rust_fn_or_method_unit(units: &mut Vec<UnitMetrics>, p: RustFnMethodPush<'_>) {
    units.push(UnitMetrics {
        file: p.file.to_string(),
        name: p.name,
        kind: p.kind,
        line: p.line,
        statements: Some(p.m.statements),
        arguments: Some(p.m.arguments),
        args_positional: Some(p.m.arguments),
        args_keyword_only: Some(0),
        indentation: Some(p.m.max_indentation),
        nested_depth: Some(p.m.nested_function_depth),
        branches: Some(p.m.branches),
        returns: Some(p.m.returns),
        return_values: None,
        locals: Some(p.m.local_variables),
        methods: None,
        lines: None,
        imports: None,
        fan_in: None,
        fan_out: None,
        indirect_deps: None,
        dependency_depth: None,
    });
}

pub fn collect_detailed_rs(
    parsed_files: &[&ParsedRustFile],
    graph: Option<&DependencyGraph>,
) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let fm = compute_rust_file_metrics(parsed);
        let lines = parsed.source.lines().count();
        units.push(file_unit_metrics(&parsed.path, lines, fm.imports, graph));
        collect_detailed_from_items(&parsed.ast.items, &parsed.path.display().to_string(), &mut units);
    }
    units
}

fn push_top_level_fn(f: &syn::ItemFn, file: &str, units: &mut Vec<UnitMetrics>) {
    let m = compute_rust_function_metrics(
        &f.sig.inputs,
        &f.block,
        count_non_doc_attrs(&f.attrs),
    );
    push_rust_fn_or_method_unit(
        units,
        RustFnMethodPush {
            file,
            name: f.sig.ident.to_string(),
            line: f.sig.ident.span().start().line,
            kind: "function",
            m: &m,
        },
    );
}

fn push_impl_block(i: &syn::ItemImpl, file: &str, units: &mut Vec<UnitMetrics>) {
    let name = get_impl_name(i);
    let mcnt = i
        .items
        .iter()
        .filter(|ii| matches!(ii, ImplItem::Fn(_)))
        .count();
    units.push(UnitMetrics {
        file: file.to_string(),
        name,
        kind: "impl",
        line: i.impl_token.span.start().line,
        statements: None,
        arguments: None,
        args_positional: None,
        args_keyword_only: None,
        indentation: None,
        nested_depth: None,
        branches: None,
        returns: None,
        return_values: None,
        locals: None,
        methods: Some(mcnt),
        lines: None,
        imports: None,
        fan_in: None,
        fan_out: None,
        indirect_deps: None,
        dependency_depth: None,
    });
    for ii in &i.items {
        if let ImplItem::Fn(m) = ii {
            push_impl_method(m, file, units);
        }
    }
}

fn push_impl_method(m: &syn::ImplItemFn, file: &str, units: &mut Vec<UnitMetrics>) {
    let metrics = compute_rust_function_metrics(
        &m.sig.inputs,
        &m.block,
        count_non_doc_attrs(&m.attrs),
    );
    push_rust_fn_or_method_unit(
        units,
        RustFnMethodPush {
            file,
            name: m.sig.ident.to_string(),
            line: m.sig.ident.span().start().line,
            kind: "method",
            m: &metrics,
        },
    );
}

pub(crate) fn collect_detailed_from_items(items: &[Item], file: &str, units: &mut Vec<UnitMetrics>) {
    for item in items {
        match item {
            Item::Fn(f) => push_top_level_fn(f, file, units),
            Item::Impl(i) => push_impl_block(i, file, units),
            Item::Mod(m) => {
                if !is_cfg_test_mod(m)
                    && let Some((_, inner)) = &m.content
                {
                    collect_detailed_from_items(inner, file, units);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn get_impl_name(i: &syn::ItemImpl) -> String {
    if let syn::Type::Path(tp) = &*i.self_ty {
        tp.path
            .segments
            .last()
            .map_or_else(|| "<impl>".to_string(), |s| s.ident.to_string())
    } else {
        "<impl>".to_string()
    }
}
