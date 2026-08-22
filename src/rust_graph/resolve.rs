use std::collections::{HashMap, HashSet};

#[cfg(test)]
use crate::graph::DependencyGraph;

pub(crate) fn qualify_child_module(parent_module: &str, child: &str) -> String {
    if matches!(parent_module, "lib" | "main" | "build") {
        child.to_string()
    } else {
        format!("{parent_module}.{child}")
    }
}

pub(crate) fn resolve_import_targets(
    import: &str,
    module_name: &str,
    internal_modules: &HashSet<String>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    if internal_modules.contains(import) {
        return vec![import.to_string()];
    }
    let Some(qualified_names) = bare_to_qualified.get(import) else {
        return Vec::new();
    };
    qualified_names
        .iter()
        .filter(|qualified| *qualified != module_name)
        .cloned()
        .collect()
}

#[cfg(test)]
pub(crate) fn resolve_import(
    import: &str,
    module_name: &str,
    internal_modules: &HashSet<String>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
    graph: &mut DependencyGraph,
) {
    for target in resolve_import_targets(import, module_name, internal_modules, bare_to_qualified) {
        graph.add_dependency(module_name, &target);
    }
}
