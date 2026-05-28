mod collect;
mod collect_parallel;
mod collect_test_file;
mod coverage;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod tests_collect;
mod calibration_analysis;
mod coverage_expand;
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
pub(crate) use collect_parallel::collect_refs_parallel_for_coverage_map;
pub(crate) use coverage::{
    build_py_coverage_map, is_definition_covered,
};
pub use detection::{has_test_framework_import, is_in_test_directory, is_test_file};
pub use disambiguation::build_name_file_map;
pub(crate) use disambiguation::{build_disambiguation_map, file_to_module_suffix};

#[cfg(test)]
pub(crate) use collect::collect_definitions;
#[cfg(test)]
pub(crate) use collect_test_file::{
    collect_all_test_file_data, collect_all_test_file_data_for_coverage_map,
    process_test_file_ast_node,
};

#[derive(Debug, Clone)]
pub struct CodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    pub end_line: usize,
    pub containing_class: Option<String>,
}

/// (`test_file_path`, `test_function_name`) — e.g. (`"tests/test_utils.py"`, `"test_parse_empty"`)
pub type CoveringTest = (PathBuf, String);

pub(crate) type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug, Clone)]
pub struct TestRefAnalysis {
    pub definitions: Vec<CodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<CodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

#[derive(Copy, Clone)]
pub(crate) enum TestRefsAnalysisKind {
    Full { need_coverage_map: bool },
    CoverageCalibration,
    Quick,
}

#[allow(clippy::too_many_lines)]
pub fn analyze_test_refs(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(
        parsed_files,
        graph,
        TestRefsAnalysisKind::Full {
            need_coverage_map: true,
        },
    )
}

/// Like [`analyze_test_refs`], but uses call-only witnesses and one-hop propagation for
/// `kiss-coverage-map` calibration.
pub fn analyze_test_refs_for_coverage_map(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, graph, TestRefsAnalysisKind::CoverageCalibration)
}

pub fn analyze_test_refs_quick(parsed_files: &[&ParsedFile]) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, None, TestRefsAnalysisKind::Quick)
}

pub fn analyze_test_refs_no_map(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(
        parsed_files,
        graph,
        TestRefsAnalysisKind::Full {
            need_coverage_map: false,
        },
    )
}

pub(crate) fn calibration_witness_refs(
    parsed_files: &[&ParsedFile],
    per_test_usage: &PerTestUsage,
) -> HashSet<String> {
    let gated_test_paths: HashSet<&std::path::Path> = parsed_files
        .iter()
        .filter(|p| coverage::is_windows_gated_test_file(&p.source))
        .map(|p| p.path.as_path())
        .collect();
    per_test_usage
        .iter()
        .filter(|(path, _)| !gated_test_paths.contains(path.as_path()))
        .flat_map(|(_, funcs)| funcs.iter().flat_map(|(_, refs)| refs.iter().cloned()))
        .collect()
}

pub(crate) struct CalibrationPostprocessCtx<'a> {
    parsed_files: &'a [&'a ParsedFile],
    per_test_usage: &'a PerTestUsage,
    name_files: &'a HashMap<String, HashSet<PathBuf>>,
    disambiguation: &'a HashMap<String, PathBuf>,
    import_bindings: &'a HashMap<String, HashSet<String>>,
    module_suffixes: &'a HashMap<PathBuf, String>,
    graph: Option<&'a DependencyGraph>,
    test_witness_refs: &'a HashSet<String>,
}

fn apply_calibration_postprocessing(
    analysis: &mut TestRefAnalysis,
    ctx: &CalibrationPostprocessCtx<'_>,
) {
    coverage::deprioritize_platform_gated_coverage(
        &analysis.definitions,
        &mut analysis.unreferenced,
        ctx.per_test_usage,
        ctx.parsed_files,
        ctx.name_files,
        ctx.disambiguation,
        ctx.import_bindings,
        ctx.module_suffixes,
    );
    if let Some(g) = ctx.graph {
        coverage::apply_import_dependency_calibration(
            analysis,
            g,
            ctx.test_witness_refs,
            ctx.name_files,
        );
    }
}

fn analyze_test_refs_inner(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
    kind: TestRefsAnalysisKind,
) -> TestRefAnalysis {
    let calibration = matches!(kind, TestRefsAnalysisKind::CoverageCalibration);
    let need_coverage_map = match kind {
        TestRefsAnalysisKind::Full {
            need_coverage_map,
        } => need_coverage_map,
        TestRefsAnalysisKind::CoverageCalibration => true,
        TestRefsAnalysisKind::Quick => false,
    };
    let (definitions, test_references, mut usage_references, import_bindings, per_test_usage) =
        if calibration {
            collect_refs_parallel_for_coverage_map(parsed_files)
        } else {
            collect_refs_parallel(parsed_files, need_coverage_map)
        };

    let test_witness_refs = if calibration {
        calibration_witness_refs(parsed_files, &per_test_usage)
    } else {
        HashSet::new()
    };
    let (calibration_strict_refs, calibration_expanded_refs, calibration_defs_per_file) =
        if calibration {
            calibration_analysis::build_calibration_coverage_refs(
                parsed_files,
                &definitions,
                &test_witness_refs,
            )
        } else {
            coverage_expand::expand_py_usage_refs_fixpoint(parsed_files, &mut usage_references);
            coverage_expand::expand_py_import_sibling_refs(parsed_files, &mut usage_references);
            (HashSet::new(), usage_references.clone(), HashMap::new())
        };

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

    let unreferenced = calibration_analysis::filter_unreferenced_definitions(
        &definitions,
        &calibration_analysis::UnreferencedFilterCtx {
            calibration,
            defs_per_file: &calibration_defs_per_file,
            test_witness_refs: &test_witness_refs,
            calibration_strict_refs: &calibration_strict_refs,
            calibration_expanded_refs: &calibration_expanded_refs,
            usage_references: &usage_references,
            name_files: &name_files,
            disambiguation: &disambiguation,
            import_bindings: &import_bindings,
            module_suffixes: &module_suffixes,
        },
    );

    let coverage_map = if need_coverage_map && !calibration {
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

    let mut analysis = TestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
        coverage_map,
    };
    if calibration {
        let cal_ctx = CalibrationPostprocessCtx {
            parsed_files,
            per_test_usage: &per_test_usage,
            name_files: &name_files,
            disambiguation: &disambiguation,
            import_bindings: &import_bindings,
            module_suffixes: &module_suffixes,
            graph,
            test_witness_refs: &test_witness_refs,
        };
        apply_calibration_postprocessing(&mut analysis, &cal_ctx);
    }
    analysis
}

#[cfg(test)]
mod calibration_tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn calibration_witness_refs_ignores_gated_test_file() {
        let mut gated = NamedTempFile::with_suffix("_test.py").unwrap();
        write!(
            gated,
            "import sys\nif sys.platform != 'win32':\n    def test_x():\n        gated_only()\n"
        )
        .unwrap();
        let mut clean = NamedTempFile::with_suffix("_test.py").unwrap();
        write!(clean, "def test_y():\n    clean_only()\n").unwrap();
        let mut parser = create_parser().expect("parser");
        let gated_p = parse_file(&mut parser, gated.path()).expect("parse");
        let clean_p = parse_file(&mut parser, clean.path()).expect("parse");
        let parsed = [&gated_p, &clean_p];
        let (_, _, _, _, per_test) = collect_refs_parallel_for_coverage_map(&parsed);
        let refs = calibration_witness_refs(&parsed, &per_test);
        assert!(refs.contains("clean_only"));
        assert!(!refs.contains("gated_only"));
    }

    #[test]
    fn apply_calibration_postprocessing_runs_without_graph() {
        let mut analysis = TestRefAnalysis {
            definitions: vec![],
            test_references: HashSet::new(),
            unreferenced: vec![],
            coverage_map: HashMap::new(),
        };
        apply_calibration_postprocessing(
            &mut analysis,
            &CalibrationPostprocessCtx {
                parsed_files: &[],
                per_test_usage: &Vec::new(),
                name_files: &HashMap::new(),
                disambiguation: &HashMap::new(),
                import_bindings: &HashMap::new(),
                module_suffixes: &HashMap::new(),
                graph: None,
                test_witness_refs: &HashSet::new(),
            },
        );
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
mod tests_6;
#[cfg(test)]
mod tests_7;
