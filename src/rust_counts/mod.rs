use std::collections::HashMap;
use std::path::Path;
use syn::{ImplItem, Item};

use crate::code_roles::{SourceRoleIndex, production_line_count, skip_syn};
use crate::config::Config;
use crate::rust_fn_metrics::{compute_rust_file_metrics_with_roles, count_non_doc_attrs};
use crate::rust_parsing::ParsedRustFile;
use crate::violation::{Violation, ViolationBuilder};

pub use crate::rust_fn_metrics::{RustFileMetrics, RustFunctionMetrics, RustTypeMetrics};

#[cfg(test)]
#[path = "inline_coverage_tests.rs"]
mod inline_coverage_tests;

#[cfg(test)]
mod tests;

#[must_use]
pub fn analyze_rust_file(parsed: &ParsedRustFile, config: &Config) -> Vec<Violation> {
    analyze_rust_file_with_roles(parsed, config, None)
}

#[must_use]
pub fn analyze_rust_file_with_roles(
    parsed: &ParsedRustFile,
    config: &Config,
    roles: Option<&SourceRoleIndex>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut analyzer = RustAnalyzer::new(&parsed.path, config, &mut violations, roles);
    analyzer.check_parsed_file_metrics(parsed);
    for item in &parsed.ast.items {
        analyzer.analyze_item(item);
    }
    analyzer.flush_inherent_method_counts();
    violations
}

#[must_use]
pub fn analyze_rust_file_include_rollup(
    parent: &ParsedRustFile,
    included: &[&ParsedRustFile],
    config: &Config,
) -> Vec<Violation> {
    analyze_rust_file_include_rollup_with_roles(parent, included, config, None)
}

#[must_use]
pub fn analyze_rust_file_include_rollup_with_roles(
    parent: &ParsedRustFile,
    included: &[&ParsedRustFile],
    config: &Config,
    roles: Option<&SourceRoleIndex>,
) -> Vec<Violation> {
    if included.is_empty() {
        return Vec::new();
    }
    let mut violations = Vec::new();
    let mut analyzer = RustAnalyzer::new(&parent.path, config, &mut violations, roles);
    let mut merged = compute_rust_file_metrics_with_roles(parent, roles);
    let mut lines = counted_source_lines(parent, roles);
    let mut contributor_paths = Vec::new();
    for frag in included {
        let fm = compute_rust_file_metrics_with_roles(frag, roles);
        merged.statements += fm.statements;
        merged.interface_types += fm.interface_types;
        merged.concrete_types += fm.concrete_types;
        merged.imports += fm.imports;
        merged.functions += fm.functions;
        lines += counted_source_lines(frag, roles);
        contributor_paths.push(frag.path.display().to_string());
    }
    let contrib = contributor_paths.join(", ");
    let fname = parent
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    analyzer.check_rolled_file_metrics(&fname, &merged, lines, &contrib);
    violations
}

fn counted_source_lines(parsed: &ParsedRustFile, roles: Option<&SourceRoleIndex>) -> usize {
    roles.map_or_else(
        || parsed.source.lines().count(),
        |roles| production_line_count(roles, &parsed.path, &parsed.source),
    )
}

struct RustAnalyzer<'a> {
    file: &'a Path,
    config: &'a Config,
    violations: &'a mut Vec<Violation>,
    inherent_method_counts: HashMap<String, (usize, usize)>,
    roles: Option<&'a SourceRoleIndex>,
}

impl<'a> RustAnalyzer<'a> {
    fn new(
        file: &'a Path,
        config: &'a Config,
        violations: &'a mut Vec<Violation>,
        roles: Option<&'a SourceRoleIndex>,
    ) -> Self {
        Self {
            file,
            config,
            violations,
            inherent_method_counts: HashMap::new(),
            roles,
        }
    }

    fn push_file_threshold_violation(
        &mut self,
        fname: &str,
        metric: &'static str,
        value: usize,
        threshold: usize,
        message: String,
        suggestion: &'static str,
    ) {
        self.violations.push(
            self.build_violation(1, fname)
                .metric(metric)
                .value(value)
                .threshold(threshold)
                .message(message)
                .suggestion(suggestion)
                .build(),
        );
    }

    fn check_parsed_file_metrics(&mut self, parsed: &ParsedRustFile) {
        let m = compute_rust_file_metrics_with_roles(parsed, self.roles);
        let fname = self
            .file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let lines = counted_source_lines(parsed, self.roles);
        self.check_rolled_file_metrics(&fname, &m, lines, "");
    }

    fn check_rolled_file_metrics(
        &mut self,
        fname: &str,
        m: &RustFileMetrics,
        lines: usize,
        include_contributors: &str,
    ) {
        let c = self.config;
        let suffix = if include_contributors.is_empty() {
            String::new()
        } else {
            format!("; include fragments: {include_contributors}")
        };

        if lines > c.lines_per_file {
            self.push_file_threshold_violation(
                fname,
                "lines_per_file",
                lines,
                c.lines_per_file,
                format!(
                    "File has {lines} lines (threshold: {}){suffix}",
                    c.lines_per_file
                ),
                "Split the file roughly in half.",
            );
        }
        if m.statements > c.statements_per_file {
            self.push_file_threshold_violation(
                fname,
                "statements_per_file",
                m.statements,
                c.statements_per_file,
                format!(
                    "File has {} statements (threshold: {}){suffix}",
                    m.statements, c.statements_per_file
                ),
                "Split the file roughly in half.",
            );
        }
        if m.interface_types > c.interface_types_per_file {
            self.push_file_threshold_violation(
                fname,
                "interface_types_per_file",
                m.interface_types,
                c.interface_types_per_file,
                format!(
                    "File has {} interface types (threshold: {}){suffix}",
                    m.interface_types, c.interface_types_per_file
                ),
                "Move traits into a dedicated module.",
            );
        }
        if m.concrete_types > c.concrete_types_per_file {
            self.push_file_threshold_violation(
                fname,
                "concrete_types_per_file",
                m.concrete_types,
                c.concrete_types_per_file,
                format!(
                    "File has {} concrete types (threshold: {}){suffix}",
                    m.concrete_types, c.concrete_types_per_file
                ),
                "Move types to separate files.",
            );
        }
        if m.imports > c.imported_names_per_file && fname != "lib.rs" && fname != "mod.rs" {
            self.push_file_threshold_violation(
                fname,
                "imported_names_per_file",
                m.imports,
                c.imported_names_per_file,
                format!(
                    "File has {} use statements (threshold: {}){suffix}",
                    m.imports, c.imported_names_per_file
                ),
                "Module may have too many responsibilities. Consider splitting.",
            );
        }
        if m.functions > c.functions_per_file {
            self.push_file_threshold_violation(
                fname,
                "functions_per_file",
                m.functions,
                c.functions_per_file,
                format!(
                    "File has {} functions (threshold: {}){suffix}",
                    m.functions, c.functions_per_file
                ),
                "Split into multiple modules with focused responsibilities.",
            );
        }
    }

    fn analyze_item(&mut self, item: &Item) {
        if skip_syn(self.roles, self.file, item) {
            return;
        }
        match item {
            Item::Fn(func) => {
                let name = func.sig.ident.to_string();
                let line = func.sig.ident.span().start().line;
                self.analyze_function(
                    &name,
                    line,
                    &func.sig.inputs,
                    &func.block,
                    count_non_doc_attrs(&func.attrs),
                    "Function",
                );
            }
            Item::Impl(impl_block) => self.analyze_impl_block(impl_block),
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    for item in items {
                        self.analyze_item(item);
                    }
                }
            }
            _ => {}
        }
    }

    fn analyze_impl_block(&mut self, impl_block: &syn::ItemImpl) {
        let method_count = count_impl_methods(impl_block);
        let type_name = get_impl_type_name(impl_block);
        let line = impl_block.impl_token.span.start().line;
        let name = type_name.as_deref().unwrap_or("<impl>");
        if impl_block.trait_.is_none() {
            let entry = self
                .inherent_method_counts
                .entry(name.to_string())
                .or_insert((line, 0));
            entry.1 += method_count;
        } else {
            self.check_methods_per_class(line, name, method_count);
        }

        for impl_item in &impl_block.items {
            if let ImplItem::Fn(method) = impl_item {
                if skip_syn(self.roles, self.file, method) {
                    continue;
                }
                let mname = method.sig.ident.to_string();
                let mline = method.sig.ident.span().start().line;
                self.analyze_function(
                    &mname,
                    mline,
                    &method.sig.inputs,
                    &method.block,
                    count_non_doc_attrs(&method.attrs),
                    "Method",
                );
            }
        }
    }

    fn build_violation(&self, line: usize, name: &str) -> ViolationBuilder {
        Violation::builder(self.file).line(line).unit_name(name)
    }

    fn check_methods_per_class(&mut self, line: usize, name: &str, count: usize) {
        if count > self.config.methods_per_class {
            self.violations.push(
                self.build_violation(line, name)
                    .metric("methods_per_class")
                    .value(count)
                    .threshold(self.config.methods_per_class)
                    .message(format!(
                        "Type '{}' has {} methods (threshold: {})",
                        name, count, self.config.methods_per_class
                    ))
                    .suggestion("Extract related methods into a separate type with its own impl.")
                    .build(),
            );
        }
    }

    fn flush_inherent_method_counts(&mut self) {
        let counts = std::mem::take(&mut self.inherent_method_counts);
        for (name, (line, count)) in counts {
            self.check_methods_per_class(line, &name, count);
        }
    }
}

mod fn_limits;

pub(crate) fn count_impl_methods(impl_block: &syn::ItemImpl) -> usize {
    impl_block
        .items
        .iter()
        .filter(|item| matches!(item, ImplItem::Fn(_)))
        .count()
}

pub(crate) fn get_impl_type_name(impl_block: &syn::ItemImpl) -> Option<String> {
    if let syn::Type::Path(type_path) = impl_block.self_ty.as_ref() {
        type_path.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}
