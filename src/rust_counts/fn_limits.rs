use crate::rust_fn_metrics::compute_rust_function_metrics_with_roles;
use syn::Block;

use super::RustAnalyzer;

impl RustAnalyzer<'_> {
    pub(crate) fn analyze_function(
        &mut self,
        name: &str,
        line: usize,
        inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
        block: &Block,
        attr_count: usize,
        ut: &str,
    ) {
        let m = compute_rust_function_metrics_with_roles(
            inputs,
            block,
            attr_count,
            Some(self.file),
            self.roles,
        );
        let c = self.config;

        macro_rules! chk {
            ($mf:ident, $cf:ident, $metric:literal, $label:literal, $sug:literal) => {
                if m.$mf > c.$cf {
                    self.violations.push(
                        self.build_violation(line, name)
                            .metric($metric)
                            .value(m.$mf)
                            .threshold(c.$cf)
                            .message(format!(
                                "{} '{}' has {} {} (threshold: {})",
                                ut, name, m.$mf, $label, c.$cf
                            ))
                            .suggestion($sug)
                            .build(),
                    );
                }
            };
        }

        chk!(
            statements,
            statements_per_function,
            "statements_per_function",
            "statements",
            "Break into smaller, focused functions."
        );
        chk!(
            arguments,
            arguments_positional,
            "positional_args",
            "arguments",
            "Group related arguments into a struct."
        );
        chk!(
            max_indentation,
            max_indentation_depth,
            "max_indentation_depth",
            "indentation depth",
            "Use early returns, guard clauses, or extract helper functions."
        );
        chk!(
            returns,
            returns_per_function,
            "returns_per_function",
            "return statements",
            "Use early guard returns at the top, then a single main return path."
        );
        chk!(
            branches,
            branches_per_function,
            "branches_per_function",
            "branches",
            "Consider using match guards, early returns, or extracting logic."
        );
        chk!(
            local_variables,
            local_variables_per_function,
            "local_variables_per_function",
            "local variables",
            "Extract logic into helper functions with fewer variables each."
        );
        chk!(
            nested_function_depth,
            nested_function_depth,
            "nested_function_depth",
            "nested closure depth",
            "Extract nested closures into separate functions."
        );
        chk!(
            bool_parameters,
            boolean_parameters,
            "boolean_parameters",
            "bool parameters",
            "Use an enum or a struct with named fields instead of multiple bools."
        );
        chk!(
            attributes,
            annotations_per_function,
            "annotations_per_function",
            "attributes",
            "Consider consolidating attributes or simplifying the function's responsibilities. (TOML key: attributes_per_function)"
        );
        chk!(
            calls,
            calls_per_function,
            "calls_per_function",
            "calls",
            "Extract some calls into helper functions to reduce coordination complexity."
        );
    }
}
