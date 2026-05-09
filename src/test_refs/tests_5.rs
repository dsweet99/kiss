use super::*;
use std::path::Path;

// ---------------------------------------------------------------------------
// detection.rs: is_protocol_class
// ---------------------------------------------------------------------------

#[test]
fn test_is_protocol_class_direct() {
    use super::detection::is_protocol_class;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class Readable(Protocol):\n    def read(self) -> str: ...\n";
    let tree = parser.parse(src, None).unwrap();
    let class_node = tree.root_node().child(0).unwrap();
    assert!(is_protocol_class(class_node, src));

    let src2 = "class Normal(Base):\n    pass\n";
    let tree2 = parser.parse(src2, None).unwrap();
    let class_node2 = tree2.root_node().child(0).unwrap();
    assert!(!is_protocol_class(class_node2, src2));

    let src3 = "class Typed(typing.Protocol):\n    pass\n";
    let tree3 = parser.parse(src3, None).unwrap();
    let class_node3 = tree3.root_node().child(0).unwrap();
    assert!(is_protocol_class(class_node3, src3));
}

// ---------------------------------------------------------------------------
// detection.rs: is_abstract_method
// ---------------------------------------------------------------------------

#[test]
fn test_is_abstract_method_direct() {
    use super::detection::is_abstract_method;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class Base:\n    @abstractmethod\n    def work(self):\n        pass\n\n    def concrete(self):\n        pass\n";
    let tree = parser.parse(src, None).unwrap();
    let class_node = tree.root_node().child(0).unwrap();
    let body = class_node.child_by_field_name("body").unwrap();
    let mut cursor = body.walk();
    let mut abstract_found = false;
    let mut concrete_found = false;
    for child in body.children(&mut cursor) {
        if child.kind() == "decorated_definition" {
            let mut inner_cursor = child.walk();
            for inner in child.children(&mut inner_cursor) {
                if inner.kind() == "function_definition" {
                    assert!(is_abstract_method(inner, src));
                    abstract_found = true;
                }
            }
        } else if child.kind() == "function_definition" {
            assert!(!is_abstract_method(child, src));
            concrete_found = true;
        }
    }
    assert!(abstract_found, "found the abstract method");
    assert!(concrete_found, "found the concrete method");
}

// ---------------------------------------------------------------------------
// detection.rs: is_test_function, is_test_class
// ---------------------------------------------------------------------------

#[test]
fn test_is_test_function_direct() {
    use super::detection::is_test_function;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def test_foo():\n    pass\ndef helper():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let func1 = root.child(0).unwrap();
    let func2 = root.child(1).unwrap();
    assert!(is_test_function(func1, src));
    assert!(!is_test_function(func2, src));
}

#[test]
fn test_is_test_class_direct() {
    use super::detection::is_test_class;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class TestFoo:\n    pass\nclass Helper:\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let cls1 = root.child(0).unwrap();
    let cls2 = root.child(1).unwrap();
    assert!(is_test_class(cls1, src));
    assert!(!is_test_class(cls2, src));
}

// ---------------------------------------------------------------------------
// disambiguation.rs: path_identifiers
// ---------------------------------------------------------------------------

#[test]
fn test_path_identifiers_segments() {
    use super::disambiguation::path_identifiers;
    let ids = path_identifiers(Path::new("src/utils/parser.py"));
    assert_eq!(ids, vec!["src", "utils", "parser"]);
}

#[test]
fn test_path_identifiers_flat_file() {
    use super::disambiguation::path_identifiers;
    let ids = path_identifiers(Path::new("helpers.py"));
    assert_eq!(ids, vec!["helpers"]);
}

// ---------------------------------------------------------------------------
// disambiguation.rs: disambiguate_files
// ---------------------------------------------------------------------------

#[test]
fn test_disambiguate_files_unique_path_segment() {
    use super::disambiguation::disambiguate_files;
    let mut files = HashSet::new();
    files.insert(PathBuf::from("alpha/utils.py"));
    files.insert(PathBuf::from("beta/utils.py"));
    let mut refs = HashSet::new();
    refs.insert("alpha".to_string());
    let result = disambiguate_files(&files, &refs);
    assert_eq!(result, Some(PathBuf::from("alpha/utils.py")));
}

#[test]
fn test_disambiguate_files_both_match_returns_none() {
    use super::disambiguation::disambiguate_files;
    let mut files = HashSet::new();
    files.insert(PathBuf::from("alpha/utils.py"));
    files.insert(PathBuf::from("beta/utils.py"));
    let mut refs = HashSet::new();
    refs.insert("alpha".to_string());
    refs.insert("beta".to_string());
    let result = disambiguate_files(&files, &refs);
    assert!(result.is_none(), "two winners => None");
}

#[test]
fn test_disambiguate_files_no_match() {
    use super::disambiguation::disambiguate_files;
    let mut files = HashSet::new();
    files.insert(PathBuf::from("alpha/utils.py"));
    files.insert(PathBuf::from("beta/utils.py"));
    let refs = HashSet::new();
    let result = disambiguate_files(&files, &refs);
    assert!(result.is_none(), "no refs match => None");
}

// ---------------------------------------------------------------------------
// disambiguation.rs: file_to_module_suffix
// ---------------------------------------------------------------------------

#[test]
fn test_file_to_module_suffix_basic() {
    assert_eq!(
        file_to_module_suffix(Path::new("pkg/sub/mod.py")),
        "pkg.sub.mod"
    );
    assert_eq!(file_to_module_suffix(Path::new("mod.py")), "mod");
}

// ---------------------------------------------------------------------------
// disambiguation.rs: module_suffix_matches
// ---------------------------------------------------------------------------

#[test]
fn test_module_suffix_matches_exact_and_suffix() {
    use super::disambiguation::module_suffix_matches;
    assert!(module_suffix_matches("pkg.sub.mod", "pkg.sub.mod"));
    assert!(module_suffix_matches("pkg.sub.mod", "sub.mod"));
    assert!(module_suffix_matches("pkg.sub.mod", "mod"));
    assert!(!module_suffix_matches("pkg.sub.mod", "other.mod"));
    assert!(!module_suffix_matches("pkg.sub.mod", "ub.mod"));
}

// ---------------------------------------------------------------------------
// mod.rs: analyze_test_refs_quick vs analyze_test_refs
// ---------------------------------------------------------------------------

#[test]
fn test_analyze_test_refs_quick_no_coverage_map() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def run():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("engine.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from engine import run\ndef test_run():\n    run()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_engine.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let quick = analyze_test_refs_quick(&[&file, &file_test]);
    assert!(
        quick.coverage_map.is_empty(),
        "quick mode skips coverage_map"
    );
    assert!(quick.unreferenced.is_empty(), "run should be covered");

    let full = analyze_test_refs(&[&file, &file_test], None);
    assert!(
        !full.coverage_map.is_empty(),
        "full mode builds coverage_map"
    );
}

// ---------------------------------------------------------------------------
// mod.rs: TestRefAnalysis struct fields
// ---------------------------------------------------------------------------

#[test]
fn test_test_ref_analysis_struct_fields() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def alpha():\n    pass\ndef beta():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "def test_alpha():\n    alpha()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);

    assert_eq!(analysis.definitions.len(), 2);
    assert!(analysis.test_references.contains("alpha"));
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "beta");
}

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
        vec![("test_it".to_string(), usage_a)],
    )];

    let map =
        super::disambiguation::build_disambiguation_map(&name_files, &refs, &per_test_usage, None);
    assert!(
        map.is_empty() || map.len() <= 1,
        "without graph, falls back to ref-based only"
    );
}
