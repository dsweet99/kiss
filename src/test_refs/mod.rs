mod collect;
mod coverage;
pub(crate) mod detection;
pub(crate) mod disambiguation;

use crate::graph::DependencyGraph;
#[cfg(test)]
use crate::graph::build_dependency_graph;
use crate::parsing::ParsedFile;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub use detection::{has_test_framework_import, is_in_test_directory, is_test_file};
pub use disambiguation::build_name_file_map;
pub(crate) use collect::collect_refs_parallel;
pub(crate) use coverage::{build_py_coverage_map, is_definition_covered};
pub(crate) use disambiguation::{build_disambiguation_map, file_to_module_suffix};

#[cfg(test)]
pub(crate) use collect::{
    collect_all_test_file_data, collect_call_target, collect_class_test_methods,
    collect_definitions, collect_import_names, collect_test_functions_with_refs, collect_type_refs,
    collect_usage_refs_in_scope, extract_import_from_binding, insert_identifier, try_add_def,
};
#[cfg(test)]
pub(crate) use coverage::{build_ref_to_covered_def_indices, is_covered_by_import};
#[cfg(test)]
pub(crate) use detection::{
    contains_test_module_name, has_python_test_naming, has_test_function_or_class,
    is_abstract_method, is_protocol_class, is_python_test_file, is_test_class, is_test_framework,
    is_test_framework_import_from, is_test_function,
};
#[cfg(test)]
pub(crate) use disambiguation::{
    disambiguate_files, disambiguate_files_graph_fallback, module_suffix_matches, path_identifiers,
    resolve_ambiguous_name,
};

#[derive(Debug, Clone)]
pub struct CodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    pub containing_class: Option<String>,
}

/// (`test_file_path`, `test_function_name`) — e.g. (`"tests/test_utils.py"`, `"test_parse_empty"`)
pub type CoveringTest = (PathBuf, String);

pub(crate) type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug)]
pub struct TestRefAnalysis {
    pub definitions: Vec<CodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<CodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

#[allow(clippy::too_many_lines)]
pub fn analyze_test_refs(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, graph, true)
}

pub fn analyze_test_refs_quick(
    parsed_files: &[&ParsedFile],
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, None, false)
}

pub fn analyze_test_refs_no_map(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, graph, false)
}

fn analyze_test_refs_inner(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
    need_coverage_map: bool,
) -> TestRefAnalysis {
    let (definitions, test_references, usage_references, import_bindings, per_test_usage) =
        collect_refs_parallel(parsed_files, need_coverage_map);

    let name_files = build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation = build_disambiguation_map(
        &name_files,
        &test_references,
        &per_test_usage,
        graph,
    );
    let module_suffixes: HashMap<PathBuf, String> = definitions
        .iter()
        .map(|d| (d.file.clone(), file_to_module_suffix(&d.file)))
        .collect();

    let unreferenced = definitions
        .iter()
        .filter(|def| {
            !is_definition_covered(
                def,
                &name_files,
                &disambiguation,
                &import_bindings,
                &module_suffixes,
                &usage_references,
            )
        })
        .cloned()
        .collect();

    let coverage_map = if need_coverage_map {
        build_py_coverage_map(
            &definitions,
            &per_test_usage,
            &name_files,
            &disambiguation,
            &import_bindings,
            &module_suffixes,
        )
    } else {
        HashMap::new()
    };

    TestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
        coverage_map,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_2;
