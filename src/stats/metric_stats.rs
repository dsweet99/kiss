use crate::code_roles::production_line_count;
use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{compute_file_metrics, walk_py_ast};
use crate::rust_fn_metrics::compute_rust_file_metrics_with_roles;
use crate::rust_parsing::ParsedRustFile;
use rayon::prelude::*;

use super::collect_py::StatsVisitor;
use super::collect_rust::collect_rust_from_items_with_roles;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetricStats {
    pub statements_per_function: Vec<usize>,
    pub arguments_positional: Vec<usize>,
    pub arguments_keyword_only: Vec<usize>,
    pub max_indentation: Vec<usize>,
    pub nested_function_depth: Vec<usize>,
    pub returns_per_function: Vec<usize>,
    pub return_values_per_function: Vec<usize>,
    pub branches_per_function: Vec<usize>,
    pub local_variables_per_function: Vec<usize>,
    pub statements_per_try_block: Vec<usize>,
    pub boolean_parameters: Vec<usize>,
    pub annotations_per_function: Vec<usize>,
    pub calls_per_function: Vec<usize>,
    pub methods_per_class: Vec<usize>,
    pub statements_per_file: Vec<usize>,
    pub lines_per_file: Vec<usize>,
    pub functions_per_file: Vec<usize>,
    pub interface_types_per_file: Vec<usize>,
    pub concrete_types_per_file: Vec<usize>,
    pub imported_names_per_file: Vec<usize>,
    pub fan_in: Vec<usize>,
    pub fan_out: Vec<usize>,
    pub cycle_size: Vec<usize>,
    pub indirect_dependencies: Vec<usize>,
    pub dependency_depth: Vec<usize>,
}

impl MetricStats {
    pub fn collect(parsed_files: &[&ParsedFile]) -> Self {
        parsed_files
            .par_iter()
            .map(|parsed| {
                let mut stats = Self::default();
                let fm = compute_file_metrics(parsed);
                stats.statements_per_file.push(fm.statements);
                stats.lines_per_file.push(parsed.source.lines().count());
                stats.functions_per_file.push(fm.functions);
                stats.interface_types_per_file.push(fm.interface_types);
                stats.concrete_types_per_file.push(fm.concrete_types);

                let fname = parsed
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if fname != "__init__.py" {
                    stats.imported_names_per_file.push(fm.imports);
                }
                let mut visitor = StatsVisitor { stats: &mut stats };
                walk_py_ast(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &mut |a| visitor.process(a),
                    false,
                );
                stats
            })
            .reduce(Self::default, |mut a, b| {
                a.merge(b);
                a
            })
    }

    pub fn merge(&mut self, o: Self) {
        macro_rules! ext { ($($f:ident),*) => { $(self.$f.extend(o.$f);)* }; }
        ext!(
            statements_per_function,
            arguments_positional,
            arguments_keyword_only,
            max_indentation,
            nested_function_depth,
            returns_per_function,
            return_values_per_function,
            branches_per_function,
            local_variables_per_function,
            statements_per_try_block,
            boolean_parameters,
            annotations_per_function,
            calls_per_function,
            methods_per_class,
            statements_per_file,
            lines_per_file,
            functions_per_file,
            interface_types_per_file,
            concrete_types_per_file,
            imported_names_per_file,
            fan_in,
            fan_out,
            cycle_size,
            indirect_dependencies,
            dependency_depth
        );
    }

    pub fn collect_graph_metrics(&mut self, graph: &DependencyGraph) {
        use std::collections::HashMap;

        let cycles = graph.find_cycles().cycles;
        let mut cycle_size_by_module: HashMap<&str, usize> = HashMap::new();
        for cycle in &cycles {
            let size = cycle.len();
            for m in cycle {
                cycle_size_by_module.insert(m.as_str(), size);
            }
        }

        for name in graph.paths.keys() {
            let m = graph.module_metrics(name);
            self.fan_in.push(m.fan_in);
            self.fan_out.push(m.fan_out);
            self.indirect_dependencies.push(m.indirect_dependencies);
            self.dependency_depth.push(m.dependency_depth);
            self.cycle_size
                .push(*cycle_size_by_module.get(name.as_str()).unwrap_or(&0));
        }
    }

    pub fn max_depth(&self) -> usize {
        self.dependency_depth.iter().copied().max().unwrap_or(0)
    }

    pub fn collect_rust(parsed_files: &[&ParsedRustFile]) -> Self {
        Self::collect_rust_with_roles(parsed_files, None)
    }

    pub fn collect_rust_with_roles(
        parsed_files: &[&ParsedRustFile],
        roles: Option<&crate::code_roles::SourceRoleIndex>,
    ) -> Self {
        let mut stats = Self::default();
        for parsed in parsed_files {
            let fm = compute_rust_file_metrics_with_roles(parsed, roles);
            stats.statements_per_file.push(fm.statements);
            let lines = roles.map_or_else(
                || parsed.source.lines().count(),
                |roles| production_line_count(roles, &parsed.path, &parsed.source),
            );
            stats.lines_per_file.push(lines);
            stats.functions_per_file.push(fm.functions);
            stats.interface_types_per_file.push(fm.interface_types);
            stats.concrete_types_per_file.push(fm.concrete_types);
            stats.imported_names_per_file.push(fm.imports);
            collect_rust_from_items_with_roles(
                &parsed.ast.items,
                &mut stats,
                Some(&parsed.path),
                roles,
            );
        }
        stats
    }
}
