use super::definitions::RustCodeDefinition;
use super::{is_covered_by_tests, is_covered_by_tests_for_coverage_map, PerTestUsage};
use crate::test_refs::CoveringTest;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// CLI surface files: integration-cone / impl-type expansion is not trusted without a
/// direct test witness (reduces inflation vs runtime line coverage).
pub(crate) fn is_coverage_map_cli_commands_file(path: &Path) -> bool {
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        let under_cli_tree = matches!(a, std::path::Component::Normal(x) if x == "cli")
            && matches!(b, std::path::Component::Normal(_));
        let under_commands = matches!(a, std::path::Component::Normal(x) if x == "commands")
            && matches!(b, std::path::Component::Normal(_));
        under_cli_tree || under_commands
    })
}

pub(crate) fn is_calibration_excluded_file(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == "logger.rs") {
        return true;
    }
    path.components().zip(path.components().skip(1)).any(
        |(a, b)| {
            matches!(a, std::path::Component::Normal(x) if x == "flags")
                && matches!(b, std::path::Component::Normal(x) if x == "doc")
        },
    )
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_rust_coverage_map(
    definitions: &[RustCodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    coverage_references: &HashSet<String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(&def.name).or_default().push(i);
        if let Some(ref t) = def.impl_for_type {
            name_to_defs.entry(t.as_str()).or_default().push(i);
        }
    }

    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            if test_id.is_empty() {
                continue;
            }
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = name_to_defs.get(ref_name.as_str()) else {
                    continue;
                };
                for &idx in def_indices {
                    if !seen.insert(idx) {
                        continue;
                    }
                    let def = &definitions[idx];
                    if !is_covered_by_tests(def, coverage_references, name_files, disambiguation) {
                        continue;
                    }
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
}

#[allow(dead_code)] // retained for gate/calibration tooling; kiss-coverage-map file_map path skips it
pub(crate) fn build_rust_coverage_map_for_calibration(
    definitions: &[RustCodeDefinition],
    per_test_usage: &PerTestUsage,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    coverage_references: &HashSet<String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(def.name.as_str()).or_default().push(i);
    }

    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            if test_id.is_empty() {
                continue;
            }
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = name_to_defs.get(ref_name.as_str()) else {
                    continue;
                };
                for &idx in def_indices {
                    if !seen.insert(idx) {
                        continue;
                    }
                    let def = &definitions[idx];
                    if !is_covered_by_tests_for_coverage_map(
                        def,
                        coverage_references,
                        name_files,
                        disambiguation,
                    ) {
                        continue;
                    }
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
}
