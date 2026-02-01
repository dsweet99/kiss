use crate::discovery::{Language, find_source_files_with_ignore};
use crate::duplication::{
    DuplicationConfig, cluster_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication,
};
use crate::gate_config::GateConfig;
use crate::graph::build_dependency_graph;
use crate::parsing::{ParsedFile, parse_files};
use crate::rust_graph::build_rust_dependency_graph;
use crate::rust_parsing::{ParsedRustFile, parse_rust_files};
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::stats::{MetricStats, PercentileSummary, compute_summaries};
use crate::test_refs::analyze_test_refs;
use std::path::{Path, PathBuf};

pub fn collect_py_stats(root: &Path) -> (MetricStats, usize) {
    collect_py_stats_with_ignore(root, &[])
}

pub fn collect_py_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    let py_files: Vec<_> = find_source_files_with_ignore(root, ignore)
        .into_iter()
        .filter(|sf| sf.language == Language::Python)
        .map(|sf| sf.path)
        .collect();
    if py_files.is_empty() {
        return (MetricStats::default(), 0);
    }
    let Ok(results) = parse_files(&py_files) else {
        return (MetricStats::default(), 0);
    };
    let parsed: Vec<ParsedFile> = results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();
    let cnt = parsed.len();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let mut stats = MetricStats::collect(&refs);
    stats.collect_graph_metrics(&build_dependency_graph(&refs));
    (stats, cnt)
}

pub fn collect_rs_stats(root: &Path) -> (MetricStats, usize) {
    collect_rs_stats_with_ignore(root, &[])
}

pub fn collect_rs_stats_with_ignore(root: &Path, ignore: &[String]) -> (MetricStats, usize) {
    let rs_files: Vec<_> = find_source_files_with_ignore(root, ignore)
        .into_iter()
        .filter(|sf| sf.language == Language::Rust)
        .map(|sf| sf.path)
        .collect();
    if rs_files.is_empty() {
        return (MetricStats::default(), 0);
    }
    let parsed: Vec<ParsedRustFile> = parse_rust_files(&rs_files)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();
    let cnt = parsed.len();
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let mut stats = MetricStats::collect_rust(&refs);
    stats.collect_graph_metrics(&build_rust_dependency_graph(&refs));
    (stats, cnt)
}

pub fn collect_all_stats(
    paths: &[String],
    lang: Option<Language>,
) -> ((MetricStats, usize), (MetricStats, usize)) {
    collect_all_stats_with_ignore(paths, lang, &[])
}

pub fn collect_all_stats_with_ignore(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> ((MetricStats, usize), (MetricStats, usize)) {
    let (mut py, mut rs) = ((MetricStats::default(), 0), (MetricStats::default(), 0));
    for path in paths {
        let root = Path::new(path);
        if lang.is_none() || lang == Some(Language::Python) {
            let (s, c) = collect_py_stats_with_ignore(root, ignore);
            py.0.merge(s);
            py.1 += c;
        }
        if lang.is_none() || lang == Some(Language::Rust) {
            let (s, c) = collect_rs_stats_with_ignore(root, ignore);
            rs.0.merge(s);
            rs.1 += c;
        }
    }
    (py, rs)
}

pub fn write_mimic_config(out: &Path, toml: &str, py_cnt: usize, rs_cnt: usize) {
    let content = if out.exists() {
        merge_config_toml(out, toml, py_cnt > 0, rs_cnt > 0)
    } else {
        toml.to_string()
    };
    if let Err(e) = std::fs::write(out, &content) {
        eprintln!("Error writing to {}: {}", out.display(), e);
        std::process::exit(1);
    }
    eprintln!(
        "Generated config from {} files → {}",
        py_cnt + rs_cnt,
        out.display()
    );
}

fn format_section(out: &mut String, name: &str, section: Option<&toml::Value>) {
    use std::fmt::Write;
    if let Some(v) = section {
        let _ = writeln!(out, "[{name}]");
        if let Some(t) = v.as_table() {
            for (k, v) in t {
                let _ = writeln!(out, "{k} = {v}");
            }
        }
        out.push('\n');
    }
}

pub fn merge_config_toml(path: &Path, new: &str, upd_py: bool, upd_rs: bool) -> String {
    let (Ok(ex_str), Ok(nw)) = (std::fs::read_to_string(path), new.parse::<toml::Table>()) else {
        return new.to_string();
    };
    let Ok(ex) = ex_str.parse::<toml::Table>() else {
        return new.to_string();
    };
    let pick = |k: &str, upd: bool| if upd { nw.get(k) } else { ex.get(k) }.cloned();
    let mut m = toml::Table::new();
    // Gate applies to all languages; always take the new value when present.
    if let Some(v) = nw.get("gate").cloned().or_else(|| ex.get("gate").cloned()) {
        m.insert("gate".to_string(), v);
    }
    for (k, upd) in [("python", upd_py), ("rust", upd_rs)] {
        if let Some(v) = pick(k, upd) {
            m.insert(k.to_string(), v);
        }
    }
    let shared = if upd_py && upd_rs {
        nw.get("shared")
    } else {
        ex.get("shared").or_else(|| nw.get("shared"))
    }
    .cloned();
    if let Some(v) = shared {
        m.insert("shared".to_string(), v);
    }
    if !(upd_py && upd_rs)
        && let Some(v) = ex.get("thresholds").cloned()
    {
        m.insert("thresholds".to_string(), v);
    }
    build_merged_output(&m)
}

fn build_merged_output(m: &toml::Table) -> String {
    let mut out = String::from(
        "# Generated by kiss mimic\n# Thresholds based on max values of analyzed codebase\n\n",
    );
    for k in ["gate", "python", "rust", "shared", "thresholds"] {
        format_section(&mut out, k, m.get(k));
    }
    out
}

pub fn generate_config_toml_by_language(
    py: &MetricStats,
    rs: &MetricStats,
    py_n: usize,
    rs_n: usize,
    gate: &GateConfig,
) -> String {
    use std::fmt::Write;
    let mut out = String::from(
        "# Generated by kiss mimic\n# Thresholds based on max values of analyzed codebase\n\n",
    );
    let _ = writeln!(out, "[gate]");
    let _ = writeln!(
        out,
        "test_coverage_threshold = {}",
        gate.test_coverage_threshold
    );
    let _ = writeln!(out, "min_similarity = {}", gate.min_similarity);
    let _ = writeln!(out, "duplication_enabled = {}\n", gate.duplication_enabled);
    if py_n > 0 {
        append_section(
            &mut out,
            "[python]",
            &compute_summaries(py),
            python_config_key,
        );
    } else {
        append_python_defaults(&mut out);
    }
    if rs_n > 0 {
        append_section(&mut out, "[rust]", &compute_summaries(rs), rust_config_key);
    } else {
        append_rust_defaults(&mut out);
    }
    out
}

pub fn infer_gate_config_for_paths(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> GateConfig {
    let (py_files, rs_files) = gather_files_by_lang(paths, lang, ignore);
    let mut gate = GateConfig::default();

    let py_parsed = if py_files.is_empty() {
        Vec::new()
    } else {
        parse_files(&py_files)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .collect::<Vec<ParsedFile>>()
    };
    let rs_parsed = if rs_files.is_empty() {
        Vec::new()
    } else {
        parse_rust_files(&rs_files)
            .into_iter()
            .filter_map(Result::ok)
            .collect::<Vec<ParsedRustFile>>()
    };

    gate.test_coverage_threshold = compute_static_test_coverage(&py_parsed, &rs_parsed);
    gate.duplication_enabled =
        !has_reportable_duplicates(&py_parsed, &rs_parsed, gate.min_similarity);
    gate
}

fn gather_files_by_lang(
    paths: &[String],
    lang: Option<Language>,
    ignore: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut py_files, mut rs_files) = (Vec::new(), Vec::new());
    for path in paths {
        for sf in find_source_files_with_ignore(Path::new(path), ignore) {
            match (sf.language, lang) {
                (Language::Python, None | Some(Language::Python)) => py_files.push(sf.path),
                (Language::Rust, None | Some(Language::Rust)) => rs_files.push(sf.path),
                _ => {}
            }
        }
    }
    (py_files, rs_files)
}

fn compute_static_test_coverage(py_parsed: &[ParsedFile], rs_parsed: &[ParsedRustFile]) -> usize {
    let (mut tested, mut total) = (0usize, 0usize);
    if !py_parsed.is_empty() {
        let refs: Vec<&ParsedFile> = py_parsed.iter().collect();
        let a = analyze_test_refs(&refs);
        total += a.definitions.len();
        tested += a.definitions.len().saturating_sub(a.unreferenced.len());
    }
    if !rs_parsed.is_empty() {
        let refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
        let a = analyze_rust_test_refs(&refs);
        total += a.definitions.len();
        tested += a.definitions.len().saturating_sub(a.unreferenced.len());
    }
    if total == 0 {
        return 100;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    (((tested as f64 / total as f64) * 100.0).round() as usize).min(100)
}

fn has_reportable_duplicates(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    min_similarity: f64,
) -> bool {
    let config = DuplicationConfig {
        min_similarity,
        ..Default::default()
    };
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let mut chunks = extract_chunks_for_duplication(&py_refs);
    chunks.extend(extract_rust_chunks_for_duplication(&rs_refs));
    if chunks.len() < 2 {
        return false;
    }
    let pairs = detect_duplicates_from_chunks(&chunks, &config);
    !cluster_duplicates(&pairs, &chunks).is_empty()
}

fn append_python_defaults(out: &mut String) {
    use crate::defaults::{graph, python};
    use std::fmt::Write;
    let _ = writeln!(out, "[python]");
    let _ = writeln!(
        out,
        "statements_per_function = {}",
        python::STATEMENTS_PER_FUNCTION
    );
    let _ = writeln!(out, "positional_args = {}", python::POSITIONAL_ARGS);
    let _ = writeln!(out, "keyword_only_args = {}", python::KEYWORD_ONLY_ARGS);
    let _ = writeln!(out, "max_indentation = {}", python::MAX_INDENTATION);
    let _ = writeln!(
        out,
        "branches_per_function = {}",
        python::BRANCHES_PER_FUNCTION
    );
    let _ = writeln!(out, "local_variables = {}", python::LOCAL_VARIABLES);
    let _ = writeln!(out, "methods_per_class = {}", python::METHODS_PER_CLASS);
    let _ = writeln!(
        out,
        "nested_function_depth = {}",
        python::NESTED_FUNCTION_DEPTH
    );
    let _ = writeln!(
        out,
        "returns_per_function = {}",
        python::RETURNS_PER_FUNCTION
    );
    let _ = writeln!(out, "statements_per_file = {}", python::STATEMENTS_PER_FILE);
    let _ = writeln!(out, "functions_per_file = {}", python::FUNCTIONS_PER_FILE);
    let _ = writeln!(
        out,
        "interface_types_per_file = {}",
        python::INTERFACE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "concrete_types_per_file = {}",
        python::CONCRETE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "imported_names_per_file = {}",
        python::IMPORTS_PER_FILE
    );
    let _ = writeln!(
        out,
        "transitive_dependencies = {}",
        python::TRANSITIVE_DEPENDENCIES
    );
    let _ = writeln!(out, "dependency_depth = {}", python::DEPENDENCY_DEPTH);
    let _ = writeln!(
        out,
        "statements_per_try_block = {}",
        python::STATEMENTS_PER_TRY_BLOCK
    );
    let _ = writeln!(out, "boolean_parameters = {}", python::BOOLEAN_PARAMETERS);
    let _ = writeln!(
        out,
        "decorators_per_function = {}",
        python::DECORATORS_PER_FUNCTION
    );
    let _ = writeln!(out, "calls_per_function = {}", python::CALLS_PER_FUNCTION);
    let _ = writeln!(out, "cycle_size = {}\n", graph::CYCLE_SIZE);
}

fn append_rust_defaults(out: &mut String) {
    use crate::defaults::{graph, rust};
    use std::fmt::Write;
    let _ = writeln!(out, "[rust]");
    let _ = writeln!(
        out,
        "statements_per_function = {}",
        rust::STATEMENTS_PER_FUNCTION
    );
    let _ = writeln!(out, "arguments = {}", rust::ARGUMENTS);
    let _ = writeln!(out, "max_indentation = {}", rust::MAX_INDENTATION);
    let _ = writeln!(
        out,
        "branches_per_function = {}",
        rust::BRANCHES_PER_FUNCTION
    );
    let _ = writeln!(out, "local_variables = {}", rust::LOCAL_VARIABLES);
    let _ = writeln!(out, "methods_per_class = {}", rust::METHODS_PER_TYPE);
    let _ = writeln!(
        out,
        "nested_function_depth = {}",
        rust::NESTED_FUNCTION_DEPTH
    );
    let _ = writeln!(out, "returns_per_function = {}", rust::RETURNS_PER_FUNCTION);
    let _ = writeln!(out, "statements_per_file = {}", rust::STATEMENTS_PER_FILE);
    let _ = writeln!(out, "functions_per_file = {}", rust::FUNCTIONS_PER_FILE);
    let _ = writeln!(
        out,
        "interface_types_per_file = {}",
        rust::INTERFACE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "concrete_types_per_file = {}",
        rust::CONCRETE_TYPES_PER_FILE
    );
    let _ = writeln!(out, "imported_names_per_file = {}", rust::IMPORTS_PER_FILE);
    let _ = writeln!(
        out,
        "transitive_dependencies = {}",
        rust::TRANSITIVE_DEPENDENCIES
    );
    let _ = writeln!(out, "dependency_depth = {}", rust::DEPENDENCY_DEPTH);
    let _ = writeln!(out, "boolean_parameters = {}", rust::BOOLEAN_PARAMETERS);
    let _ = writeln!(
        out,
        "attributes_per_function = {}",
        rust::ATTRIBUTES_PER_FUNCTION
    );
    let _ = writeln!(out, "calls_per_function = {}", rust::CALLS_PER_FUNCTION);
    let _ = writeln!(out, "cycle_size = {}\n", graph::CYCLE_SIZE);
}

fn append_section(
    out: &mut String,
    header: &str,
    sums: &[PercentileSummary],
    key_fn: fn(&str) -> Option<&'static str>,
) {
    use std::fmt::Write;
    out.push_str(header);
    out.push('\n');
    for s in sums {
        if let Some(k) = key_fn(s.metric_id) {
            let _ = writeln!(out, "{k} = {}", s.max);
        }
    }
    out.push('\n');
}

/// Map `metric_id` to config key (common keys shared by Python and Rust)
fn common_config_key(metric_id: &str) -> Option<&'static str> {
    match metric_id {
        "statements_per_function" => Some("statements_per_function"),
        "max_indentation_depth" => Some("max_indentation"),
        "branches_per_function" => Some("branches_per_function"),
        "local_variables_per_function" => Some("local_variables"),
        "cycle_size" => Some("cycle_size"),
        "methods_per_type" => Some("methods_per_class"),
        "nested_function_depth" => Some("nested_function_depth"),
        "returns_per_function" => Some("returns_per_function"),
        "calls_per_function" => Some("calls_per_function"),
        "statements_per_file" => Some("statements_per_file"),
        "functions_per_file" => Some("functions_per_file"),
        "interface_types_per_file" => Some("interface_types_per_file"),
        "concrete_types_per_file" => Some("concrete_types_per_file"),
        "imported_names_per_file" => Some("imported_names_per_file"),
        "transitive_deps" => Some("transitive_dependencies"),
        "dependency_depth" => Some("dependency_depth"),
        _ => None,
    }
}

pub fn python_config_key(metric_id: &str) -> Option<&'static str> {
    match metric_id {
        "args_positional" => Some("positional_args"),
        "args_keyword_only" => Some("keyword_only_args"),
        "return_values_per_return" => Some("return_values_per_function"),
        "statements_per_try_block" => Some("statements_per_try_block"),
        "boolean_parameters" => Some("boolean_parameters"),
        "annotations_per_function" => Some("decorators_per_function"),
        _ => common_config_key(metric_id),
    }
}

pub fn rust_config_key(metric_id: &str) -> Option<&'static str> {
    match metric_id {
        "args_total" => Some("arguments"),
        "boolean_parameters" => Some("boolean_parameters"),
        "annotations_per_function" => Some("attributes_per_function"),
        _ => common_config_key(metric_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_collection() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(collect_py_stats(tmp.path()).1, 0);
        assert_eq!(collect_rs_stats(tmp.path()).1, 0);
        let paths = vec![tmp.path().to_string_lossy().to_string()];
        assert_eq!(collect_all_stats(&paths, None).0.1, 0);
        assert_eq!(
            collect_py_stats_with_ignore(tmp.path(), &["fake_".into()]).1,
            0
        );
        assert_eq!(
            collect_rs_stats_with_ignore(tmp.path(), &["fake_".into()]).1,
            0
        );
        assert_eq!(
            collect_all_stats_with_ignore(&paths, None, &["fake_".into()])
                .0
                .1,
            0
        );
    }

    #[test]
    fn test_config_keys() {
        assert_eq!(
            python_config_key("statements_per_function"),
            Some("statements_per_function")
        );
        assert_eq!(
            rust_config_key("statements_per_function"),
            Some("statements_per_function")
        );
        assert_eq!(
            python_config_key("statements_per_file"),
            Some("statements_per_file")
        );
        assert_eq!(
            common_config_key("branches_per_function"),
            Some("branches_per_function")
        );
    }

    #[test]
    fn test_config_generation() {
        let gate = GateConfig::default();
        assert!(
            generate_config_toml_by_language(
                &MetricStats::default(),
                &MetricStats::default(),
                0,
                0,
                &gate
            )
            .contains("Generated by kiss")
        );
        assert!(build_merged_output(&toml::Table::new()).contains("Generated by kiss"));
        let mut out = String::new();
        let mut table = toml::Table::new();
        table.insert("key".into(), toml::Value::Integer(42));
        format_section(&mut out, "test", Some(&toml::Value::Table(table)));
        assert!(out.contains("[test]") && out.contains("key = 42"));
        let mut out2 = String::new();
        append_section(
            &mut out2,
            "[python]",
            &[PercentileSummary {
                metric_id: "statements_per_function",
                display_name: "Statements per function",
                count: 10,
                max: 50,
                p50: 5,
                p90: 10,
                p95: 15,
                p99: 20,
            }],
            python_config_key,
        );
        assert!(out2.contains("statements_per_function = 50"));
    }

    #[test]
    fn test_defaults_appenders() {
        let mut py_full = String::new();
        append_python_defaults(&mut py_full);
        assert!(py_full.contains("[python]") && py_full.contains("statements_per_function"));
        let mut rs_full = String::new();
        append_rust_defaults(&mut rs_full);
        assert!(rs_full.contains("[rust]") && rs_full.contains("arguments"));
    }
}
