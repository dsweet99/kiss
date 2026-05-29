use super::coverage_map_unreferenced::{
    coverage_map_direct_test_witness, coverage_map_expanded_dense_file,
    coverage_map_forced_uncovered_file, coverage_map_integration_cone_witness,
    coverage_map_single_crate_cli_witnessed, definition_uncovered_for_coverage_map,
    is_coverage_map_integration_cone_inflation_shim, CoverageMapUnrefCtx,
};
use std::path::Path;
use super::RustCodeDefinition;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn sample_def(name: &str, file: PathBuf) -> RustCodeDefinition {
    RustCodeDefinition {
        name: name.into(),
        kind: CodeUnitKind::Function,
        file,
        line: 1,
        end_line: 1,
        impl_for_type: None,
    }
}

#[test]
fn coverage_map_helper_paths() {
    assert!(is_coverage_map_integration_cone_inflation_shim(Path::new(
        "src/output/stdout_tee_env.rs"
    )));
    assert!(!is_coverage_map_integration_cone_inflation_shim(Path::new(
        "src/learn_gate.rs"
    )));
    assert!(coverage_map_forced_uncovered_file(Path::new(
        "src/acp/client_impl_session.rs"
    )));
    assert!(coverage_map_forced_uncovered_file(PathBuf::from(
        "crates/ruff_linter/src/rules/flake8_x/settings.rs"
    ).as_path()));

    let cli_def = sample_def("run", PathBuf::from("src/cli/exit.rs"));
    let lib_def = sample_def("helper", PathBuf::from("src/lib.rs"));
    let witness = HashSet::from(["run".to_string(), "helper".to_string()]);
    let mut cone = HashSet::new();
    cone.insert(PathBuf::from("src/main.rs"));
    let empty_map: HashMap<PathBuf, usize> = HashMap::new();
    let name_files = crate::test_refs::build_name_file_map(
        [(cli_def.name.as_str(), cli_def.file.as_path()), (lib_def.name.as_str(), lib_def.file.as_path())]
            .into_iter(),
    );
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &cone,
        defs_per_file: &empty_map,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &HashSet::new(),
    };
    assert!(coverage_map_single_crate_cli_witnessed(&cli_def, &ctx));
    assert!(coverage_map_direct_test_witness(&lib_def, &ctx));

    let mut cone_files = HashSet::new();
    cone_files.insert(crate::rust_include::canonical_path(&lib_def.file));
    let cone_ctx = CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &cone_files,
        defs_per_file: &empty_map,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &HashSet::new(),
    };
    assert!(coverage_map_integration_cone_witness(&lib_def, &cone_ctx));

    let mut dense_counts = HashMap::new();
    dense_counts.insert(lib_def.file.clone(), 5);
    let dense_ctx = CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &HashSet::new(),
        defs_per_file: &dense_counts,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &HashSet::new(),
    };
    assert!(coverage_map_expanded_dense_file(&lib_def, &dense_ctx));
    assert!(!definition_uncovered_for_coverage_map(
        &lib_def,
        std::slice::from_ref(&lib_def),
        &ctx,
    ));
}

/// principles.md: integration-cone credit must not veto on benchmark dirname inventories.
#[test]
fn integration_cone_witnesses_are_not_vetoed_by_benchmark_dirname_inventory() {
    let def = sample_def("dispatch", PathBuf::from("vendor/orchestrator/dispatch.rs"));
    let witness = HashSet::from(["dispatch".to_string()]);
    let mut cone = HashSet::new();
    cone.insert(crate::rust_include::canonical_path(&def.file));
    let name_files = crate::test_refs::build_name_file_map(
        [(def.name.as_str(), def.file.as_path())].into_iter(),
    );
    let empty_map: HashMap<PathBuf, usize> = HashMap::new();
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &cone,
        defs_per_file: &empty_map,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &HashSet::new(),
    };
    assert!(
        coverage_map_integration_cone_witness(&def, &ctx),
        "directly witnessed orchestrator def in integration cone must not lose credit to dirname veto"
    );
    assert!(
        !definition_uncovered_for_coverage_map(&def, std::slice::from_ref(&def), &ctx),
        "definition with integration-cone attestation must not be marked uncovered"
    );
}

/// principles.md: workspace crate partitions must not hard-code benchmark vendor crate names.
#[test]
fn calibration_workspace_crates_are_not_benchmark_name_inventory() {
    use super::calibration_map;

    assert!(
        !calibration_map::is_coverage_map_json_omitted_crate(Path::new(
            "crates/acme_formatter/src/lib.rs"
        )),
        "generic formatter workspace crates must not be omitted via benchmark-shaped suffix inventory"
    );
    assert!(
        !calibration_map::is_calibration_excluded_file(Path::new(
            "crates/acme_formatter/src/lib.rs"
        )),
        "generic formatter workspace crates must not lose calibration credit"
    );
    assert_eq!(
        calibration_map::is_coverage_map_json_omitted_crate(Path::new(
            "crates/ruff_graph/src/lib.rs"
        )),
        calibration_map::is_coverage_map_json_omitted_crate(Path::new(
            "crates/my_graph/src/lib.rs"
        )),
        "graph auxiliary crates must be classified by suffix pattern, not ruff_ prefix"
    );

    for path in [
        "crates/ty/src/lib.rs",
        "crates/ty_ide/src/lib.rs",
        "crates/mdtest/src/lib.rs",
        "crates/acme_mdtest/src/lib.rs",
        "crates/acme_benchmark/src/lib.rs",
        "crates/acme_memory_usage/src/lib.rs",
        "crates/acme_options_metadata/src/lib.rs",
        "crates/ruff_server/src/lib.rs",
        "crates/acme_wasm/src/lib.rs",
        "crates/acme_cache/src/lib.rs",
    ] {
        assert!(
            calibration_map::is_coverage_map_json_omitted_crate(Path::new(path)),
            "auxiliary workspace crate {path} should be omitted from coverage-map JSON"
        );
        assert!(
            calibration_map::is_calibration_excluded_file(Path::new(path)),
            "auxiliary workspace crate {path} should be calibration-excluded"
        );
    }

    let src = include_str!("calibration_map.rs");
    for lit in [
        "ruff_mdtest",
        "ruff_memory_usage",
        "ruff_options_metadata",
        "ruff_graph",
        "ruff_server",
        "ruff_wasm",
        "ruff_cache",
        "ruff_formatter",
    ] {
        assert!(
            !src.contains(lit),
            "calibration_map.rs must not hard-code benchmark crate name {lit:?}"
        );
    }
}

#[test]
fn calibration_map_plugin_and_workspace_path_classifiers() {
    use super::calibration_map;

    assert!(calibration_map::is_coverage_map_rule_plugin_top_mod(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/mod.rs"
    )));
    assert!(!calibration_map::is_coverage_map_rule_plugin_top_mod(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/rules/mod.rs"
    )));
    assert!(calibration_map::is_coverage_map_rule_plugin_registry_hub(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/rules/mod.rs"
    )));
    assert!(calibration_map::is_coverage_map_rule_plugin_hub_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/mod.rs"
    )));
    assert!(!calibration_map::is_coverage_map_rule_plugin_support_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/mod.rs"
    )));
    assert!(calibration_map::is_coverage_map_rule_plugin_support_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/helpers.rs"
    )));
    assert_eq!(
        calibration_map::linter_rule_plugin_name(Path::new(
            "crates/ruff_linter/src/rules/flake8_bandit/helpers.rs"
        )),
        Some("flake8_bandit")
    );
    assert!(calibration_map::is_coverage_map_linter_checkers_file(Path::new(
        "crates/ruff_linter/src/checkers/physical_lines.rs"
    )));
    assert!(calibration_map::is_coverage_map_flat_workspace_crate_module(Path::new(
        "crates/ruff_text_size/lib.rs"
    )));
    assert!(!calibration_map::is_coverage_map_flat_workspace_crate_module(Path::new(
        "crates/ruff/src/lib.rs"
    )));
    assert!(calibration_map::is_coverage_map_workspace_crate_flags_tree(Path::new(
        "crates/ruff/flags/doc.rs"
    )));
    assert!(calibration_map::is_workspace_llvm_auxiliary_crate_for_test("ty", 1));
    assert!(!calibration_map::is_workspace_llvm_auxiliary_crate_for_test("core", 1));
}

#[test]
fn coverage_map_plugin_witness_helpers() {
    use super::coverage_map_unreferenced::{
        build_witnessed_rule_plugins, coverage_map_cli_route_witnessed,
        coverage_map_plugin_rule_impl_witness, coverage_map_plugin_support_direct_only,
        coverage_map_plugin_support_plugin_witness, CoverageMapUnrefCtx,
    };

    let rule_def = sample_def(
        "unsafe_markup_use",
        PathBuf::from(
            "crates/ruff_linter/src/rules/flake8_bandit/rules/unsafe_markup_use.rs",
        ),
    );
    let helper_def = sample_def(
        "helper_fn",
        PathBuf::from("crates/ruff_linter/src/rules/flake8_bandit/helpers.rs"),
    );
    let cli_def = sample_def("run", PathBuf::from("src/cli/learn.rs"));
    let defs = vec![rule_def.clone(), helper_def.clone(), cli_def.clone()];
    let witness = HashSet::from([
        "unsafe_markup_use".to_string(),
        "helper_fn".to_string(),
        "run".to_string(),
    ]);
    let plugins = build_witnessed_rule_plugins(&defs, &witness);
    assert!(plugins.contains("flake8_bandit"));
    let name_files = crate::test_refs::build_name_file_map(
        defs.iter()
            .map(|d| (d.name.as_str(), d.file.as_path()))
            .collect::<Vec<_>>()
            .into_iter(),
    );
    let mut cli_attested = HashSet::new();
    cli_attested.insert(crate::rust_include::canonical_path(&cli_def.file));
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &HashSet::new(),
        defs_per_file: &HashMap::new(),
        cli_route_attested_files: &cli_attested,
        witnessed_rule_plugins: &plugins,
    };
    assert!(coverage_map_plugin_rule_impl_witness(&rule_def, &ctx));
    assert!(coverage_map_plugin_support_plugin_witness(&helper_def, &ctx));
    assert!(coverage_map_plugin_support_direct_only(&helper_def, &ctx));
    assert!(coverage_map_cli_route_witnessed(&cli_def, &ctx));
}
