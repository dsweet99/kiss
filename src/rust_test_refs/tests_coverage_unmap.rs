use super::coverage_map_unreferenced::{
    coverage_map_direct_test_witness, coverage_map_expanded_dense_file,
    coverage_map_forced_uncovered_file, coverage_map_integration_cone_witness,
    coverage_map_single_crate_cli_witnessed, definition_uncovered_for_coverage_map,
    CoverageMapUnrefCtx,
};
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
    };
    assert!(coverage_map_expanded_dense_file(&lib_def, &dense_ctx));
    assert!(!definition_uncovered_for_coverage_map(
        &lib_def,
        std::slice::from_ref(&lib_def),
        &ctx,
    ));
}
