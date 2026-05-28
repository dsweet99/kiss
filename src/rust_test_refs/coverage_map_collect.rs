use super::calibration;
use super::calibration_map;
use super::definitions::{
    collect_rust_definitions, collect_test_module_references_for_coverage_map,
};
use super::references::{
    collect_per_test_usage_for_coverage_map, collect_rust_references,
    collect_rust_references_for_coverage_map,
};
use super::{is_rust_test_file, PerTestUsage, RustCodeDefinition};
use crate::rust_parsing::ParsedRustFile;
use std::collections::HashSet;
use std::path::PathBuf;

pub(crate) fn subprocess_integration_test_paths(parsed_files: &[&ParsedRustFile]) -> HashSet<PathBuf> {
    parsed_files
        .iter()
        .filter(|p| {
            is_rust_test_file(&p.path) && calibration::is_subprocess_integration_test_file(p)
        })
        .map(|p| crate::rust_include::canonical_path(&p.path))
        .collect()
}

pub(crate) fn test_witness_refs_excluding_subprocess(
    per_test_usage: &PerTestUsage,
    subprocess_paths: &HashSet<PathBuf>,
) -> HashSet<String> {
    per_test_usage
        .iter()
        .filter(|(path, _)| {
            !subprocess_paths.contains(&crate::rust_include::canonical_path(path))
        })
        .flat_map(|(_, funcs)| funcs.iter().flat_map(|(_, refs)| refs.iter().cloned()))
        .collect()
}

/// Witness refs from integration/unit test files only (not `#[cfg(test)]` inside production sources).
#[cfg(test)]
pub(crate) fn external_test_witness_refs(
    per_test_usage: &PerTestUsage,
    subprocess_paths: &HashSet<PathBuf>,
) -> HashSet<String> {
    per_test_usage
        .iter()
        .filter(|(path, _)| {
            is_rust_test_file(path)
                && !subprocess_paths.contains(&crate::rust_include::canonical_path(path))
        })
        .flat_map(|(_, funcs)| funcs.iter().flat_map(|(_, refs)| refs.iter().cloned()))
        .collect()
}

pub(crate) fn collect_coverage_map_scan(
    parsed_files: &[&ParsedRustFile],
) -> (
    Vec<RustCodeDefinition>,
    HashSet<String>,
    HashSet<String>,
    PerTestUsage,
) {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();
    let mut coverage_references = HashSet::new();
    let mut per_test_usage: PerTestUsage = Vec::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            collect_rust_references(&parsed.ast, &mut test_references);
            if !calibration_map::is_kiss_static_smoke_test_file(&parsed.path)
                && !calibration::is_subprocess_integration_test_file(parsed)
            {
                collect_rust_references_for_coverage_map(&parsed.ast, &mut coverage_references);
            }
        } else {
            collect_rust_definitions(&parsed.ast, &parsed.path, &mut definitions);
            collect_test_module_references_for_coverage_map(&parsed.ast, &mut test_references);
        }
        if calibration_map::is_kiss_static_smoke_test_file(&parsed.path) {
            continue;
        }
        let test_funcs = collect_per_test_usage_for_coverage_map(&parsed.ast);
        if !test_funcs.is_empty() {
            per_test_usage.push((parsed.path.clone(), test_funcs));
        }
    }
    (definitions, test_references, coverage_references, per_test_usage)
}
