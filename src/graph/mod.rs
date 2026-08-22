mod context;
mod dependency_graph {
    use petgraph::algo::tarjan_scc;
    use petgraph::graph::{DiGraph, NodeIndex};
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::path::{Component, Path, PathBuf};

    include!("dependency_graph_body.rs");
}
mod graph_analyze;
mod graph_build;
mod graph_python;

pub use context::{
    ContextDependencyGraph, EdgeOrigin, RoleDependencyGraphs, module_name_for_path,
    path_for_module_name,
};
pub use dependency_graph::{
    CycleInfo, DependencyGraph, ModuleGraphMetrics, is_entry_point, qualified_module_name,
};
pub use graph_analyze::{
    GraphKeyMaxima, analyze_graph, compute_cyclomatic_complexity, graph_key_maxima,
};
pub use graph_build::{build_dependency_graph, build_python_context_graph};
pub(crate) use graph_python::{
    extract_dynamic_import_module, extract_imports_for_cache, is_dunder_import,
    is_importlib_import_module,
};

#[cfg(test)]
pub(crate) use dependency_graph::{bare_module_name, is_crate_root_aggregator, is_orphan};
#[cfg(test)]
pub(crate) use graph_analyze::{
    count_decision_points, cycle_size_violation, get_module_path, is_decision_point,
};
#[cfg(test)]
pub(crate) use graph_build::{
    ImportListPass, build_dependency_graph_from_import_lists, parent_prefix_match, resolve_bare,
    resolve_dotted, resolve_import,
};
#[cfg(test)]
pub(crate) use graph_python::{
    extract_imports_recursive, extract_modules_from_import_from, push_dotted_segments,
    push_import_name_segments,
};

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;

#[cfg(test)]
#[path = "graph_test_2.rs"]
mod graph_test_2;
