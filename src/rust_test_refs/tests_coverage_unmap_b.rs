use super::calibration_map;
use super::coverage_map_unreferenced::{
    coverage_map_plugin_support_cone_witness, definition_uncovered_for_coverage_map,
    is_coverage_map_integration_cone_inflation_shim, CoverageMapUnrefCtx,
};
use super::coverage_map_excluded_file;
use super::RustCodeDefinition;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[test]
fn integration_cone_inflation_segment_paths() {
    assert!(is_coverage_map_integration_cone_inflation_shim(Path::new(
        "src/tool_summary/format.rs"
    )));
    assert!(is_coverage_map_integration_cone_inflation_shim(Path::new(
        "src/cli/mod.rs"
    )));
}

#[test]
fn plugin_support_cone_witness_and_json_omit() {
    let helper_def = RustCodeDefinition {
        name: "format_msg".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("crates/ruff_linter/src/rules/flake8_bandit/helpers.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    };
    let mut counts = HashMap::new();
    counts.insert(helper_def.file.clone(), 3);
    let plugins = HashSet::from(["flake8_bandit".to_string()]);
    let name_files = crate::test_refs::build_name_file_map(
        [(helper_def.name.as_str(), helper_def.file.as_path())].into_iter(),
    );
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &HashSet::new(),
        coverage_references: &HashSet::new(),
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &HashSet::new(),
        defs_per_file: &counts,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &plugins,
    };
    assert!(calibration_map::is_coverage_map_rule_plugin_support_file(&helper_def.file));
    assert!(coverage_map_plugin_support_cone_witness(&helper_def, &ctx));
    assert!(!definition_uncovered_for_coverage_map(
        &helper_def,
        std::slice::from_ref(&helper_def),
        &ctx,
    ));
    assert!(coverage_map_excluded_file(&helper_def.file));
    assert!(coverage_map_excluded_file(Path::new(
        "crates/ruff_python_stdlib/src/sys/builtin_modules.rs"
    )));
}

#[test]
fn rule_impl_type_attestation_requires_method_name_when_file_has_multiple_defs() {
    use super::coverage_map_unreferenced::{
        coverage_map_plugin_rule_impl_type_attestation, CoverageMapUnrefCtx,
    };
    let file = PathBuf::from("crates/linter/src/rules/widget_plugin/rules/guard.rs");
    let impl_witness = HashSet::from(["Widget".to_string()]);
    let defs = [
        RustCodeDefinition {
            name: "check".into(),
            kind: CodeUnitKind::Method,
            file: file.clone(),
            line: 1,
            end_line: 1,
            impl_for_type: Some("Widget".into()),
        },
        RustCodeDefinition {
            name: "extra".into(),
            kind: CodeUnitKind::Method,
            file: file.clone(),
            line: 2,
            end_line: 2,
            impl_for_type: Some("Widget".into()),
        },
    ];
    let name_files = crate::test_refs::build_name_file_map(
        defs.iter().map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let defs_per_file = HashMap::from([(file.clone(), defs.len())]);
    let ctx = CoverageMapUnrefCtx {
        test_witness_refs: &impl_witness,
        coverage_references: &impl_witness,
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &HashSet::new(),
        defs_per_file: &defs_per_file,
        cli_route_attested_files: &HashSet::new(),
        witnessed_rule_plugins: &HashSet::new(),
    };
    assert!(
        !coverage_map_plugin_rule_impl_type_attestation(&defs[1], &ctx),
        "impl-type witness alone must not credit every method in multi-def rule files"
    );
    let mut per_method = impl_witness.clone();
    per_method.insert("extra".into());
    let ctx_named = CoverageMapUnrefCtx {
        coverage_references: &per_method,
        ..ctx
    };
    assert!(coverage_map_plugin_rule_impl_type_attestation(
        &defs[1],
        &ctx_named
    ));
}
