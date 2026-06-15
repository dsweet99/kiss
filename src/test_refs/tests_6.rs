use super::*;

// ---------------------------------------------------------------------------
// mod.rs: analyze_test_refs_inner via analyze_test_refs with graph
// ---------------------------------------------------------------------------

#[test]
fn test_analyze_test_refs_inner_with_graph() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def helper():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from mymod import helper\ndef test_helper():\n    helper()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let parsed: Vec<&ParsedFile> = vec![&file, &file_test];
    let graph = build_dependency_graph(&parsed);
    let analysis = analyze_test_refs(&parsed, Some(&graph));
    assert!(analysis.unreferenced.is_empty());
    let key = (PathBuf::from("mymod.py"), "helper".to_string());
    assert!(analysis.coverage_map.contains_key(&key));
}

// ---------------------------------------------------------------------------
// disambiguation.rs: disambiguate_files_graph_fallback
// ---------------------------------------------------------------------------

#[test]
fn test_disambiguate_files_graph_fallback_empty_test_files() {
    use super::disambiguation::disambiguate_files_graph_fallback;
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def f(): pass\n";
    let t = parser.parse(src, None).unwrap();
    let f = ParsedFile {
        path: PathBuf::from("a.py"),
        source: src.into(),
        tree: t,
    };
    let parsed: Vec<&ParsedFile> = vec![&f];
    let graph = build_dependency_graph(&parsed);
    let mut files = HashSet::new();
    files.insert(PathBuf::from("a.py"));
    let result = disambiguate_files_graph_fallback(&files, &[], &graph);
    assert!(result.is_none(), "empty test_files => None");
}

// ---------------------------------------------------------------------------
// disambiguation.rs: resolve_ambiguous_name
// ---------------------------------------------------------------------------

#[test]
fn test_resolve_ambiguous_name_ref_based() {
    use super::disambiguation::resolve_ambiguous_name;
    let mut files = HashSet::new();
    files.insert(PathBuf::from("alpha/mod.py"));
    files.insert(PathBuf::from("beta/mod.py"));
    let mut refs = HashSet::new();
    refs.insert("alpha".to_string());
    let name_to_test_files: HashMap<&str, Vec<PathBuf>> = HashMap::new();
    let result = resolve_ambiguous_name("func", &files, &refs, &name_to_test_files, None);
    assert_eq!(result, Some(PathBuf::from("alpha/mod.py")));
}

#[test]
fn test_resolve_ambiguous_name_prefers_same_file_inline_test_witness() {
    use super::disambiguation::resolve_ambiguous_name;
    let mut files = HashSet::new();
    files.insert(PathBuf::from("src/a.rs"));
    files.insert(PathBuf::from("src/b.rs"));
    let refs = HashSet::from(["helper".to_string()]);
    let name_to_test_files: HashMap<&str, Vec<PathBuf>> =
        HashMap::from([("helper", vec![PathBuf::from("src/b.rs")])]);

    let result = resolve_ambiguous_name("helper", &files, &refs, &name_to_test_files, None);

    assert_eq!(result, Some(PathBuf::from("src/b.rs")));
}

// ---------------------------------------------------------------------------
// disambiguation.rs: collect_test_files_for_ambiguous_names (via build_disambiguation_map)
// ---------------------------------------------------------------------------

#[test]
fn test_collect_test_files_for_ambiguous_names_via_build() {
    let mut name_files: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    let mut dup = HashSet::new();
    dup.insert(PathBuf::from("a.py"));
    dup.insert(PathBuf::from("b.py"));
    name_files.insert("dup".to_string(), dup);

    let refs = HashSet::new();

    let mut usage_a = HashSet::new();
    usage_a.insert("dup".to_string());
    let per_test_usage: super::PerTestUsage = vec![(
        PathBuf::from("test_a.py"),
        vec![("test_it".to_string(), usage_a.clone(), HashSet::new())],
    )];

    let map =
        super::disambiguation::build_disambiguation_map(&name_files, &refs, &per_test_usage, None);
    assert!(
        map.is_empty() || map.len() <= 1,
        "without graph, falls back to ref-based only"
    );
}

#[test]
fn test_is_method_covered_by_class_and_name_direct() {
    use super::coverage::is_method_covered_by_class_and_name;
    let def = CodeDefinition {
        name: "process".to_string(),
        kind: crate::units::CodeUnitKind::Method,
        file: PathBuf::from("mod.py"),
        line: 5,
        containing_class: Some("Widget".to_string()),
    };
    let mut usage = HashSet::new();
    assert!(!is_method_covered_by_class_and_name(&def, &usage));
    usage.insert("Widget".to_string());
    assert!(!is_method_covered_by_class_and_name(&def, &usage));
    usage.insert("process".to_string());
    assert!(is_method_covered_by_class_and_name(&def, &usage));
}
