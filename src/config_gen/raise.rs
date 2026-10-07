use crate::graph::GraphKeyMaxima;
use crate::stats::{MetricStats, compute_summaries};

use super::collect::CollectedLang;
use super::config_keys::{python_config_key, rust_config_key};

const GRAPH_METRIC_IDS: &[&str] = &["cycle_size", "indirect_dependencies", "dependency_depth"];

pub fn raise_measured_thresholds(text: &str, py: &CollectedLang, rs: &CollectedLang) -> String {
    let mut out = text.to_string();
    if py.file_count > 0 {
        out = raise_language(&out, "python", &py.stats, py.graph_max, python_config_key);
    }
    if rs.file_count > 0 {
        out = raise_language(&out, "rust", &rs.stats, rs.graph_max, rust_config_key);
    }
    out
}

fn raise_language(
    text: &str,
    section: &str,
    stats: &MetricStats,
    graph_max: GraphKeyMaxima,
    key_fn: fn(&str) -> Option<&'static str>,
) -> String {
    let mut out = text.to_string();
    for summary in compute_summaries(stats) {
        if GRAPH_METRIC_IDS.contains(&summary.metric_id) {
            continue;
        }
        let Some(key) = key_fn(summary.metric_id) else {
            continue;
        };
        out = raise_key(&out, section, key, summary.max);
    }
    out = raise_key(
        &out,
        section,
        "indirect_dependencies",
        graph_max.indirect_dependencies,
    );
    out = raise_key(
        &out,
        section,
        "dependency_depth",
        graph_max.dependency_depth,
    );
    raise_key(&out, section, "cycle_size", graph_max.cycle_size)
}

fn raise_key(text: &str, section: &str, key: &str, measured: usize) -> String {
    let Some(current) = section_usize(text, section, key) else {
        return text.to_string();
    };
    if measured <= current {
        return text.to_string();
    }
    replace_assignment(text, section, key, measured)
}

fn section_usize(text: &str, section: &str, key: &str) -> Option<usize> {
    let table: toml::Table = text.parse().ok()?;
    let value = table.get(section)?.as_table()?.get(key)?;
    match value {
        toml::Value::Integer(n) if *n >= 0 => usize::try_from(*n).ok(),
        _ => None,
    }
}

fn replace_assignment(text: &str, section: &str, key: &str, value: usize) -> String {
    let header = format!("[{section}]");
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == header;
            lines.push(line.to_string());
            continue;
        }
        if in_section && assignment_key(trimmed) == Some(key) {
            let indent_len = line.len() - line.trim_start().len();
            let indent = &line[..indent_len];
            lines.push(format!("{indent}{key} = {value}"));
            continue;
        }
        lines.push(line.to_string());
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn assignment_key(trimmed: &str) -> Option<&str> {
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang_with_positional(max_args: usize, files: usize) -> CollectedLang {
        let mut stats = MetricStats::default();
        stats.arguments_positional.push(max_args);
        CollectedLang {
            stats,
            file_count: files,
            graph_max: GraphKeyMaxima::default(),
        }
    }

    #[test]
    fn raise_increases_only_an_exceeded_python_threshold() {
        let text = "\
[global]
duplication_enabled = true

[python]
positional_args = 3
lines_per_file = 250

[rust]
arguments = 4
";
        let raised = raise_measured_thresholds(
            text,
            &lang_with_positional(6, 1),
            &lang_with_positional(9, 0),
        );
        assert!(raised.contains("positional_args = 6"), "{raised}");
        assert!(raised.contains("lines_per_file = 250"), "{raised}");
        assert!(raised.contains("arguments = 4"), "{raised}");
        assert!(raised.contains("duplication_enabled = true"), "{raised}");
    }

    #[test]
    fn raise_leaves_text_when_code_fits() {
        let text = "[python]\npositional_args = 3\n";
        let raised = raise_measured_thresholds(
            text,
            &lang_with_positional(2, 1),
            &lang_with_positional(0, 0),
        );
        assert_eq!(raised, text);
    }
}
