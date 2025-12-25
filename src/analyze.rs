//! Core analysis logic for kiss

use std::path::{Path, PathBuf};

use kiss::{
    analyze_file, analyze_graph, analyze_rust_file, analyze_rust_test_refs, analyze_test_refs,
    build_dependency_graph, build_rust_dependency_graph, cluster_duplicates,
    detect_duplicates, detect_duplicates_from_chunks, extract_chunks_for_duplication,
    extract_rust_chunks_for_duplication, find_source_files_with_ignore, parse_files, parse_rust_files,
    Config, DependencyGraph, DuplicateCluster, DuplicationConfig, GateConfig, Language,
    ParsedFile, ParsedRustFile, Violation,
};
use kiss::cli_output::{
    print_coverage_gate_failure, print_duplicates, print_final_status, print_no_files_message,
    print_py_test_refs, print_rs_test_refs, print_violations,
};

pub fn run_analyze(
    path: &str, py_config: &Config, rs_config: &Config, lang_filter: Option<Language>,
    bypass_gate: bool, gate_config: &GateConfig, ignore_prefixes: &[String],
) -> bool {
    let root = Path::new(path);
    let (py_files, rs_files) = gather_files(root, lang_filter, ignore_prefixes);

    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(lang_filter, root);
        return true;
    }

    let (py_parsed, rs_parsed, mut viols) = parse_all(&py_files, &rs_files, py_config, rs_config);

    if !bypass_gate && !check_coverage_gate(&py_parsed, &rs_parsed, gate_config) {
        return false;
    }

    let (py_graph, rs_graph) = build_graphs(&py_parsed, &rs_parsed);
    viols.extend(analyze_graphs(py_graph.as_ref(), rs_graph.as_ref(), py_config, rs_config));

    print_all_results(&viols, &py_parsed, &rs_parsed)
}

pub fn gather_files(root: &Path, lang: Option<Language>, ignore_prefixes: &[String]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let all = find_source_files_with_ignore(root, ignore_prefixes);
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

pub fn parse_all(
    py_files: &[PathBuf], rs_files: &[PathBuf], py_config: &Config, rs_config: &Config,
) -> (Vec<ParsedFile>, Vec<ParsedRustFile>, Vec<Violation>) {
    let (py_parsed, mut viols) = parse_and_analyze_py(py_files, py_config);
    let (rs_parsed, rs_viols) = parse_and_analyze_rs(rs_files, rs_config);
    viols.extend(rs_viols);
    (py_parsed, rs_parsed, viols)
}

pub fn parse_and_analyze_py(files: &[PathBuf], config: &Config) -> (Vec<ParsedFile>, Vec<Violation>) {
    if files.is_empty() { return (Vec::new(), Vec::new()); }
    let results = match parse_files(files) {
        Ok(r) => r,
        Err(e) => { eprintln!("Failed to initialize Python parser: {e}"); return (Vec::new(), Vec::new()); }
    };
    let mut parsed = Vec::new();
    let mut viols = Vec::new();
    for result in results {
        match result {
            Ok(p) => { viols.extend(analyze_file(&p, config)); parsed.push(p); }
            Err(e) => eprintln!("Error parsing Python: {e}"),
        }
    }
    (parsed, viols)
}

pub fn parse_and_analyze_rs(files: &[PathBuf], config: &Config) -> (Vec<ParsedRustFile>, Vec<Violation>) {
    if files.is_empty() { return (Vec::new(), Vec::new()); }
    let mut parsed = Vec::new();
    let mut viols = Vec::new();
    for result in parse_rust_files(files) {
        match result {
            Ok(p) => { viols.extend(analyze_rust_file(&p, config)); parsed.push(p); }
            Err(e) => eprintln!("Error parsing Rust: {e}"),
        }
    }
    (parsed, viols)
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

pub fn analyze_graphs(py_graph: Option<&DependencyGraph>, rs_graph: Option<&DependencyGraph>, py_config: &Config, rs_config: &Config) -> Vec<Violation> {
    let mut viols = Vec::new();
    if let Some(g) = py_graph { viols.extend(analyze_graph(g, py_config)); }
    if let Some(g) = rs_graph { viols.extend(analyze_graph(g, rs_config)); }
    viols
}

pub fn check_coverage_gate(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile], gate_config: &GateConfig) -> bool {
    let (coverage, tested, total, unreferenced) = compute_test_coverage(py_parsed, rs_parsed);
    if coverage < gate_config.test_coverage_threshold {
        print_coverage_gate_failure(coverage, gate_config.test_coverage_threshold, tested, total, &unreferenced);
        return false;
    }
    true
}

pub fn compute_test_coverage(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> (usize, usize, usize, Vec<(PathBuf, String, usize)>) {
    let mut tested = 0;
    let mut total = 0;
    let mut unreferenced = Vec::new();

    if !py_parsed.is_empty() {
        let refs: Vec<&ParsedFile> = py_parsed.iter().collect();
        let analysis = analyze_test_refs(&refs);
        total += analysis.definitions.len();
        tested += analysis.definitions.len() - analysis.unreferenced.len();
        for def in analysis.unreferenced { unreferenced.push((def.file, def.name, def.line)); }
    }

    if !rs_parsed.is_empty() {
        let refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
        let analysis = analyze_rust_test_refs(&refs);
        total += analysis.definitions.len();
        tested += analysis.definitions.len() - analysis.unreferenced.len();
        for def in analysis.unreferenced { unreferenced.push((def.file, def.name, def.line)); }
    }

    unreferenced.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    let coverage = if total > 0 { ((tested as f64 / total as f64) * 100.0).round() as usize } else { 100 };
    (coverage, tested, total, unreferenced)
}

fn print_all_results(viols: &[Violation], py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> bool {
    let py_dups = detect_py_duplicates(py_parsed);
    let rs_dups = detect_rs_duplicates(rs_parsed);
    let dup_count = py_dups.len() + rs_dups.len();
    let has_violations = !viols.is_empty() || dup_count > 0;
    
    // Print violations and duplicates first
    print_violations(viols);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    
    // Print test coverage warnings (informational)
    let _ = print_py_test_refs(py_parsed) + print_rs_test_refs(rs_parsed);
    
    // Final status at the end for clear LLM signal
    print_final_status(has_violations);
    
    !has_violations
}

pub fn detect_py_duplicates(parsed: &[ParsedFile]) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let chunks = extract_chunks_for_duplication(&refs);
    cluster_duplicates(&detect_duplicates(&refs, &DuplicationConfig::default()), &chunks)
}

pub fn detect_rs_duplicates(parsed: &[ParsedRustFile]) -> Vec<DuplicateCluster> {
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let chunks = extract_rust_chunks_for_duplication(&refs);
    cluster_duplicates(&detect_duplicates_from_chunks(&chunks, &DuplicationConfig::default()), &chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_run_analyze_empty() {
        let tmp = TempDir::new().unwrap();
        assert!(run_analyze(&tmp.path().to_string_lossy(), &Config::default(), &Config::default(), None, true, &GateConfig::default(), &[]));
    }

    #[test]
    fn test_parse_and_analyze_empty() {
        let config = Config::default();
        let (py, py_v) = parse_and_analyze_py(&[], &config);
        let (rs, rs_v) = parse_and_analyze_rs(&[], &config);
        assert!(py.is_empty() && py_v.is_empty() && rs.is_empty() && rs_v.is_empty());
    }

    #[test]
    fn test_coverage_empty() {
        let (cov, tested, total, unref) = compute_test_coverage(&[], &[]);
        assert_eq!((tested, total, cov), (0, 0, 100));
        assert!(unref.is_empty());
        assert!(check_coverage_gate(&[], &[], &GateConfig { test_coverage_threshold: 0 }));
    }

    #[test]
    fn test_graphs_empty() {
        let (py_g, rs_g) = build_graphs(&[], &[]);
        assert!(py_g.is_none() && rs_g.is_none());
        assert!(analyze_graphs(None, None, &Config::default(), &Config::default()).is_empty());
    }

    #[test]
    fn test_duplicates_empty() {
        assert!(detect_py_duplicates(&[]).is_empty());
        assert!(detect_rs_duplicates(&[]).is_empty());
    }
}
