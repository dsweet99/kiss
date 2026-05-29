use super::*;
use crate::graph::DependencyGraph;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[test]
fn is_py_base_explicit_import_witnessed_requires_call() {
    use super::coverage::is_py_base_explicit_import_witnessed;
    let file = PathBuf::from("pkg/base/serializer.py");
    let def = CodeDefinition {
        name: "serialize".into(),
        kind: CodeUnitKind::Function,
        file: file.clone(),
        line: 1,
        end_line: 5,
        containing_class: None,
    };
    let mut import_bindings = HashMap::new();
    import_bindings.insert("pkg.base.serializer".into(), HashSet::from(["serialize".into()]));
    let mut module_suffixes = HashMap::new();
    module_suffixes.insert(file, "pkg.base.serializer".into());
    let import_only = HashSet::new();
    assert!(!is_py_base_explicit_import_witnessed(
        &def,
        &import_bindings,
        &module_suffixes,
        &import_only,
    ));
    let with_call = HashSet::from(["serialize".into()]);
    assert!(is_py_base_explicit_import_witnessed(
        &def,
        &import_bindings,
        &module_suffixes,
        &with_call,
    ));
}

#[test]
fn apply_import_dependency_calibration_uses_dep_witness() {
    use super::coverage::{apply_import_dependency_calibration, module_definition_counts};
    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(PathBuf::from("pkg/seed.py"), "pkg.seed".into());
    graph
        .path_to_module
        .insert(PathBuf::from("pkg/dep.py"), "pkg.dep".into());
    graph.add_dependency("pkg.seed", "pkg.dep");
    let seed = CodeDefinition {
        name: "seed_fn".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/seed.py"),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let dep = CodeDefinition {
        name: "dep_fn".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/dep.py"),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let usage = HashSet::from(["seed_fn".into(), "dep_fn".into()]);
    let name_files = crate::test_refs::build_name_file_map(
        [("seed_fn", seed.file.as_path()), ("dep_fn", dep.file.as_path())].into_iter(),
    );
    let mut analysis = TestRefAnalysis {
        definitions: vec![seed, dep],
        test_references: usage.clone(),
        unreferenced: vec![],
        coverage_map: HashMap::new(),
    };
    apply_import_dependency_calibration(&mut analysis, &graph, &usage, &name_files);
    assert!(analysis.unreferenced.is_empty());
    let counts = module_definition_counts(&analysis.definitions, &graph);
    assert_eq!(counts.get("pkg.dep").copied(), Some(1));
}

#[test]
fn oi_import_target_and_calibration_cap_branches() {
    use super::coverage::{apply_import_dependency_calibration, is_py_oi_module_import_witnessed};
    let oi_file = PathBuf::from("pkg/base/oi/interfaces.py");
    let evaluate_file = PathBuf::from("pkg/base/oi/evaluate.py");
    let oi_def = CodeDefinition {
        name: "IThing".into(),
        kind: CodeUnitKind::Class,
        file: oi_file.clone(),
        line: 1,
        end_line: 5,
        containing_class: None,
    };
    let mut import_bindings = HashMap::new();
    import_bindings.insert("pkg.base.oi".into(), HashSet::new());
    let mut module_suffixes = HashMap::new();
    module_suffixes.insert(oi_file.clone(), "pkg.base.oi.interfaces".into());
    module_suffixes.insert(evaluate_file.clone(), "pkg.base.oi.evaluate".into());
    assert!(is_py_oi_module_import_witnessed(
        &oi_def,
        &import_bindings,
        &module_suffixes,
    ));
    let root_db = PathBuf::from("pkg/base/oi/objectdb.py");
    let root_def = CodeDefinition {
        name: "ObjectDB".into(),
        kind: CodeUnitKind::Class,
        file: root_db.clone(),
        line: 1,
        end_line: 5,
        containing_class: None,
    };
    module_suffixes.insert(root_db, "pkg.base.oi.objectdb".into());
    assert!(!is_py_oi_module_import_witnessed(
        &root_def,
        &import_bindings,
        &module_suffixes,
    ));
    let mut eval_def = oi_def.clone();
    eval_def.file = evaluate_file;
    eval_def.name = "score".into();
    assert!(!is_py_oi_module_import_witnessed(
        &eval_def,
        &import_bindings,
        &module_suffixes,
    ));

    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(oi_file.clone(), "pkg.base.oi.interfaces".into());
    graph
        .path_to_module
        .insert(PathBuf::from("pkg/hub.py"), "pkg.hub".into());
    graph.add_dependency("pkg.base.oi.interfaces", "pkg.hub");
    let oi_fn = CodeDefinition {
        name: "oi_fn".into(),
        kind: CodeUnitKind::Function,
        file: oi_file,
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let hub_fn = CodeDefinition {
        name: "hub_fn".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/hub.py"),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let usage = HashSet::from(["oi_fn".into(), "hub_fn".into()]);
    let name_files = crate::test_refs::build_name_file_map(
        [("oi_fn", oi_fn.file.as_path()), ("hub_fn", hub_fn.file.as_path())].into_iter(),
    );
    let mut analysis = TestRefAnalysis {
        definitions: vec![oi_fn, hub_fn],
        test_references: usage.clone(),
        unreferenced: vec![],
        coverage_map: HashMap::new(),
    };
    apply_import_dependency_calibration(&mut analysis, &graph, &usage, &name_files);
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn module_definition_counts_skips_unmapped_files() {
    use super::coverage::module_definition_counts;
    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(PathBuf::from("pkg/mapped.py"), "pkg.mapped".into());
    let mapped = CodeDefinition {
        name: "f".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/mapped.py"),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let orphan = CodeDefinition {
        name: "g".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("orphan.py"),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let mapped2 = CodeDefinition {
        name: "h".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("pkg/mapped.py"),
        line: 2,
        end_line: 2,
        containing_class: None,
    };
    let counts = module_definition_counts(&[mapped, mapped2, orphan], &graph);
    assert_eq!(counts.get("pkg.mapped").copied(), Some(2));
    assert_eq!(counts.len(), 1);
}

#[test]
fn analyze_test_refs_for_coverage_map_with_attested_and_optimistic_bounds() {
    use crate::parsing::{create_parser, parse_file};
    use super::calibration_analysis::CalibrationCoverageBound;
    let mut parser = create_parser().expect("parser");
    let src = "def prod():\n    pass\n";
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").expect("tmp");
    std::io::Write::write_all(&mut tmp, src.as_bytes()).expect("write");
    let parsed = parse_file(&mut parser, tmp.path()).expect("parse");
    let refs = [&parsed];
    let attested = analyze_test_refs_for_coverage_map_with_bound(
        &refs,
        None,
        CalibrationCoverageBound::Attested,
    );
    let optimistic = analyze_test_refs_for_coverage_map_with_bound(
        &refs,
        None,
        CalibrationCoverageBound::Optimistic,
    );
    assert_eq!(attested.definitions.len(), 1);
    assert_eq!(optimistic.definitions.len(), 1);
}

fn rope_base_exceptions_import_ctx(
    path: &std::path::Path,
) -> (
    HashMap<String, HashSet<String>>,
    HashMap<PathBuf, String>,
) {
    let mut import_bindings = HashMap::new();
    import_bindings.insert(
        "rope.base.exceptions".into(),
        HashSet::from(["ResourceNotFoundError".into()]),
    );
    let mut module_suffixes = HashMap::new();
    module_suffixes.insert(path.to_path_buf(), "rope.base.exceptions".into());
    (import_bindings, module_suffixes)
}

#[test]
fn base_tree_credits_with_strict_witness_after_void_split() {
    use super::calibration_analysis::{
        build_calibration_coverage_refs, filter_unreferenced_definitions, UnreferencedFilterCtx,
    };
    let path = PathBuf::from("rope/base/exceptions.py");
    let def = CodeDefinition {
        name: "ResourceNotFoundError".into(),
        kind: CodeUnitKind::Class,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let witnesses = HashSet::from(["ResourceNotFoundError".to_string()]);
    let (strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let (import_bindings, module_suffixes) = rope_base_exceptions_import_ctx(&path);
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        coverage_bound: CalibrationCoverageBound::Shipped,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &strict,
        calibration_expanded_refs: &expanded,
        void_dispatch_attestation: &HashMap::new(),
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &import_bindings,
        module_suffixes: &module_suffixes,
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert!(
        unref.is_empty(),
        "base/ is no longer void-force-uncovered; strict witnesses should credit"
    );
}

#[test]
fn base_tree_strict_witness_can_cover_with_dispatch_attestation() {
    use super::calibration_analysis::{
        build_calibration_coverage_refs, filter_unreferenced_definitions, UnreferencedFilterCtx,
    };
    let path = PathBuf::from("rope/base/exceptions.py");
    let def = CodeDefinition {
        name: "ResourceNotFoundError".into(),
        kind: CodeUnitKind::Class,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let witnesses = HashSet::from(["ResourceNotFoundError".to_string()]);
    let (strict, expanded, counts) =
        build_calibration_coverage_refs(&[], std::slice::from_ref(&def), &witnesses);
    let mut attestation = HashMap::new();
    attestation.insert(
        path.clone(),
        HashSet::from(["ResourceNotFoundError".to_string()]),
    );
    let (import_bindings, module_suffixes) = rope_base_exceptions_import_ctx(&path);
    let ctx = UnreferencedFilterCtx {
        calibration: true,
        coverage_bound: CalibrationCoverageBound::Shipped,
        defs_per_file: &counts,
        test_witness_refs: &witnesses,
        calibration_strict_refs: &strict,
        calibration_expanded_refs: &expanded,
        void_dispatch_attestation: &attestation,
        usage_references: &HashSet::new(),
        name_files: &HashMap::new(),
        disambiguation: &HashMap::new(),
        import_bindings: &import_bindings,
        module_suffixes: &module_suffixes,
    };
    let unref = filter_unreferenced_definitions(&[def], &ctx);
    assert!(unref.is_empty());
}
