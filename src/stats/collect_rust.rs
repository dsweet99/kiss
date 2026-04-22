use syn::{ImplItem, Item};

use crate::rust_fn_metrics::{compute_rust_function_metrics, count_non_doc_attrs, is_cfg_test_mod};

use super::metric_stats::MetricStats;

pub(crate) fn push_rust_fn_metrics(
    stats: &mut MetricStats,
    m: &crate::rust_counts::RustFunctionMetrics,
) {
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
            stats.arguments_per_function.len(),
            2,
            "push_rust_fn_metrics should push arguments for each method"
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
