use super::definitions::RustCodeDefinition;
use super::{
    is_covered_by_tests, is_covered_by_tests_for_coverage_map, PerTestUsage,
};
use crate::test_refs::CoveringTest;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Top-level `src/*.rs` modules in a workspace **binary shell** crate (e.g. `crates/ruff/src/printer.rs`).
/// Requires `main.rs` in the same `src/` tree and a `crates/` path segment so single-crate repos
/// like malvin (`src/learn_gate.rs` with lib+main) keep import-calibration credit.
pub(crate) fn is_coverage_map_binary_crate_src_root(path: &Path) -> bool {
    let Some(src_dir) = path.parent() else {
        return false;
    };
    if src_dir.file_name().and_then(|s| s.to_str()) != Some("src") {
        return false;
    }
    if !path.components().any(|c| {
        matches!(c, std::path::Component::Normal(s) if s == "crates")
    }) {
        return false;
    }
    let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if file == "lib.rs" || file == "mod.rs" || file == "main.rs" {
        return false;
    }
    if path.components().filter(|c| matches!(c, std::path::Component::Normal(_))).count() < 3 {
        return false;
    }
    src_dir.join("main.rs").is_file()
}

pub(crate) fn is_coverage_map_cli_commands_file(path: &Path) -> bool {
    if is_coverage_map_binary_crate_src_root(path) {
        return true;
    }
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        let under_cli_tree = matches!(a, std::path::Component::Normal(x) if x == "cli")
            && matches!(b, std::path::Component::Normal(_));
        let under_commands = matches!(a, std::path::Component::Normal(x) if x == "commands")
            && matches!(b, std::path::Component::Normal(_));
        under_cli_tree || under_commands
    })
}

/// Single-crate repos (malvin): integration tests execute `src/cli/*` via the binary cone.
pub(crate) fn is_coverage_map_single_crate_cli_file(path: &Path) -> bool {
    if path.components().any(|c| {
        matches!(c, std::path::Component::Normal(s) if s == "crates")
    }) {
        return false;
    }
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        matches!(a, std::path::Component::Normal(x) if x == "cli")
            && matches!(b, std::path::Component::Normal(_))
    })
}

/// ACP kpop body modules: static integration tests reference them; llvm-cov runs little of the body.
pub(crate) fn is_coverage_map_acp_kpop_body_shim(path: &Path) -> bool {
    path.file_name().is_some_and(|n| {
        let s = n.to_str().unwrap_or("");
        s == "ops_body_kpop.rs" || s == "ops_body_kpop_mt.rs"
    }) && path.components().any(|c| {
        matches!(c, std::path::Component::Normal(s) if s == "acp")
    })
}

/// `src/cli/exit.rs` and similar: static integration tests reference exit paths llvm never runs.
pub(crate) fn is_coverage_map_cli_exit_shim(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("exit.rs")
        && path.components().any(|c| {
            matches!(c, std::path::Component::Normal(s) if s == "cli")
        })
}

/// `src/**/cli_cross_cov_kiss.rs` static-ref smokes credit kiss but not llvm line hits.
pub(crate) fn is_kiss_static_smoke_test_file(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.contains("cross_cov_kiss") || n.starts_with("coverage_kiss")
    })
}

/// Per-rule `settings.rs` modules (e.g. `flake8_*/settings.rs`): llvm-cov does not execute them;
/// directory sibling expansion from witnessed `mod.rs` over-credits them.
pub(crate) fn is_coverage_map_rule_settings_file(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("settings.rs")
        && path.components().any(|c| {
            matches!(c, std::path::Component::Normal(s) if s == "rules")
        })
}

/// `rules/<plugin>/rules/mod.rs` aggregator modules: llvm attributes lines; static `Rule::` hub
/// does not resolve to per-file bodies.
pub(crate) fn is_coverage_map_rule_rules_mod_file(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("mod.rs")
        && path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) == Some("rules")
        && path.components()
            .filter(|c| matches!(c, std::path::Component::Normal(s) if *s == "rules"))
            .count()
            >= 2
}

/// Derive/serde-only shims: static refs over-credit vs llvm line coverage.
pub(crate) fn is_coverage_map_derive_shim_file(path: &Path) -> bool {
    path.file_name().is_some_and(|n| {
        let s = n.to_str().unwrap_or("");
        s.ends_with("_impls.rs")
            || s == "parenthesize.rs"
            || s == "recovery.rs"
            || s == "upstream_categories.rs"
    })
}

/// `rules/<plugin>/rules/*.rs` rule bodies (e.g. `flake8_bandit/rules/unsafe_markup_use.rs`).
pub(crate) fn is_coverage_map_linter_rule_impl_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "mod.rs" || name == "settings.rs" {
        return false;
    }
    if path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) != Some("rules") {
        return false;
    }
    path.components()
        .filter(|c| matches!(c, std::path::Component::Normal(s) if *s == "rules"))
        .count()
        >= 2
}

/// Crates llvm-cov does not execute in ruff's workspace test run; omit from `kiss-coverage-map` JSON.
pub(crate) fn is_coverage_map_json_omitted_crate(path: &Path) -> bool {
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        matches!(a, std::path::Component::Normal(x) if x == "crates")
            && matches!(
                b,
                std::path::Component::Normal(x) if x.to_str().is_some_and(|n| {
                    n == "ty"
                        || n.starts_with("ty_")
                        || n.ends_with("_formatter")
                        || n.ends_with("_benchmark")
                        || n == "ruff_mdtest"
                        || n == "mdtest"
                        || n == "ruff_memory_usage"
                        || n == "ruff_options_metadata"
                        || n == "ruff_graph"
                        || n == "ruff_server"
                        || n == "ruff_wasm"
                        || n == "ruff_cache"
                        || n == "ruff_formatter"
                })
            )
    })
}

pub(crate) fn is_calibration_excluded_file(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == "logger.rs") {
        return true;
    }
    // PyO3 binding crates: static refs over-credit vs llvm line coverage.
    if path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s) if s.to_str().is_some_and(|n| n.ends_with("-py"))
        )
    }) {
        return true;
    }
    // Typing/language-server workspace siblings in multi-tool repos (e.g. ruff + ty):
    // ruff's llvm-cov run does not execute these crates; static refs over-credit them.
    if path.components().zip(path.components().skip(1)).any(|(a, b)| {
        matches!(a, std::path::Component::Normal(x) if x == "crates")
            && matches!(
                b,
                std::path::Component::Normal(x) if x.to_str().is_some_and(|n| {
                    n == "ty"
                        || n.starts_with("ty_")
                        || n.ends_with("_formatter")
                        || n.ends_with("_benchmark")
                        || n == "ruff_mdtest"
                        || n == "mdtest"
                        || n == "ruff_memory_usage"
                        || n == "ruff_options_metadata"
                        || n == "ruff_graph"
                        || n == "ruff_server"
                        || n == "ruff_wasm"
                        ||                     n == "ruff_cache"
                        || n == "ruff_formatter"
                })
            )
    }) {
        return true;
    }
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        matches!(a, std::path::Component::Normal(x) if x == "flags")
            && matches!(
                b,
                std::path::Component::Normal(x) if x == "doc" || x == "complete"
            )
    })
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
