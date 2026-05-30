use std::path::{Path, PathBuf};

/// Workspace **binary shell** crate modules under `crates/<crate>/src/**` (e.g. `crates/ruff/src/commands/check.rs`).
/// Requires `main.rs` in the same `src/` tree and a `crates/` path segment so single-crate repos
/// like malvin (`src/learn_gate.rs` with lib+main) keep import-calibration credit.
pub(crate) fn is_coverage_map_binary_crate_src_root(path: &Path) -> bool {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let Some(src_idx) = comps.iter().position(|&c| c == "src") else {
        return false;
    };
    if !comps.contains(&"crates") {
        return false;
    }
    let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if file == "lib.rs" || file == "mod.rs" || file == "main.rs" {
        return false;
    }
    if comps.len() <= src_idx + 1 {
        return false;
    }
    super::calibration_map_shims::coverage_map_binary_crate_src_dir(path)
        .is_some_and(|src_dir| src_dir.join("main.rs").is_file())
}

/// Production file physically split via `#[path = "..."]` in a sibling module (workflow shards).
pub(crate) fn is_coverage_map_path_attr_sibling_body(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return false;
    };
    for entry in entries.flatten() {
        let sibling = entry.path();
        if sibling == path || sibling.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&sibling) else {
            continue;
        };
        if content.contains(&format!(r#"#[path = "{file_name}"]"#))
            || content.contains(&format!(r#"#[path="{file_name}"]"#))
        {
            return true;
        }
    }
    false
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

/// PyO3 binding crates (`*-py`): llvm line coverage mis-aligns with static name witnesses.
pub(crate) fn is_coverage_map_pyo3_binding_crate(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s) if s.to_str().is_some_and(|n| n.ends_with("-py"))
        )
    })
}

/// ACP ops-body modules: static integration tests reference them; llvm-cov runs little of the body.
pub(crate) fn is_coverage_map_acp_kpop_body_shim(path: &Path) -> bool {
    path.file_name().is_some_and(|n| {
        let s = n.to_str().unwrap_or("");
        (s.starts_with("ops_body_") || s.starts_with("ops_body_mt_")) && s.ends_with(".rs")
    }) && path.components().any(|c| {
        matches!(c, std::path::Component::Normal(s) if s == "acp")
    })
}

/// `acp/client_impl_*.rs`: integration tests witness many symbols; llvm-cov executes a thin subset.
pub(crate) fn is_coverage_map_acp_client_impl_shim(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_str().is_some_and(|s| s.starts_with("client_impl_")))
        && path.components().any(|c| {
            matches!(c, std::path::Component::Normal(s) if s == "acp")
        })
}

/// Included fragment (`.inc`): llvm attributes lines separately; static witnesses cannot
/// match execution depth — omit from `kiss-coverage-map` JSON alignment.
pub(crate) fn is_coverage_map_rust_include_fragment_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("inc"))
}

/// Thin `include!(…)` host modules: llvm attributes included lines to the `.rs` path; static
/// defs live in included fragments kiss maps separately — omit from JSON alignment.
pub(crate) fn is_coverage_map_rust_include_host_file(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("rs") {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    if !content.contains("include!(") {
        return false;
    }
    content.lines().filter(|l| !l.trim().is_empty()).count() <= 20
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
            || s == "relocate.rs"
    }) || super::calibration_map_shims::is_coverage_map_ast_visitor_shim_file(path)
}

pub(crate) fn path_normal_components(path: &Path) -> Vec<&str> {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect()
}

/// Plugin directory name for paths under `…/rules/<plugin>/…`.
pub(crate) fn linter_rule_plugin_name(path: &Path) -> Option<&str> {
    let comps = path_normal_components(path);
    let rules_idx = comps.iter().position(|&c| c == "rules")?;
    comps.get(rules_idx + 1).copied()
}

/// `rules/<plugin>/mod.rs` — plugin facade re-export hub.
pub(crate) fn is_coverage_map_rule_plugin_top_mod(path: &Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()) != Some("mod.rs") {
        return false;
    }
    let Some(plugin_dir) = path.parent() else {
        return false;
    };
    plugin_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        == Some("rules")
        && plugin_dir
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n != "rules")
}

/// `rules/<plugin>/rules/mod.rs` — rule registry hub llvm executes when tests run a plugin.
pub(crate) fn is_coverage_map_rule_plugin_registry_hub(path: &Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()) != Some("mod.rs") {
        return false;
    }
    let Some(rules_dir) = path.parent() else {
        return false;
    };
    if rules_dir.file_name().and_then(|s| s.to_str()) != Some("rules") {
        return false;
    }
    rules_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        == Some("rules")
}

pub(crate) fn is_coverage_map_rule_plugin_hub_file(path: &Path) -> bool {
    is_coverage_map_rule_plugin_registry_hub(path) || is_coverage_map_rule_plugin_top_mod(path)
}

/// `helpers.rs` and siblings under `rules/<plugin>/` (not `rules/<plugin>/rules/*.rs` bodies).
pub(crate) fn is_coverage_map_rule_plugin_support_file(path: &Path) -> bool {
    if is_coverage_map_rule_plugin_hub_file(path) || is_coverage_map_linter_rule_impl_file(path) {
        return false;
    }
    let Some(plugin) = linter_rule_plugin_name(path) else {
        return false;
    };
    let comps = path_normal_components(path);
    let Some(rules_idx) = comps.iter().position(|&c| c == "rules") else {
        return false;
    };
    let after = &comps[rules_idx + 1..];
    after.first() == Some(&plugin) && after.get(1).is_some_and(|&seg| seg != "rules")
}

/// `src/checkers/**` dispatch modules llvm runs when integration tests exercise the linter.
pub(crate) fn is_coverage_map_linter_checkers_file(path: &Path) -> bool {
    let comps = path_normal_components(path);
    comps.windows(2)
        .any(|w| w[0] == "src" && w[1] == "checkers")
}

/// Workspace member with `crates/<crate>/*.rs` layout (no `src/` between crate root and modules).
pub(crate) fn is_coverage_map_flat_workspace_crate_module(path: &Path) -> bool {
    let comps = path_normal_components(path);
    let Some(crates_idx) = comps.iter().position(|&c| c == "crates") else {
        return false;
    };
    let Some(_crate_name) = comps.get(crates_idx + 1) else {
        return false;
    };
    let after_crate = &comps[crates_idx + 2..];
    !after_crate.is_empty() && after_crate[0] != "src"
}

/// `crates/<crate>/flags/**` — argv tables; static CLI-token expansion over-credits vs llvm.
pub(crate) fn is_coverage_map_workspace_crate_flags_tree(path: &Path) -> bool {
    let comps = path_normal_components(path);
    let Some(crates_idx) = comps.iter().position(|&c| c == "crates") else {
        return false;
    };
    comps.get(crates_idx + 2) == Some(&"flags")
}

/// `rules/<plugin>/rules/**/*.rs` rule bodies (e.g. `flake8_bandit/rules/unsafe_markup_use.rs`,
/// nested `pycodestyle/rules/logical_lines/*.rs`).
pub(crate) fn is_coverage_map_linter_rule_impl_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "mod.rs" || name == "settings.rs" {
        return false;
    }
    let comps = path_normal_components(path);
    let Some(rules_idx) = comps.iter().position(|&c| c == "rules") else {
        return false;
    };
    comps.get(rules_idx + 2) == Some(&"rules")
}

/// Workspace auxiliary crates llvm-cov typically skips (typing siblings, servers, harnesses).
/// Classified by structural name patterns under `crates/`, not benchmark-specific inventories.
pub(crate) fn is_workspace_llvm_auxiliary_crate(name: &str, workspace_crate_siblings: usize) -> bool {
    name == "ty"
        || name.starts_with("ty_")
        || name == "mdtest"
        || name.ends_with("_benchmark")
        || name.ends_with("_mdtest")
        || name.ends_with("_memory_usage")
        || name.ends_with("_options_metadata")
        || name.ends_with("_graph")
        || name.ends_with("_server")
        || name.ends_with("_wasm")
        || name.ends_with("_cache")
        || name.ends_with("_macros")
        || (workspace_crate_siblings >= 1
            && (name.ends_with("_formatter") || name.ends_with("_codegen")))
}

#[cfg(test)]
pub(crate) fn is_workspace_llvm_auxiliary_crate_for_test(name: &str, workspace_crate_siblings: usize) -> bool {
    is_workspace_llvm_auxiliary_crate(name, workspace_crate_siblings)
}

fn workspace_crates_dir(path: &Path) -> Option<PathBuf> {
    let mut dir = PathBuf::new();
    for comp in path.components() {
        dir.push(comp.as_os_str());
        if matches!(comp, std::path::Component::Normal(s) if s == "crates") {
            return Some(dir);
        }
    }
    None
}

fn workspace_crates_sibling_count(path: &Path) -> usize {
    let Some(crates_dir) = workspace_crates_dir(path) else {
        return 0;
    };
    std::fs::read_dir(crates_dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| {
                    let p = e.path();
                    p.is_dir() && (p.join("src").is_dir() || p.join("lib.rs").is_file())
                })
                .count()
        })
        .unwrap_or(0)
}

pub(crate) fn is_under_crates_auxiliary_crate(path: &Path) -> bool {
    let siblings = workspace_crates_sibling_count(path);
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        matches!(a, std::path::Component::Normal(x) if x == "crates")
            && matches!(
                b,
                std::path::Component::Normal(x) if x
                    .to_str()
                    .is_some_and(|n| is_workspace_llvm_auxiliary_crate(n, siblings))
            )
    })
}

/// Crates llvm-cov does not execute in a multi-crate workspace test run; omit from `kiss-coverage-map` JSON.
pub(crate) fn is_coverage_map_json_omitted_crate(path: &Path) -> bool {
    is_under_crates_auxiliary_crate(path)
}

pub(crate) fn is_calibration_excluded_file(path: &Path) -> bool {
    if is_coverage_map_linter_checkers_file(path)
        || is_coverage_map_workspace_crate_flags_tree(path)
    {
        return true;
    }
    if path.file_name().is_some_and(|n| n == "logger.rs")
        && path.components().any(|c| {
            matches!(c, std::path::Component::Normal(s) if s == "crates")
        })
    {
        return true;
    }
    if is_coverage_map_pyo3_binding_crate(path) {
        return true;
    }
    // Typing/language-server and other llvm-unexecuted workspace siblings: static refs over-credit them.
    if is_under_crates_auxiliary_crate(path) {
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
