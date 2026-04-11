use syn::{ImplItem, Item};

use crate::rust_fn_metrics::{
    compute_rust_function_metrics, count_non_doc_attrs, is_cfg_test_mod,
};

use super::metric_stats::MetricStats;

pub(crate) fn push_rust_fn_metrics(stats: &mut MetricStats, m: &crate::rust_counts::RustFunctionMetrics) {
    stats.statements_per_function.push(m.statements);
    stats.arguments_per_function.push(m.arguments);
    stats.max_indentation.push(m.max_indentation);
    stats.nested_function_depth.push(m.nested_function_depth);
    stats.returns_per_function.push(m.returns);
    stats.branches_per_function.push(m.branches);
    stats.local_variables_per_function.push(m.local_variables);
    stats.boolean_parameters.push(m.bool_parameters);
    stats.annotations_per_function.push(m.attributes);
    stats.calls_per_function.push(m.calls);
}

pub(crate) fn collect_rust_from_items(items: &[Item], stats: &mut MetricStats) {
    for item in items {
        match item {
            Item::Fn(f) => push_rust_fn_metrics(
                stats,
                &compute_rust_function_metrics(
                    &f.sig.inputs,
                    &f.block,
                    count_non_doc_attrs(&f.attrs),
                ),
            ),
            Item::Impl(i) => collect_rust_impl(i, stats),
            Item::Mod(m) => {
                if !is_cfg_test_mod(m)
                    && let Some((_, items)) = &m.content
                {
                    collect_rust_from_items(items, stats);
                }
            }
            _ => {}
        }
    }
}

fn collect_rust_impl(i: &syn::ItemImpl, stats: &mut MetricStats) {
    let mcnt = i
        .items
        .iter()
        .filter(|ii| matches!(ii, ImplItem::Fn(_)))
        .count();
    stats.methods_per_class.push(mcnt);
    for ii in &i.items {
        if let ImplItem::Fn(m) = ii {
            push_rust_fn_metrics(
                stats,
                &compute_rust_function_metrics(
                    &m.sig.inputs,
                    &m.block,
                    count_non_doc_attrs(&m.attrs),
                ),
            );
        }
    }
}
