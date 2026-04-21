use crate::graph::DependencyGraph;
use crate::rust_fn_metrics::{
    compute_rust_file_metrics, compute_rust_function_metrics, count_non_doc_attrs, is_cfg_test_mod,
    RustFunctionMetrics,
};
use crate::rust_parsing::ParsedRustFile;
use syn::{ImplItem, Item};

use super::types::UnitMetrics;
use super::{FileScopeMetrics, file_unit_metrics};

pub(crate) struct RustFnMethodPush<'a> {
    pub file: &'a str,
    pub name: String,
    pub line: usize,
    pub kind: &'static str,
    pub m: &'a RustFunctionMetrics,
}

pub(crate) fn push_rust_fn_or_method_unit(units: &mut Vec<UnitMetrics>, p: RustFnMethodPush<'_>) {
    let mut u = UnitMetrics::new(p.file.to_string(), p.name, p.kind, p.line);
    u.statements = Some(p.m.statements);
    u.arguments = Some(p.m.arguments);
    u.args_positional = Some(p.m.arguments);
    u.args_keyword_only = Some(0);
    u.indentation = Some(p.m.max_indentation);
    u.nested_depth = Some(p.m.nested_function_depth);
    u.branches = Some(p.m.branches);
    u.returns = Some(p.m.returns);
    u.locals = Some(p.m.local_variables);
    u.boolean_parameters = Some(p.m.bool_parameters);
    u.annotations = Some(p.m.attributes);
    u.calls = Some(p.m.calls);
    units.push(u);
}

pub fn collect_detailed_rs(
    parsed_files: &[&ParsedRustFile],
    graph: Option<&DependencyGraph>,
) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let fm = compute_rust_file_metrics(parsed);
        let lines = parsed.source.lines().count();
        units.push(file_unit_metrics(
            &parsed.path,
            FileScopeMetrics {
                lines,
                imports: fm.imports,
                statements: fm.statements,
                functions: fm.functions,
                interface_types: fm.interface_types,
                concrete_types: fm.concrete_types,
            },
            graph,
        ));
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
    let mut u = UnitMetrics::new(file.to_string(), name, "impl", i.impl_token.span.start().line);
    u.methods = Some(mcnt);
    units.push(u);
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
                if is_cfg_test_mod(m) {
                    // Skip test modules; they can contain large fixtures that would
                    // distort summary stats.
                } else if let Some((_, inner)) = &m.content {
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

#[cfg(test)]
mod rust_coverage {
    use super::*;
    use std::io::Write;

    #[test]
    fn touch_for_coverage() {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        write!(tmp, "fn foo() {{ let x = 1; }}\nstruct S;\nimpl S {{ fn m(&self) {{ let y = 2; }} }}").unwrap();
        let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
        let refs: Vec<&crate::rust_parsing::ParsedRustFile> = vec![&parsed];
        let units = collect_detailed_rs(&refs, None);
        assert!(units.len() >= 3, "expected file + function + impl units, got {}", units.len());
        assert!(units.iter().any(|u| u.kind == "function"), "expected a function unit");
        assert!(units.iter().any(|u| u.kind == "impl"), "expected an impl unit");
    }

    #[test]
    fn push_top_level_fn_metrics() {
        let code = "fn compute(a: i32, b: i32) -> i32 { let c = a + b; c }";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_items(&ast.items, "test.rs", &mut units);

        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.kind, "function");
        assert_eq!(u.name, "compute");
        assert_eq!(u.arguments.unwrap(), 2);
        assert!(u.statements.is_some());
        assert!(u.branches.is_some());
        assert!(u.locals.is_some());
        assert!(u.calls.is_some());
    }

    #[test]
    fn push_impl_block_and_push_impl_method_metrics() {
        let code = r"
            struct Widget;
            impl Widget {
                fn new() -> Self { Widget }
                fn update(&mut self, flag: bool) { let _ = flag; }
            }
        ";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_items(&ast.items, "w.rs", &mut units);

        let impl_units: Vec<_> = units.iter().filter(|u| u.kind == "impl").collect();
        assert_eq!(impl_units.len(), 1);
        assert_eq!(impl_units[0].name, "Widget");
        assert_eq!(impl_units[0].methods.unwrap(), 2);

        let method_units: Vec<_> = units.iter().filter(|u| u.kind == "method").collect();
        assert_eq!(method_units.len(), 2);
        let names: Vec<&str> = method_units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"new"));
        assert!(names.contains(&"update"));
    }

    #[test]
    fn rust_fn_method_push_struct_used() {
        let code = "fn solo() { let x = 1; }";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut units = Vec::new();
        collect_detailed_from_items(&ast.items, "test.rs", &mut units);
        let u = &units[0];
        assert!(u.indentation.is_some());
        assert!(u.nested_depth.is_some());
        assert!(u.boolean_parameters.is_some());
        assert!(u.annotations.is_some());
        assert!(u.args_positional.is_some());
        assert_eq!(u.args_keyword_only.unwrap(), 0);
        let _ = RustFnMethodPush {
            file: "x.rs",
            name: "f".to_string(),
            line: 1,
            kind: "function",
            m: &crate::rust_fn_metrics::compute_rust_function_metrics(
                &syn::punctuated::Punctuated::new(),
                &syn::parse_str::<syn::Block>("{}").unwrap(),
                0,
            ),
        };
    }
}
