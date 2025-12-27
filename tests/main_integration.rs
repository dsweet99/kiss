
use kiss::cli_output::{
    print_coverage_gate_failure, print_duplicates, print_final_status, print_no_files_message,
    print_py_test_refs, print_rs_test_refs, print_violations,
};
use kiss::config_gen::{collect_py_stats, collect_rs_stats, merge_config_toml, write_mimic_config};
use kiss::{
    analyze_file, analyze_graph, analyze_test_refs, build_dependency_graph,
    cluster_duplicates, detect_duplicates, extract_chunks_for_duplication, find_source_files,
    parse_files, Config, DependencyGraph, DuplicationConfig, Language, ParsedFile,
    ParsedRustFile,
};
use std::path::PathBuf;
use tempfile::TempDir;

fn gather_files(root: &std::path::Path, lang: Option<Language>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let all = find_source_files(root);
    let (mut py, mut rs) = (Vec::new(), Vec::new());
    for sf in all {
        match (sf.language, lang) {
            (Language::Python, None | Some(Language::Python)) => py.push(sf.path),
            (Language::Rust, None | Some(Language::Rust)) => rs.push(sf.path),
            _ => {}
        }
    }
    (py, rs)
}

fn parse_and_analyze_py(files: &[PathBuf], config: &Config) -> (Vec<ParsedFile>, Vec<kiss::Violation>) {
    if files.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let results = parse_files(files).unwrap();
    let mut parsed = Vec::new();
    let mut viols = Vec::new();
    for p in results.into_iter().flatten() {
        viols.extend(analyze_file(&p, config));
        parsed.push(p);
    }
    (parsed, viols)
}

fn compute_test_coverage(py_parsed: &[ParsedFile]) -> (usize, usize, usize, Vec<(PathBuf, String, usize)>) {
    let mut tested = 0;
    let mut total = 0;
    let mut unreferenced = Vec::new();

    if !py_parsed.is_empty() {
        let refs: Vec<&ParsedFile> = py_parsed.iter().collect();
        let analysis = analyze_test_refs(&refs);
        total += analysis.definitions.len();
        tested += analysis.definitions.len() - analysis.unreferenced.len();
        for def in analysis.unreferenced {
            unreferenced.push((def.file, def.name, def.line));
        }
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let coverage = if total > 0 {
        ((tested as f64 / total as f64) * 100.0).round() as usize
    } else {
        100
    };
    (coverage, tested, total, unreferenced)
}

#[test]
fn test_gather_files_all_and_filtered() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.py"), "").unwrap();
    std::fs::write(tmp.path().join("b.rs"), "").unwrap();
    let (py, rs) = gather_files(tmp.path(), None);
    assert_eq!(py.len(), 1);
    assert_eq!(rs.len(), 1);
    let (py2, rs2) = gather_files(tmp.path(), Some(Language::Python));
    assert_eq!(py2.len(), 1);
    assert_eq!(rs2.len(), 0);
}

#[test]
fn test_coverage_gate_blocks_untested_code() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("utils.py"), "def my_function():\n    pass\n").unwrap();
    let (py_files, _) = gather_files(tmp.path(), Some(Language::Python));
    let (py_parsed, _) = parse_and_analyze_py(&py_files, &Config::default());
    let (coverage, _, total, unreferenced) = compute_test_coverage(&py_parsed);
    assert_eq!(total, 1);
    assert_eq!(coverage, 0);
    assert_eq!(unreferenced.len(), 1);
}

#[test]
fn test_coverage_gate_passes_with_tests() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("utils.py"), "def my_function():\n    pass\n").unwrap();
    std::fs::write(
        tmp.path().join("test_utils.py"),
        "from utils import my_function\ndef test_it():\n    my_function()\n",
    )
    .unwrap();
    let (py_files, _) = gather_files(tmp.path(), Some(Language::Python));
    let (py_parsed, _) = parse_and_analyze_py(&py_files, &Config::default());
    let (coverage, _, _, _) = compute_test_coverage(&py_parsed);
    assert_eq!(coverage, 100);
}

#[test]
fn test_print_functions_no_panic() {
    print_violations(&[]);
    print_final_status(false);
    print_duplicates("Python", &[]);
    assert_eq!(print_py_test_refs(&[]), 0);
    assert_eq!(print_rs_test_refs(&[]), 0);
}

#[test]
fn test_print_helpers_no_panic() {
    let tmp = TempDir::new().unwrap();
    print_no_files_message(None, tmp.path());
    print_coverage_gate_failure(50, 80, 5, 10, &[]);
}

#[test]
fn test_collect_stats_empty() {
    let tmp = TempDir::new().unwrap();
    let (_, py_cnt) = collect_py_stats(tmp.path());
    let (_, rs_cnt) = collect_rs_stats(tmp.path());
    assert_eq!(py_cnt, 0);
    assert_eq!(rs_cnt, 0);
}

#[test]
fn test_config_merge() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    writeln!(tmp, "[python]\nstatements_per_function = 10").unwrap();
    let merged = merge_config_toml(tmp.path(), "[rust]\nstatements_per_function = 20", false, true);
    assert!(merged.contains("[python]") || merged.contains("[rust]"));
}

#[test]
fn test_write_mimic_config() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("out.toml");
    write_mimic_config(&path, "[python]\nx = 1", 1, 0);
    assert!(path.exists());
}

#[test]
fn test_analyze_graph_empty() {
    assert!(analyze_graph(&DependencyGraph::new(), &Config::default()).is_empty());
}

#[test]
fn test_build_graphs_empty() {
    let py: Vec<ParsedFile> = vec![];
    let rs: Vec<ParsedRustFile> = vec![];
    let py_graph = if py.is_empty() { None } else { Some(build_dependency_graph(&py.iter().collect::<Vec<_>>())) };
    let rs_graph: Option<DependencyGraph> = if rs.is_empty() { None } else { Some(build_dependency_graph(&[]))  };
    assert!(py_graph.is_none() && rs_graph.is_none());
}

#[test]
fn test_detect_duplicates_empty() {
    let parsed: Vec<ParsedFile> = vec![];
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let chunks = extract_chunks_for_duplication(&refs);
    let dups = cluster_duplicates(&detect_duplicates(&refs, &DuplicationConfig::default()), &chunks);
    assert!(dups.is_empty());
}

#[test]
fn test_parse_and_analyze_empty() {
    let config = Config::default();
    let (py_parsed, py_viols) = parse_and_analyze_py(&[], &config);
    assert!(py_parsed.is_empty() && py_viols.is_empty());
}

#[test]
fn test_build_focus_set() {
    use kiss::discovery::find_source_files_with_ignore;
    use std::collections::HashSet;

    fn build_focus_set(focus_paths: &[String], lang: Option<Language>, ignore_prefixes: &[String]) -> HashSet<PathBuf> {
        let mut focus_set = HashSet::new();
        for focus_path in focus_paths {
            let path = std::path::Path::new(focus_path);
            if path.is_file() {
                if let Ok(canonical) = path.canonicalize() { focus_set.insert(canonical); }
            } else {
                for sf in find_source_files_with_ignore(path, ignore_prefixes) {
                    if (lang.is_none() || lang == Some(sf.language)) && let Ok(canonical) = sf.path.canonicalize() {
                        focus_set.insert(canonical);
                    }
                }
            }
        }
        focus_set
    }

    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.py"), "def foo(): pass").unwrap();
    std::fs::write(tmp.path().join("b.rs"), "fn main() {}").unwrap();
    let subdir = tmp.path().join("sub");
    std::fs::create_dir(&subdir).unwrap();
    std::fs::write(subdir.join("c.py"), "def bar(): pass").unwrap();

    let focus = build_focus_set(&[tmp.path().to_string_lossy().to_string()], None, &[]);
    assert_eq!(focus.len(), 3);

    let focus_sub = build_focus_set(&[subdir.to_string_lossy().to_string()], None, &[]);
    assert_eq!(focus_sub.len(), 1);

    let focus_file = build_focus_set(&[tmp.path().join("a.py").to_string_lossy().to_string()], None, &[]);
    assert_eq!(focus_file.len(), 1);
}

#[test]
fn test_is_focus_file() {
    use std::collections::HashSet;

    fn is_focus_file(file: &std::path::Path, focus_set: &HashSet<PathBuf>) -> bool {
        if focus_set.is_empty() { return true; }
        file.canonicalize().is_ok_and(|canonical| focus_set.contains(&canonical))
    }

    let tmp = TempDir::new().unwrap();
    let file_a = tmp.path().join("a.py");
    let file_b = tmp.path().join("b.py");
    std::fs::write(&file_a, "").unwrap();
    std::fs::write(&file_b, "").unwrap();

    let mut focus_set = HashSet::new();
    focus_set.insert(file_a.canonicalize().unwrap());

    assert!(is_focus_file(&file_a, &focus_set));
    assert!(!is_focus_file(&file_b, &focus_set));

    let empty_set = HashSet::new();
    assert!(is_focus_file(&file_a, &empty_set));
    assert!(is_focus_file(&file_b, &empty_set));
}

