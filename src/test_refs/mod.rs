mod collect;
mod collect_mock_patch;
mod collect_parallel;
mod coverage;
mod coverage_weighted;
mod dead_region;
pub(crate) mod detection;
pub(crate) mod disambiguation;

use crate::graph::DependencyGraph;
#[cfg(test)]
use crate::graph::build_dependency_graph;
use crate::parsing::ParsedFile;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub use collect_parallel::test_functions_in;
pub(crate) use collect_parallel::collect_refs_parallel;
pub(crate) use coverage::{build_py_coverage_map, is_definition_covered};
pub use coverage_weighted::compute_py_weighted_file_pcts;
pub use coverage_weighted::py_init_marker_pct;
pub use detection::{has_test_framework_import, is_in_test_directory, is_test_file};
pub use disambiguation::build_name_file_map;
pub(crate) use disambiguation::{build_disambiguation_map, file_to_module_suffix};

#[cfg(test)]
pub(crate) use collect::collect_definitions;

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

pub(crate) type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>, HashSet<String>)>)>;

#[derive(Debug, Clone)]
pub struct TestRefAnalysis {
    pub definitions: Vec<CodeDefinition>,
    pub test_references: HashSet<String>,
    /// Names that appear as call targets in test code (not import/bind-only).
    pub call_references: HashSet<String>,
    /// Names patched via unittest.mock.patch (or similar) in tests.
    pub mocked_references: HashSet<String>,
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

pub fn analyze_test_refs_quick(parsed_files: &[&ParsedFile]) -> TestRefAnalysis {
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
    let (definitions, test_references, usage_references, call_references, import_bindings, per_test_usage, mocked_references) =
        collect_refs_parallel(parsed_files, need_coverage_map);

    let name_files = build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation =
        build_disambiguation_map(&name_files, &test_references, &per_test_usage, graph);
    let module_suffixes: HashMap<PathBuf, String> = definitions
        .iter()
        .map(|d| (d.file.clone(), file_to_module_suffix(&d.file)))
        .collect();

    let unreferenced: Vec<CodeDefinition> = definitions
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
        call_references,
        mocked_references,
        unreferenced,
        coverage_map,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_2;
#[cfg(test)]
mod tests_3;
#[cfg(test)]
mod tests_4;
#[cfg(test)]
mod tests_5;
#[cfg(test)]
mod tests_weighted;
