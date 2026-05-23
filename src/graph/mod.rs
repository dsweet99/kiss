//! Python dependency graph construction and analysis.
//!
//! Split `include!` shells keep per-includer rollup under `lines_per_file` while avoiding a
//! single flat include chain that would roll ~900 lines into `graph/mod.rs`.

mod dependency_graph {
    use petgraph::Direction;
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

pub use graph_analyze::{analyze_graph, compute_cyclomatic_complexity};
pub use graph_build::build_dependency_graph;
pub use dependency_graph::{
    CycleInfo, DependencyGraph, ModuleGraphMetrics, is_entry_point, qualified_module_name,
};

#[cfg(test)]
pub(crate) use graph_analyze::{
    collect_module_violations, count_decision_points, cycle_size_violation,
    dependency_depth_violation, get_module_path, indirect_deps_violation, is_decision_point,
    is_init_module, is_path_covered_by_another, orphan_violation, path_dedup_set,
};
#[cfg(test)]
pub(crate) use graph_build::{
    ImportListPass, GraphBuildState, build_dependency_graph_from_import_lists,
    parent_prefix_match, resolve_bare, resolve_dotted, resolve_import,
};
#[cfg(test)]
pub(crate) use dependency_graph::{
    bare_module_name, file_stem_str, is_crate_root_aggregator, is_orphan, is_test_module,
    join_qualified_dirs_and_stem, parent_dir_strings, trim_src_suffix,
};
#[cfg(test)]
pub(crate) use graph_python::{
    collect_imported_name_candidates, extract_dynamic_import_module, extract_imports_for_cache,
    extract_imports_recursive, extract_modules_from_import_from, is_dunder_import,
    is_importlib_import_module, parse_python_string_literal, push_dotted_segments,
    push_import_name_segments, read_base_module, strip_rbub_prefix, unquote_single, unquote_triple,
};

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;

#[cfg(test)]
#[path = "graph_test_2.rs"]
mod graph_test_2;
