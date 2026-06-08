use std::collections::{HashMap, HashSet};

use crate::graph::DependencyGraph;

pub(crate) fn qualify_child_module(parent_module: &str, child: &str) -> String {
    if matches!(parent_module, "lib" | "main" | "build") {
        child.to_string()
    } else {
        format!("{parent_module}.{child}")
    }
}

pub(crate) fn resolve_import(
    import: &str,
    module_name: &str,
    internal_modules: &HashSet<String>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
    graph: &mut DependencyGraph,
) {
    if internal_modules.contains(import) {
        graph.add_dependency(module_name, import);
        return;
    }
    let Some(qualified_names) = bare_to_qualified.get(import) else {
        return;
    };
    for qualified in qualified_names {
        if qualified != module_name {
            graph.add_dependency(module_name, qualified);
        }
    }
}
