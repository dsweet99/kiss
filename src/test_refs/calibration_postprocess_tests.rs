use super::*;
use crate::parsing::{create_parser, parse_file};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn calibration_witness_refs_ignores_gated_test_file() {
    let mut gated = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(
        gated,
        "import sys\nif sys.platform != 'win32':\n    def test_x():\n        gated_only()\n"
    )
    .unwrap();
    let mut clean = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(clean, "def test_y():\n    clean_only()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let gated_p = parse_file(&mut parser, gated.path()).expect("parse");
    let clean_p = parse_file(&mut parser, clean.path()).expect("parse");
    let parsed = [&gated_p, &clean_p];
    let (_, _, _, _, per_test) = collect_refs_parallel_for_coverage_map(&parsed);
    let refs = calibration_witness_refs(&parsed, &per_test);
    assert!(refs.contains("clean_only"));
    assert!(!refs.contains("gated_only"));
}

#[test]
fn apply_calibration_postprocessing_runs_without_graph() {
    let mut analysis = TestRefAnalysis {
        definitions: vec![],
        test_references: HashSet::new(),
        unreferenced: vec![],
        coverage_map: HashMap::new(),
    };
    apply_calibration_postprocessing(
        &mut analysis,
        &CalibrationPostprocessCtx {
            parsed_files: &[],
            per_test_usage: &Vec::new(),
            name_files: &HashMap::new(),
            disambiguation: &HashMap::new(),
            import_bindings: &HashMap::new(),
            module_suffixes: &HashMap::new(),
            graph: None,
            test_witness_refs: &HashSet::new(),
        },
    );
}
