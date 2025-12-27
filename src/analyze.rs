
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kiss::{
    analyze_file, analyze_graph, analyze_rust_file, analyze_rust_test_refs, analyze_test_refs,
    build_dependency_graph, build_rust_dependency_graph, cluster_duplicates,
    detect_duplicates, detect_duplicates_from_chunks, extract_chunks_for_duplication,
    extract_rust_chunks_for_duplication, find_source_files_with_ignore, parse_files, parse_rust_files,
    extract_code_units, extract_rust_code_units, is_test_file, is_rust_test_file,
    Config, DependencyGraph, DuplicateCluster, DuplicationConfig, GateConfig, Language,
    ParsedFile, ParsedRustFile, Violation,
};
use kiss::cli_output::{
    print_coverage_gate_failure, print_duplicates, print_final_status, print_no_files_message,
    print_violations,
};

pub struct AnalyzeOptions<'a> {
    pub universe: &'a str,
    pub focus_paths: &'a [String],
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub lang_filter: Option<Language>,
    pub bypass_gate: bool,
    pub gate_config: &'a GateConfig,
    pub ignore_prefixes: &'a [String],
}

pub fn run_analyze(opts: &AnalyzeOptions<'_>) -> bool {
    let t0 = std::time::Instant::now();
    let universe_root = Path::new(opts.universe);
    let (py_files, rs_files) = gather_files(universe_root, opts.lang_filter, opts.ignore_prefixes);
    let t1 = std::time::Instant::now();

    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(opts.lang_filter, universe_root);
        return true;
    }

    let focus_set = build_focus_set(opts.focus_paths, opts.lang_filter, opts.ignore_prefixes);
    let result = parse_all(&py_files, &rs_files, opts.py_config, opts.rs_config);
    let t2 = std::time::Instant::now();
    let mut viols = filter_viols_by_focus(result.violations, &focus_set);

    if !opts.bypass_gate && !check_coverage_gate(&result.py_parsed, &result.rs_parsed, opts.gate_config, &focus_set) {
        return false;
    }

    let (py_graph, rs_graph) = build_graphs(&result.py_parsed, &result.rs_parsed);
    let t3 = std::time::Instant::now();
    log_timing_phase1(t0, t1, t2, t3);
    print_analysis_summary(result.py_parsed.len() + result.rs_parsed.len(), result.code_unit_count, py_graph.as_ref(), rs_graph.as_ref());

    viols.extend(collect_graph_viols(py_graph.as_ref(), rs_graph.as_ref(), opts.py_config, opts.rs_config, &focus_set, result.py_parsed.len() + result.rs_parsed.len()));
    let t4 = std::time::Instant::now();

    if opts.bypass_gate { viols.extend(collect_coverage_viols(&result.py_parsed, &result.rs_parsed, &focus_set)); }
    eprintln!("[TIMING] graph_analysis={:.2}s, test_refs={:.2}s", t4.duration_since(t3).as_secs_f64(), std::time::Instant::now().duration_since(t4).as_secs_f64());

    print_all_results(&viols, &result.py_parsed, &result.rs_parsed, opts.gate_config.min_similarity, &focus_set)
}

fn filter_viols_by_focus(mut viols: Vec<Violation>, focus_set: &HashSet<PathBuf>) -> Vec<Violation> {
    viols.retain(|v| is_focus_file(&v.file, focus_set));
    viols
}

fn log_timing_phase1(t0: std::time::Instant, t1: std::time::Instant, t2: std::time::Instant, t3: std::time::Instant) {
    eprintln!("[TIMING] discovery={:.2}s, parse+analyze={:.2}s, coverage=0.00s, graph={:.2}s",
        t1.duration_since(t0).as_secs_f64(), t2.duration_since(t1).as_secs_f64(), t3.duration_since(t2).as_secs_f64());
}

fn collect_graph_viols(py_graph: Option<&DependencyGraph>, rs_graph: Option<&DependencyGraph>, py_config: &Config, rs_config: &Config, focus_set: &HashSet<PathBuf>, file_count: usize) -> Vec<Violation> {
    if file_count <= 1 { return Vec::new(); }
    let mut viols = analyze_graphs(py_graph, rs_graph, py_config, rs_config);
    viols.retain(|v| is_focus_file(&v.file, focus_set));
    viols
}

fn collect_coverage_viols(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], focus_set: &HashSet<PathBuf>) -> Vec<Violation> {
    compute_test_coverage(py_parsed, rs_parsed, focus_set).3.into_iter()
        .map(|(file, name, line)| Violation {
            file, line, unit_name: name, metric: "test_coverage".to_string(), value: 0, threshold: 0,
            message: "Add test coverage for this code unit.".to_string(), suggestion: String::new(),
        })
        .collect()
}

pub fn gather_files(root: &Path, lang: Option<Language>, ignore_prefixes: &[String]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let all = find_source_files_with_ignore(root, ignore_prefixes);
    let (mut py, mut rs) = (Vec::new(), Vec::new());
    for sf in all {
        let path = sf.path.canonicalize().unwrap_or(sf.path);
        match (sf.language, lang) {
            (Language::Python, None | Some(Language::Python)) => py.push(path),
            (Language::Rust, None | Some(Language::Rust)) => rs.push(path),
            _ => {}
        }
    }
    (py, rs)
}

pub fn build_focus_set(focus_paths: &[String], lang: Option<Language>, ignore_prefixes: &[String]) -> HashSet<PathBuf> {
    let mut focus_set = HashSet::new();
    for focus_path in focus_paths {
        let path = Path::new(focus_path);
        if path.is_file() {
            if let Ok(canonical) = path.canonicalize() {
                focus_set.insert(canonical);
            }
        } else {
            let (py, rs) = gather_files(path, lang, ignore_prefixes);
            focus_set.extend(py);
            focus_set.extend(rs);
        }
    }
    focus_set
}

pub fn is_focus_file(file: &Path, focus_set: &HashSet<PathBuf>) -> bool {
    focus_set.is_empty() || focus_set.contains(file)
}

pub struct ParseResult {
    pub py_parsed: Vec<ParsedFile>,
    pub rs_parsed: Vec<ParsedRustFile>,
    pub violations: Vec<Violation>,
    pub code_unit_count: usize,
}

pub fn parse_all(py_files: &[PathBuf], rs_files: &[PathBuf], py_config: &Config, rs_config: &Config) -> ParseResult {
    let (py_parsed, mut viols, py_units) = parse_and_analyze_py(py_files, py_config);
    let (rs_parsed, rs_viols, rs_units) = parse_and_analyze_rs(rs_files, rs_config);
    viols.extend(rs_viols);
    ParseResult { py_parsed, rs_parsed, violations: viols, code_unit_count: py_units + rs_units }
}

pub fn parse_and_analyze_py(files: &[PathBuf], config: &Config) -> (Vec<ParsedFile>, Vec<Violation>, usize) {
    if files.is_empty() { return (Vec::new(), Vec::new(), 0); }
    let results = match parse_files(files) {
        Ok(r) => r,
        Err(e) => { eprintln!("Failed to initialize Python parser: {e}"); return (Vec::new(), Vec::new(), 0); }
    };
    let (mut parsed, mut viols, mut unit_count) = (Vec::new(), Vec::new(), 0);
    for result in results {
        match result {
            Ok(p) => {
                unit_count += extract_code_units(&p).len();
                if !is_test_file(&p.path) { viols.extend(analyze_file(&p, config)); }
                parsed.push(p);
            }
            Err(e) => eprintln!("Error parsing Python: {e}"),
        }
    }
    (parsed, viols, unit_count)
}

pub fn parse_and_analyze_rs(files: &[PathBuf], config: &Config) -> (Vec<ParsedRustFile>, Vec<Violation>, usize) {
    if files.is_empty() { return (Vec::new(), Vec::new(), 0); }
    let (mut parsed, mut viols, mut unit_count) = (Vec::new(), Vec::new(), 0);
    for result in parse_rust_files(files) {
        match result {
            Ok(p) => {
                unit_count += extract_rust_code_units(&p).len();
                if !is_rust_test_file(&p.path) { viols.extend(analyze_rust_file(&p, config)); }
                parsed.push(p);
            }
            Err(e) => eprintln!("Error parsing Rust: {e}"),
        }
    }
    (parsed, viols, unit_count)
}

pub fn build_graphs(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> (Option<DependencyGraph>, Option<DependencyGraph>) {
    let py_graph = if py_parsed.is_empty() { None } else {
        Some(build_dependency_graph(&py_parsed.iter().collect::<Vec<_>>()))
    };
    let rs_graph = if rs_parsed.is_empty() { None } else {
        Some(build_rust_dependency_graph(&rs_parsed.iter().collect::<Vec<_>>()))
    };
    (py_graph, rs_graph)
}

fn print_analysis_summary(file_count: usize, unit_count: usize, py_g: Option<&DependencyGraph>, rs_g: Option<&DependencyGraph>) {
    let (nodes, edges) = graph_stats(py_g, rs_g);
    println!("Analyzed: {file_count} files, {unit_count} code units, {nodes} graph nodes, {edges} graph edges");
}

fn graph_stats(py_g: Option<&DependencyGraph>, rs_g: Option<&DependencyGraph>) -> (usize, usize) {
    let (mut nodes, mut edges) = (0, 0);
    if let Some(g) = py_g { nodes += g.graph.node_count(); edges += g.graph.edge_count(); }
    if let Some(g) = rs_g { nodes += g.graph.node_count(); edges += g.graph.edge_count(); }
    (nodes, edges)
}

pub fn analyze_graphs(py_graph: Option<&DependencyGraph>, rs_graph: Option<&DependencyGraph>, py_config: &Config, rs_config: &Config) -> Vec<Violation> {
    let mut viols = Vec::new();
    if let Some(g) = py_graph { viols.extend(analyze_graph(g, py_config)); }
    if let Some(g) = rs_graph { viols.extend(analyze_graph(g, rs_config)); }
    viols
}

pub fn check_coverage_gate(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], gate_config: &GateConfig, focus_set: &HashSet<PathBuf>) -> bool {
    let (coverage, tested, total, unreferenced) = compute_test_coverage(py_parsed, rs_parsed, focus_set);
    if coverage < gate_config.test_coverage_threshold {
        print_coverage_gate_failure(coverage, gate_config.test_coverage_threshold, tested, total, &unreferenced);
        return false;
    }
    true
}

pub fn compute_test_coverage(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], focus_set: &HashSet<PathBuf>) -> (usize, usize, usize, Vec<(PathBuf, String, usize)>) {
    let (mut tested, mut total, mut unreferenced) = (0, 0, Vec::new());

    if !py_parsed.is_empty() {
        let a = analyze_test_refs(&py_parsed.iter().collect::<Vec<_>>());
        let defs: Vec<_> = a.definitions.iter().map(|d| (&d.file, &d.name, d.line)).collect();
        let unref: Vec<_> = a.unreferenced.iter().map(|d| (&d.file, &d.name, d.line)).collect();
        tally_coverage(&defs, &unref, focus_set, &mut tested, &mut total, &mut unreferenced);
    }
    if !rs_parsed.is_empty() {
        let a = analyze_rust_test_refs(&rs_parsed.iter().collect::<Vec<_>>());
        let defs: Vec<_> = a.definitions.iter().map(|d| (&d.file, &d.name, d.line)).collect();
        let unref: Vec<_> = a.unreferenced.iter().map(|d| (&d.file, &d.name, d.line)).collect();
        tally_coverage(&defs, &unref, focus_set, &mut tested, &mut total, &mut unreferenced);
    }

    unreferenced.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let coverage = if total > 0 { ((tested as f64 / total as f64) * 100.0).round() as usize } else { 100 };
    (coverage, tested, total, unreferenced)
}

fn tally_coverage(defs: &[(&PathBuf, &String, usize)], unref: &[(&PathBuf, &String, usize)], focus_set: &HashSet<PathBuf>, tested: &mut usize, total: &mut usize, unreferenced: &mut Vec<(PathBuf, String, usize)>) {
    for (file, name, line) in defs {
        if !is_focus_file(file, focus_set) { continue; }
        *total += 1;
        if !unref.iter().any(|(f, n, l)| f == file && n == name && l == line) { *tested += 1; }
    }
    for (file, name, line) in unref {
        if is_focus_file(file, focus_set) { unreferenced.push(((*file).clone(), (*name).clone(), *line)); }
    }
}

fn print_all_results(viols: &[Violation], py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], min_similarity: f64, focus_set: &HashSet<PathBuf>) -> bool {
    let t0 = std::time::Instant::now();
    let py_dups = filter_duplicates_by_focus(detect_py_duplicates(py_parsed, min_similarity), focus_set);
    let rs_dups = filter_duplicates_by_focus(detect_rs_duplicates(rs_parsed, min_similarity), focus_set);
    let t1 = std::time::Instant::now();
    let dup_count = py_dups.len() + rs_dups.len();
    
    print_violations(viols);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    let t2 = std::time::Instant::now();
    eprintln!("[TIMING] dup_detect={:.2}s, output={:.2}s", t1.duration_since(t0).as_secs_f64(), t2.duration_since(t1).as_secs_f64());
    
    let has_violations = !viols.is_empty() || dup_count > 0;
    print_final_status(has_violations);
    
    !has_violations
}

fn filter_duplicates_by_focus(dups: Vec<DuplicateCluster>, focus_set: &HashSet<PathBuf>) -> Vec<DuplicateCluster> {
    dups.into_iter()
        .filter(|cluster| cluster.chunks.iter().any(|c| is_focus_file(&c.file, focus_set)))
        .collect()
}

pub fn detect_py_duplicates(parsed: &[ParsedFile], min_similarity: f64) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let chunks = extract_chunks_for_duplication(&refs);
    let config = DuplicationConfig { min_similarity, ..Default::default() };
    cluster_duplicates(&detect_duplicates(&refs, &config), &chunks)
}

pub fn detect_rs_duplicates(parsed: &[ParsedRustFile], min_similarity: f64) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let chunks = extract_rust_chunks_for_duplication(&refs);
    let config = DuplicationConfig { min_similarity, ..Default::default() };
    cluster_duplicates(&detect_duplicates_from_chunks(&chunks, &config), &chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_analyze_options_struct() {
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let gate_cfg = GateConfig::default();
        let _ = AnalyzeOptions {
            universe: ".", focus_paths: &[], py_config: &py_cfg, rs_config: &rs_cfg,
            lang_filter: None, bypass_gate: false, gate_config: &gate_cfg,
            ignore_prefixes: &[],
        };
    }

    #[test]
    fn test_parse_result_struct() {
        let _ = ParseResult { py_parsed: vec![], rs_parsed: vec![], violations: vec![], code_unit_count: 0 };
    }

    #[test]
    fn test_gather_files_and_build_focus_set() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test.py"), "x=1").unwrap();
        let (py, rs) = gather_files(tmp.path(), None, &[]);
        assert_eq!(py.len(), 1);
        assert!(rs.is_empty());
        let focus = build_focus_set(&[tmp.path().to_string_lossy().to_string()], None, &[]);
        assert!(!focus.is_empty());
    }

    #[test]
    fn test_parse_all_and_analyze() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "def f(): pass").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "fn main() {}").unwrap();
        let py = vec![tmp.path().join("a.py")];
        let rs = vec![tmp.path().join("b.rs")];
        let result = parse_all(&py, &rs, &Config::python_defaults(), &Config::rust_defaults());
        assert_eq!(result.py_parsed.len(), 1);
        assert_eq!(result.rs_parsed.len(), 1);
    }

    #[test]
    fn test_build_graphs_and_analyze() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "import b\ndef f(): pass").unwrap();
        std::fs::write(tmp.path().join("b.py"), "x=1").unwrap();
        let py = vec![tmp.path().join("a.py"), tmp.path().join("b.py")];
        let result = parse_all(&py, &[], &Config::python_defaults(), &Config::rust_defaults());
        let (py_g, rs_g) = build_graphs(&result.py_parsed, &result.rs_parsed);
        assert!(py_g.is_some());
        assert!(rs_g.is_none());
        let viols = analyze_graphs(py_g.as_ref(), rs_g.as_ref(), &Config::python_defaults(), &Config::rust_defaults());
        let _ = viols; // may or may not have violations
    }

    #[test]
    fn test_coverage_gate_and_tally() {
        let gate = GateConfig { test_coverage_threshold: 0, ..Default::default() };
        let focus = HashSet::new();
        assert!(check_coverage_gate(&[], &[], &gate, &focus));
        let (cov, tested, total, unref) = compute_test_coverage(&[], &[], &focus);
        assert_eq!(cov, 100);
        assert_eq!(tested, 0);
        assert_eq!(total, 0);
        assert!(unref.is_empty());
        // tally_coverage
        let mut t = 0; let mut tot = 0; let mut u = vec![];
        tally_coverage(&[], &[], &HashSet::new(), &mut t, &mut tot, &mut u);
        assert_eq!(t, 0);
    }

    #[test]
    fn test_print_functions_and_helpers() {
        print_analysis_summary(0, 0, None, None);
        let (n, e) = graph_stats(None, None);
        assert_eq!(n, 0);
        assert_eq!(e, 0);
        assert!(is_focus_file(Path::new("any.py"), &HashSet::new())); // empty focus = all
        let dups = filter_duplicates_by_focus(vec![], &HashSet::new());
        assert!(dups.is_empty());
    }

    #[test]
    fn test_detect_duplicates() {
        let py_dups = detect_py_duplicates(&[], 0.7);
        assert!(py_dups.is_empty());
        let rs_dups = detect_rs_duplicates(&[], 0.7);
        assert!(rs_dups.is_empty());
    }

    #[test]
    fn test_print_all_results() {
        let result = print_all_results(&[], &[], &[], 0.7, &HashSet::new());
        assert!(result); // no violations = true
    }

    #[test]
    fn test_run_analyze_no_files() {
        let tmp = TempDir::new().unwrap();
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let gate_cfg = GateConfig::default();
        let opts = AnalyzeOptions {
            universe: tmp.path().to_str().unwrap(), focus_paths: &[],
            py_config: &py_cfg, rs_config: &rs_cfg, lang_filter: None,
            bypass_gate: true, gate_config: &gate_cfg, ignore_prefixes: &[],
        };
        assert!(run_analyze(&opts)); // no files = success
    }

    #[test]
    fn test_parse_and_analyze_rs_directly() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("lib.rs"), "fn foo() { let x = 1; }").unwrap();
        let files = vec![tmp.path().join("lib.rs")];
        let (parsed, viols, units) = parse_and_analyze_rs(&files, &Config::rust_defaults());
        assert_eq!(parsed.len(), 1);
        assert!(viols.is_empty()); // simple code should have no violations
        assert!(units > 0);
    }
}
