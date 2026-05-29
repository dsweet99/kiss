use super::coverage_map_unreferenced::{
    coverage_map_single_crate_cli_witnessed, definition_uncovered_for_coverage_map,
    is_coverage_map_integration_cone_inflation_shim, CoverageMapUnrefCtx,
};
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[test]
fn single_crate_cli_credit_must_not_collapse_to_bare_name_integration_cone_gate() {
    let def = super::RustCodeDefinition {
        name: "handle".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("src/cli/handle.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    };
    let bare_name_witness = HashSet::from(["handle".to_string()]);
    let mut cone = HashSet::new();
    cone.insert(PathBuf::from("src/main.rs"));
    let name_files = crate::test_refs::build_name_file_map(
        [(def.name.as_str(), def.file.as_path())].into_iter(),
    );
    let defs_per_file = HashMap::from([(def.file.clone(), 1usize)]);
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &bare_name_witness,
        coverage_references: &HashSet::new(),
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &cone,
        defs_per_file: &defs_per_file,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &HashSet::new(),
    };

    assert!(
        !coverage_map_single_crate_cli_witnessed(&def, &ctx),
        "bare-name witness must not credit CLI handler via integration-cone channel collapse"
    );
    assert!(
        definition_uncovered_for_coverage_map(&def, std::slice::from_ref(&def), &ctx),
        "CLI inflator partition must stay uncovered without call witness in coverage_references"
    );
}

/// principles.md: Formal model §6 — partitions from topology, not dirname lists
/// smell_registry: dirname_inventory
/// metamorphic: global_segment_bijection
/// follow_up: rename first-level src segment vendor_progression → kpop_progression
/// invariant: integration-cone inflation shim must classify isomorphic first-level segments identically under rename
/// counterexample: paired src/vendor_progression/decoy.rs and src/kpop_progression/decoy.rs with identical inner file topology
#[test]
fn integration_cone_inflation_shim_is_invariant_under_first_level_segment_rename() {
    let source = Path::new("src/vendor_progression/decoy.rs");
    let follow_up = Path::new("src/kpop_progression/decoy.rs");

    assert_eq!(
        is_coverage_map_integration_cone_inflation_shim(source),
        is_coverage_map_integration_cone_inflation_shim(follow_up),
        "integration-cone inflation shim must not key on benchmark first-level dirname inventory"
    );
}

/// principles.md: Quality bar §1 — CLI argv routing must not rely on benchmark workflow basename tables
/// smell_registry: cli_argv_inventory
/// metamorphic: workflow_basename_role_swap
/// follow_up: rename paired top-level CLI module widget_session.rs → kpop_session.rs
/// invariant: CLI route bulk-credit exclusion must depend on structural argv topology, not malvin workflow basename inventory
/// counterexample: paired src/cli/widget_session.rs and src/cli/kpop_session.rs with identical top-level stem shape and argv token widget
#[test]
fn cli_route_bulk_credit_must_not_key_on_malvin_workflow_basenames() {
    use super::calibration_route::file_matches_cli_route;

    let tokens = HashSet::from(["widget".to_string()]);
    let neutral = Path::new("src/cli/widget_session.rs");
    let benchmark_shaped = Path::new("src/cli/kpop_session.rs");

    assert_eq!(
        file_matches_cli_route(neutral, &tokens),
        file_matches_cli_route(benchmark_shaped, &tokens),
        "CLI route matching must be invariant under workflow basename role-swap when argv topology is fixed"
    );
    assert!(
        file_matches_cli_route(neutral, &tokens),
        "neutral top-level CLI session module must structurally match its argv token without benchmark basename gate"
    );
}

/// principles.md: Development methodology §4 — structural mechanisms over thresholds; Quality bar §1 — no filename tables
/// smell_registry: filename_inventory
/// metamorphic: workflow_basename_role_swap + def_count_axis_sweep
/// follow_up: sweep defs_per_file 0..=16 on paired src/acp/ops_body_kpop.rs → src/acp/ops_body_widget.rs
/// invariant: dense-partition transition signatures (axis, index, direction) must match under ACP basename role-swap when witness topology is fixed
/// counterexample: paired src/acp/ops_body_kpop.rs and src/acp/ops_body_widget.rs with identical impl-type witness in coverage_references
#[test]
fn acp_body_forced_uncovered_threshold_geography_must_not_key_on_kpop_basename() {
    fn transition_signature(rel_path: &str) -> Vec<usize> {
        let file = PathBuf::from(rel_path);
        let witness = HashSet::from(["Widget".to_string()]);
        let def = super::RustCodeDefinition {
            name: "dispatch".into(),
            kind: CodeUnitKind::Method,
            file: file.clone(),
            line: 1,
            end_line: 1,
            impl_for_type: Some("Widget".into()),
        };
        let name_files = crate::test_refs::build_name_file_map(
            [(def.name.as_str(), def.file.as_path())].into_iter(),
        );
        let mut prev_uncovered = None;
        let mut flips = Vec::new();
        for k in 0..=16 {
            let defs_per_file = HashMap::from([(file.clone(), k)]);
            let ctx = CoverageMapUnrefCtx {
                test_witness_refs: &HashSet::new(),
                coverage_references: &witness,
                name_files: &name_files,
                disambiguation: &HashMap::new(),
                integration_cone_files: &HashSet::new(),
                defs_per_file: &defs_per_file,
                cli_route_attested_files: &HashSet::new(),
                witnessed_rule_plugins: &HashSet::new(),
            };
            let uncovered =
                definition_uncovered_for_coverage_map(&def, std::slice::from_ref(&def), &ctx);
            if let Some(prev) = prev_uncovered
                && uncovered != prev
            {
                flips.push(k);
            }
            prev_uncovered = Some(uncovered);
        }
        flips
    }

    let neutral = transition_signature("src/acp/ops_body_widget.rs");
    let benchmark_shaped = transition_signature("src/acp/ops_body_kpop.rs");

    assert_eq!(
        neutral, benchmark_shaped,
        "dense-partition transition signature must be invariant under ACP basename role-swap (ops_body_kpop inventory)"
    );
    assert!(
        !neutral.is_empty(),
        "neutral ACP body module must cross dense threshold when defs_per_file axis is swept"
    );
}

/// principles.md: Formal model §6 — partitions from topology, not distant repo inventory
/// smell_registry: non_local_context
/// metamorphic: graft_shell_family
/// follow_up: graft eight neutral decoy crates alongside unchanged crates/acme_formatter tree
/// invariant: auxiliary JSON omission for a formatter-suffix workspace member must not flip when only distant crates/ shells change
/// counterexample: lone crates/acme_formatter vs same inner tree with decoy_0..decoy_7 siblings
#[test]
fn workspace_formatter_auxiliary_must_not_depend_on_distant_crate_shell_graft() {
    use super::calibration_map;
    use std::fs;

    fn formatter_json_omitted_with_decoy_shells(decoy_count: usize) -> bool {
        let tmp = tempfile::TempDir::new().unwrap();
        let crates = tmp.path().join("crates");
        fs::create_dir_all(crates.join("acme_formatter/src")).unwrap();
        fs::write(crates.join("acme_formatter/src/lib.rs"), "// lib\n").unwrap();
        for i in 0..decoy_count {
            fs::create_dir_all(crates.join(format!("decoy_{i}"))).unwrap();
        }
        let path = crates.join("acme_formatter/src/lib.rs");
        calibration_map::is_coverage_map_json_omitted_crate(&path)
    }

    let lone_workspace = formatter_json_omitted_with_decoy_shells(0);
    let grafted_workspace = formatter_json_omitted_with_decoy_shells(8);

    assert_eq!(
        lone_workspace, grafted_workspace,
        "formatter auxiliary JSON omission must be invariant to distant crates/ graft shells"
    );
    assert!(
        lone_workspace,
        "neutral formatter workspace member should be omitted once suffix role is established"
    );
}

/// principles.md: Quality bar §1 — path patterns tied to code shape, not global basename allowlists
/// smell_registry: repo_root_vocabulary
/// metamorphic: none
/// follow_up: n/a
/// invariant: witness-sufficient production defs must not stay uncovered solely because basename is logger.rs
/// counterexample: src/acme/logger.rs vs src/acme/trace.rs with identical witness {emit_event} in both tiers
#[test]
fn witness_sufficient_defs_must_not_be_vetoed_by_global_logger_basename_inventory() {
    fn uncovered_for(rel_path: &str, witness: &HashSet<String>) -> bool {
        let file = PathBuf::from(rel_path);
        let def = super::RustCodeDefinition {
            name: "emit_event".into(),
            kind: CodeUnitKind::Function,
            file: file.clone(),
            line: 1,
            end_line: 1,
            impl_for_type: None,
        };
        let name_files = crate::test_refs::build_name_file_map(
            [(def.name.as_str(), def.file.as_path())].into_iter(),
        );
        let defs_per_file = HashMap::from([(file, 1usize)]);
        let ctx = CoverageMapUnrefCtx {
            test_witness_refs: witness,
            coverage_references: witness,
            name_files: &name_files,
            disambiguation: &HashMap::new(),
            integration_cone_files: &HashSet::new(),
            defs_per_file: &defs_per_file,
            cli_route_attested_files: &HashSet::new(),
            witnessed_rule_plugins: &HashSet::new(),
        };
        definition_uncovered_for_coverage_map(&def, std::slice::from_ref(&def), &ctx)
    }

    let witness = HashSet::from(["emit_event".to_string()]);
    let logger_path = "src/acme/logger.rs";
    let neutral_path = "src/acme/trace.rs";

    assert!(
        !uncovered_for(neutral_path, &witness),
        "neutral production module must be credited when name is witnessed in both strict and expanded tiers"
    );
    assert_eq!(
        uncovered_for(logger_path, &witness),
        uncovered_for(neutral_path, &witness),
        "global logger.rs basename inventory must not veto witness-sufficient credit"
    );
}

/// principles.md: Development methodology §4 — structural mechanisms over per-partition numeric staircases
/// smell_registry: numeric_staircase
/// metamorphic: none
/// follow_up: n/a
/// invariant: linter rule-impl bodies with impl-type witnesses must not be forced uncovered at defs_per_file=1 solely because dense_cap=0
/// counterexample: rules/widget_plugin/rules/guard.rs method with impl-type witness in coverage_references
#[test]
fn linter_rule_impl_dense_cap_must_not_veto_impl_type_witness_at_single_definition() {
    let file = PathBuf::from("crates/linter/src/rules/widget_plugin/rules/guard.rs");
    let witness = HashSet::from(["Widget".to_string()]);
    let def = super::RustCodeDefinition {
        name: "guard".into(),
        kind: CodeUnitKind::Method,
        file: file.clone(),
        line: 1,
        end_line: 1,
        impl_for_type: Some("Widget".into()),
    };
    let name_files = crate::test_refs::build_name_file_map(
        [(def.name.as_str(), def.file.as_path())].into_iter(),
    );
    let defs_per_file = HashMap::from([(file.clone(), 1usize)]);
    let witnessed_plugins = HashSet::from(["widget_plugin".to_string()]);
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &HashSet::new(),
        defs_per_file: &defs_per_file,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &witnessed_plugins,
    };

    assert!(
        !definition_uncovered_for_coverage_map(&def, std::slice::from_ref(&def), &ctx),
        "single-def linter rule impl with impl-type witness must not be dense-vetoed at defs_per_file=1"
    );
}
