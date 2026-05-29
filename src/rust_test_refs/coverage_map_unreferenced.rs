use super::calibration;
use super::calibration_map;
use super::definitions::RustCodeDefinition;
use super::{is_covered_by_tests_for_coverage_map, is_directly_referenced};
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) struct CoverageMapUnrefCtx<'a> {
    pub test_witness_refs: &'a HashSet<String>,
    pub coverage_references: &'a HashSet<String>,
    pub name_files: &'a HashMap<String, HashSet<PathBuf>>,
    pub disambiguation: &'a HashMap<String, PathBuf>,
    pub integration_cone_files: &'a HashSet<PathBuf>,
    pub defs_per_file: &'a HashMap<PathBuf, usize>,
    pub cli_route_attested_files: &'a HashSet<PathBuf>,
    /// Plugins with ≥1 rule def name in `coverage_references` (registry manifold S, g10 idea #3).
    pub witnessed_rule_plugins: &'a HashSet<String>,
}

fn is_integration_cone_inflation_segment(seg: &str) -> bool {
    seg.contains('_')
        || matches!(
            seg,
            "cli" | "acp" | "output" | "orchestrator" | "cursor_store" | "tool_summary"
        )
}

/// Integration-cone inflation shim: `src/<segment>/<file>.rs` where the segment is a
/// multi-module workflow directory (underscore or known high-fan-out role), not a flat `src/*.rs` root.
pub(crate) fn is_coverage_map_integration_cone_inflation_shim(path: &Path) -> bool {
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
    let Some(seg) = comps.get(src_idx + 1) else {
        return false;
    };
    comps.len() >= src_idx + 3 && is_integration_cone_inflation_segment(seg)
}

pub(crate) fn coverage_map_forced_uncovered_file(path: &Path) -> bool {
    calibration_map::is_coverage_map_rule_settings_file(path)
        || calibration_map::is_coverage_map_cli_exit_shim(path)
        || calibration_map::is_coverage_map_acp_client_impl_shim(path)
        || calibration_map::is_coverage_map_binary_crate_src_root(path)
}

pub(crate) fn build_witnessed_rule_plugins(
    definitions: &[RustCodeDefinition],
    coverage_references: &HashSet<String>,
) -> HashSet<String> {
    definitions
        .iter()
        .filter(|d| calibration_map::is_coverage_map_linter_rule_impl_file(&d.file))
        .filter(|d| coverage_references.contains(&d.name))
        .filter_map(|d| {
            calibration_map::linter_rule_plugin_name(&d.file).map(str::to_string)
        })
        .collect()
}

pub(crate) fn coverage_map_plugin_rule_impl_witness(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    if !calibration_map::is_coverage_map_linter_rule_impl_file(&d.file) {
        return false;
    }
    let Some(plugin) = calibration_map::linter_rule_plugin_name(&d.file) else {
        return false;
    };
    // Registry manifold alone over-credits rule bodies llvm never executes (g11).
    ctx.witnessed_rule_plugins.contains(plugin)
        && ctx.test_witness_refs.contains(&d.name)
        && is_covered_by_tests_for_coverage_map(
            d,
            ctx.coverage_references,
            ctx.name_files,
            ctx.disambiguation,
        )
}

/// Plugin support modules (helpers.rs): credit when the plugin has any witnessed rule impl
/// (llvm runs helpers when integration tests exercise the plugin).
pub(crate) fn coverage_map_plugin_support_plugin_witness(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    if !calibration_map::is_coverage_map_rule_plugin_support_file(&d.file) {
        return false;
    }
    let Some(plugin) = calibration_map::linter_rule_plugin_name(&d.file) else {
        return false;
    };
    ctx.witnessed_rule_plugins.contains(plugin)
        && is_covered_by_tests_for_coverage_map(
            d,
            ctx.coverage_references,
            ctx.name_files,
            ctx.disambiguation,
        )
}

pub(crate) fn coverage_map_plugin_support_direct_only(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    calibration_map::is_coverage_map_rule_plugin_support_file(&d.file)
        && coverage_map_direct_test_witness(d, ctx)
}

pub(crate) fn coverage_map_cli_route_witnessed(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    use super::calibration_map::is_coverage_map_single_crate_cli_file;
    let key = crate::rust_include::canonical_path(&d.file);
    is_coverage_map_single_crate_cli_file(&d.file)
        && ctx.cli_route_attested_files.contains(&key)
        && ctx.coverage_references.contains(&d.name)
}

pub(crate) fn coverage_map_single_crate_cli_witnessed(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    calibration_map::is_coverage_map_single_crate_cli_file(&d.file)
        && !ctx.integration_cone_files.is_empty()
        && ctx.coverage_references.contains(&d.name)
        && ctx.defs_per_file.get(&d.file).copied().unwrap_or(0) <= 8
}

pub(crate) fn coverage_map_direct_test_witness(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    !calibration::is_coverage_map_cli_commands_file(&d.file)
        && !calibration_map::is_coverage_map_linter_rule_impl_file(&d.file)
        && is_covered_by_tests_for_coverage_map(
            d,
            ctx.test_witness_refs,
            ctx.name_files,
            ctx.disambiguation,
        )
}

pub(crate) fn coverage_map_integration_cone_witness(
    d: &RustCodeDefinition,
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    ctx.integration_cone_files
        .contains(&crate::rust_include::canonical_path(&d.file))
        && !calibration::is_calibration_excluded_file(&d.file)
        && !calibration::is_coverage_map_cli_commands_file(&d.file)
        && !is_coverage_map_integration_cone_inflation_shim(&d.file)
        && ctx.test_witness_refs.contains(&d.name)
}

pub(crate) fn coverage_map_expanded_dense_file(d: &RustCodeDefinition, ctx: &CoverageMapUnrefCtx<'_>) -> bool {
    if calibration_map::is_coverage_map_linter_rule_impl_file(&d.file)
        && matches!(
            d.kind,
            CodeUnitKind::TraitImplMethod | CodeUnitKind::Method
        )
        && d
            .impl_for_type
            .as_ref()
            .is_some_and(|t| ctx.coverage_references.contains(t))
    {
        return false;
    }
    let dense_cap = if calibration_map::is_coverage_map_linter_rule_impl_file(&d.file) {
        0
    } else {
        4
    };
    ctx.defs_per_file.get(&d.file).copied().unwrap_or(0) > dense_cap
}

pub(crate) fn definition_uncovered_for_coverage_map(
    d: &RustCodeDefinition,
    _definitions: &[RustCodeDefinition],
    ctx: &CoverageMapUnrefCtx<'_>,
) -> bool {
    if coverage_map_forced_uncovered_file(&d.file) {
        return true;
    }
    let witnessed = coverage_map_cli_route_witnessed(d, ctx)
        || coverage_map_single_crate_cli_witnessed(d, ctx)
        || coverage_map_direct_test_witness(d, ctx)
        || coverage_map_plugin_rule_impl_witness(d, ctx)
        || coverage_map_plugin_support_direct_only(d, ctx)
        || coverage_map_plugin_support_plugin_witness(d, ctx)
        || coverage_map_integration_cone_witness(d, ctx);
    if witnessed {
        return false;
    }
    if calibration_map::is_coverage_map_rule_plugin_support_file(&d.file) {
        return true;
    }
    let from_expanded = is_covered_by_tests_for_coverage_map(
        d,
        ctx.coverage_references,
        ctx.name_files,
        ctx.disambiguation,
    );
    if !from_expanded || calibration::is_coverage_map_cli_commands_file(&d.file) {
        return true;
    }
    !is_directly_referenced(d, ctx.coverage_references, ctx.name_files, ctx.disambiguation)
        && coverage_map_expanded_dense_file(d, ctx)
}

pub(crate) fn unreferenced_for_coverage_map(
    definitions: &[RustCodeDefinition],
    ctx: &CoverageMapUnrefCtx<'_>,
) -> Vec<RustCodeDefinition> {
    definitions
        .iter()
        .filter(|d| definition_uncovered_for_coverage_map(d, definitions, ctx))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::CodeUnitKind;

    #[test]
    fn forced_uncovered_and_witness_paths() {
        let defs = vec![
            RustCodeDefinition {
                name: "seen".into(),
                kind: CodeUnitKind::Function,
                file: PathBuf::from("a.rs"),
                line: 1,
                end_line: 1,
                impl_for_type: None,
            },
            RustCodeDefinition {
                name: "miss".into(),
                kind: CodeUnitKind::Function,
                file: PathBuf::from("a.rs"),
                line: 2,
                end_line: 2,
                impl_for_type: None,
            },
        ];
        let counts: HashMap<PathBuf, usize> = defs.iter().fold(HashMap::new(), |mut m, d| {
            *m.entry(d.file.clone()).or_default() += 1;
            m
        });
        let name_files = crate::test_refs::build_name_file_map(
            defs.iter().map(|d| (d.name.as_str(), d.file.as_path())),
        );
        let witness = HashSet::from(["seen".to_string()]);
        let ctx = CoverageMapUnrefCtx {
            test_witness_refs: &witness,
            coverage_references: &HashSet::new(),
            name_files: &name_files,
            disambiguation: &HashMap::new(),
            integration_cone_files: &HashSet::new(),
            defs_per_file: &counts,
            cli_route_attested_files: &HashSet::new(),
            witnessed_rule_plugins: &HashSet::new(),
        };
        let unref = unreferenced_for_coverage_map(&defs, &ctx);
        assert_eq!(unref.len(), 1);
        assert_eq!(unref[0].name, "miss");
    }
}
