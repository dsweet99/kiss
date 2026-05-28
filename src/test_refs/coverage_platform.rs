use crate::test_refs::{CodeDefinition, PerTestUsage};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) fn is_platform_specific_prod_file(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("_win32") || s.contains("_windows") || s.contains("_extension")
}

pub(crate) fn is_windows_gated_test_file(source: &str) -> bool {
    source.contains("platform != \"win32\"")
        || source.contains("platform != 'win32'")
        || source.contains("sys.platform != \"win32\"")
        || source.contains("sys.platform != 'win32'")
}

pub(crate) fn is_pragma_no_cover_def(
    def: &CodeDefinition,
    parsed_files: &[&crate::parsing::ParsedFile],
) -> bool {
    let Some(parsed) = parsed_files.iter().find(|p| p.path == def.file) else {
        return false;
    };
    let lines: Vec<&str> = parsed.source.lines().collect();
    let idx = def.line.saturating_sub(1);
    if idx < lines.len() && lines[idx].contains("pragma: no cover") {
        return true;
    }
    idx > 0 && lines[idx - 1].contains("pragma: no cover")
}

pub(crate) fn deprioritize_pragma_no_cover_coverage(
    definitions: &[CodeDefinition],
    unreferenced: &mut Vec<CodeDefinition>,
    per_test_usage: &PerTestUsage,
    parsed_files: &[&crate::parsing::ParsedFile],
) {
    let mut to_add = Vec::new();
    for def in definitions {
        if !is_pragma_no_cover_def(def, parsed_files) {
            continue;
        }
        let direct = per_test_usage.iter().any(|(_, funcs)| {
            funcs.iter().any(|(_, refs)| refs.contains(&def.name))
        });
        if direct {
            continue;
        }
        let already_unref = unreferenced
            .iter()
            .any(|u| u.file == def.file && u.name == def.name && u.line == def.line);
        if !already_unref {
            to_add.push(def.clone());
        }
    }
    unreferenced.extend(to_add);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn deprioritize_platform_gated_coverage(
    definitions: &[CodeDefinition],
    unreferenced: &mut Vec<CodeDefinition>,
    per_test_usage: &PerTestUsage,
    parsed_files: &[&crate::parsing::ParsedFile],
    _name_files: &HashMap<String, HashSet<PathBuf>>,
    _disambiguation: &HashMap<String, PathBuf>,
    _import_bindings: &HashMap<String, HashSet<String>>,
    _module_suffixes: &HashMap<PathBuf, String>,
) {
    let gated_tests: HashSet<&Path> = parsed_files
        .iter()
        .filter(|p| is_windows_gated_test_file(&p.source))
        .map(|p| p.path.as_path())
        .collect();
    if gated_tests.is_empty() {
        return;
    }
    let mut to_add = Vec::new();
    for def in definitions {
        if !is_platform_specific_prod_file(&def.file) {
            continue;
        }
        let direct_test_witness =
            platform_direct_test_witness(def, per_test_usage, &gated_tests);
        let already_unref = unreferenced
            .iter()
            .any(|u| u.file == def.file && u.name == def.name && u.line == def.line);
        if already_unref {
            if direct_test_witness {
                unreferenced.retain(|u| {
                    u.file != def.file || u.name != def.name || u.line != def.line
                });
            }
            continue;
        }
        if direct_test_witness {
            continue;
        }
        to_add.push(def.clone());
    }
    unreferenced.extend(to_add);
}

pub(crate) fn platform_direct_test_witness(
    def: &CodeDefinition,
    per_test_usage: &PerTestUsage,
    gated_tests: &HashSet<&Path>,
) -> bool {
    let gated_names: HashSet<&str> = per_test_usage
        .iter()
        .filter(|(path, _)| gated_tests.contains(path.as_path()))
        .flat_map(|(_, funcs)| funcs.iter().flat_map(|(_, refs)| refs.iter().map(String::as_str)))
        .collect();
    for (test_path, funcs) in per_test_usage {
        if gated_tests.contains(test_path.as_path()) {
            continue;
        }
        for (_, refs) in funcs {
            if refs.contains(&def.name) {
                return true;
            }
            if def.containing_class.as_ref().is_some_and(|c| refs.contains(c))
                && !gated_names.contains(def.name.as_str())
            {
                return true;
            }
        }
    }
    false
}
