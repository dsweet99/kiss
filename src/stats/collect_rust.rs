use std::collections::BTreeMap;

use syn::{ImplItem, Item};

use crate::code_roles::{SourceRoleIndex, skip_syn};
use crate::rust_counts::get_impl_type_name;
use crate::rust_fn_metrics::{compute_rust_function_metrics_with_roles, count_non_doc_attrs};

use super::metric_stats::MetricStats;

pub(crate) fn push_rust_fn_metrics(
    stats: &mut MetricStats,
    m: &crate::rust_counts::RustFunctionMetrics,
) {
    stats.statements_per_function.push(m.statements);
    stats.arguments_positional.push(m.arguments);
    stats.max_indentation.push(m.max_indentation);
    stats.nested_function_depth.push(m.nested_function_depth);
    stats.returns_per_function.push(m.returns);
    stats.branches_per_function.push(m.branches);
    stats.local_variables_per_function.push(m.local_variables);
    stats.boolean_parameters.push(m.bool_parameters);
    stats.annotations_per_function.push(m.attributes);
    stats.calls_per_function.push(m.calls);
}

#[cfg(test)]
pub(crate) fn collect_rust_from_items(items: &[Item], stats: &mut MetricStats) {
    collect_rust_from_items_with_roles(items, stats, None, None);
}

pub(crate) fn collect_rust_from_items_with_roles(
    items: &[Item],
    stats: &mut MetricStats,
    path: Option<&std::path::Path>,
    roles: Option<&SourceRoleIndex>,
) {
    let mut inherent = BTreeMap::new();
    collect_rust_from_items_inner(items, stats, &mut inherent, path, roles);
    for count in inherent.values() {
        stats.methods_per_class.push(*count);
    }
}

fn collect_rust_from_items_inner(
    items: &[Item],
    stats: &mut MetricStats,
    inherent: &mut BTreeMap<String, usize>,
    path: Option<&std::path::Path>,
    roles: Option<&SourceRoleIndex>,
) {
    for item in items {
        if path.is_some_and(|p| skip_syn(roles, p, item)) {
            continue;
        }
        match item {
            Item::Fn(f) => push_rust_fn_metrics(
                stats,
                &compute_rust_function_metrics_with_roles(
                    &f.sig.inputs,
                    &f.block,
                    count_non_doc_attrs(&f.attrs),
                    path,
                    roles,
                ),
            ),
            Item::Impl(i) => collect_rust_impl(i, stats, inherent, path, roles),
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    collect_rust_from_items_inner(items, stats, inherent, path, roles);
                }
            }
            _ => {}
        }
    }
}

fn collect_rust_impl(
    i: &syn::ItemImpl,
    stats: &mut MetricStats,
    inherent: &mut BTreeMap<String, usize>,
    path: Option<&std::path::Path>,
    roles: Option<&SourceRoleIndex>,
) {
    let mcnt = i
        .items
        .iter()
        .filter(|ii| match ii {
            ImplItem::Fn(m) => !path.is_some_and(|p| skip_syn(roles, p, m)),
            _ => false,
        })
        .count();
    if i.trait_.is_none() {
        let name = get_impl_type_name(i).unwrap_or_else(|| "<impl>".into());
        *inherent.entry(name).or_insert(0) += mcnt;
    } else {
        stats.methods_per_class.push(mcnt);
    }
    for ii in &i.items {
        if let ImplItem::Fn(m) = ii {
            if path.is_some_and(|p| skip_syn(roles, p, m)) {
                continue;
            }
            push_rust_fn_metrics(
                stats,
                &compute_rust_function_metrics_with_roles(
                    &m.sig.inputs,
                    &m.block,
                    count_non_doc_attrs(&m.attrs),
                    path,
                    roles,
                ),
            );
        }
    }
}

#[cfg(test)]
mod collect_rust_coverage {
    use super::*;

    #[test]
    fn touch_for_coverage() {
        let code = "struct Foo;\nimpl Foo { fn bar(&self) { let x = 1; } }";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut stats = MetricStats::default();
        collect_rust_from_items(&ast.items, &mut stats);
        assert!(
            !stats.methods_per_class.is_empty(),
            "impl block should populate methods_per_class"
        );
        assert!(
            !stats.statements_per_function.is_empty(),
            "impl method should populate statements"
        );
    }

    #[test]
    fn collect_rust_impl_populates_method_stats() {
        let code = r"
            struct Counter;
            impl Counter {
                fn inc(&mut self, by: usize) {
                    let old = self.count;
                    self.count = old + by;
                }
                fn reset(&mut self) {
                    self.count = 0;
                }
            }
        ";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut stats = MetricStats::default();
        collect_rust_from_items(&ast.items, &mut stats);

        assert_eq!(
            stats.methods_per_class,
            vec![2],
            "collect_rust_impl should count 2 methods"
        );
        assert_eq!(
            stats.statements_per_function.len(),
            2,
            "collect_rust_impl should push stats for each method"
        );
        assert_eq!(
            stats.arguments_positional.len(),
            2,
            "push_rust_fn_metrics should push positional args for each method"
        );
    }

    #[test]
    fn collect_rust_impl_with_top_level_fn() {
        let code = r"
            fn top(a: i32) -> i32 { a + 1 }
            struct S;
            impl S {
                fn method(&self) { let _ = 1; }
            }
        ";
        let ast: syn::File = syn::parse_str(code).unwrap();
        let mut stats = MetricStats::default();
        collect_rust_from_items(&ast.items, &mut stats);

        assert_eq!(stats.methods_per_class, vec![1]);
        assert_eq!(stats.statements_per_function.len(), 2);
        assert!(stats.branches_per_function.len() == 2);
        assert!(stats.local_variables_per_function.len() == 2);
        assert!(stats.boolean_parameters.len() == 2);
        assert!(stats.annotations_per_function.len() == 2);
        assert!(stats.calls_per_function.len() == 2);
    }
}
