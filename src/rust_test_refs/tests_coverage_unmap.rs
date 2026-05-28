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
