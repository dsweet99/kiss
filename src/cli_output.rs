use crate::discovery::Language;
use crate::duplication::{DuplicateCluster, DuplicatePair};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::test_refs::analyze_test_refs;
use crate::violation::Violation;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const VIOLATIONS_FIX_HINT: &str =
    "Run 'kiss rules' for more information about fixing violations.";

/// Format a candidate list for display, truncating to `max` items with ellipsis.
pub fn format_candidate_list(candidates: &[String], max: usize) -> String {
    if candidates.len() > max {
        format!("{}…", candidates[..max].join(", "))
    } else {
        candidates.join(", ")
    }
}

/// Returns the minimum per-file coverage percentage, or 100 if no files have definitions.
pub fn min_per_file_coverage(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> usize {
    let map = file_coverage_map(definitions, unreferenced);
    map.values().min().copied().unwrap_or(100)
}

pub fn file_coverage_map(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> HashMap<PathBuf, usize> {
    file_coverage_map_from_paths(
        definitions.iter().map(|(f, _, _)| f),
        unreferenced.iter().map(|(f, _, _)| f),
    )
}

/// Per-file coverage weighted by source line spans (for [`kiss-coverage-map`] calibration).
/// Each physical line counts at most once (avoids stacking many small defs on shared lines).
pub fn file_coverage_map_by_line_spans(
    definitions: &[(PathBuf, String, usize, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> HashMap<PathBuf, usize> {
    file_coverage_map_by_line_spans_with_credit_end(
        &definitions
            .iter()
            .map(|(f, n, s, e)| (f.clone(), n.clone(), *s, *e, *e))
            .collect::<Vec<_>>(),
        unreferenced,
    )
}

/// Like [`file_coverage_map_by_line_spans`], but `credit_end` may be less than `total_end` so
/// inflator paths can count full def bodies in the denominator while crediting header lines only.
pub fn file_coverage_map_by_line_spans_with_credit_end(
    definitions: &[(PathBuf, String, usize, usize, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> HashMap<PathBuf, usize> {
    let unref_keys: HashSet<(&PathBuf, &str, usize)> = unreferenced
        .iter()
        .map(|(f, n, l)| (f, n.as_str(), *l))
        .collect();
    let mut total_lines: HashMap<PathBuf, HashSet<usize>> = HashMap::new();
    let mut covered_lines: HashMap<PathBuf, HashSet<usize>> = HashMap::new();
    for (file, name, start, total_end, credit_end) in definitions {
        let covered = !unref_keys.contains(&(file, name.as_str(), *start));
        let credit_end = (*credit_end).min(*total_end);
        for line in *start..=*total_end {
            total_lines.entry(file.clone()).or_default().insert(line);
            if covered && line <= credit_end {
                covered_lines.entry(file.clone()).or_default().insert(line);
            }
        }
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    total_lines
        .into_iter()
        .map(|(file, total_set)| {
            let total = total_set.len();
            let covered = covered_lines
                .get(&file)
                .map(|s| s.intersection(&total_set).count())
                .unwrap_or(0);
            let pct = if total == 0 {
                100
            } else {
                ((covered as f64 / total as f64) * 100.0).round() as usize
            };
            (file, pct)
        })
        .collect()
}

pub fn file_coverage_map_from_paths<'a>(
    definitions: impl IntoIterator<Item = &'a PathBuf>,
    unreferenced: impl IntoIterator<Item = &'a PathBuf>,
) -> HashMap<PathBuf, usize> {
    let mut defs_per_file: HashMap<PathBuf, usize> = HashMap::new();
    let mut unref_per_file: HashMap<PathBuf, usize> = HashMap::new();
    for file in definitions {
        *defs_per_file.entry(file.clone()).or_default() += 1;
    }
    for file in unreferenced {
        *unref_per_file.entry(file.clone()).or_default() += 1;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    defs_per_file
        .into_iter()
        .map(|(file, total)| {
            let unref = unref_per_file.get(&file).copied().unwrap_or(0);
            let tested = total.saturating_sub(unref);
            let pct = ((tested as f64 / total as f64) * 100.0).round() as usize;
            (file, pct)
        })
        .collect()
}

pub fn print_dry_results(pairs: &[DuplicatePair]) {
    for p in pairs {
        println!(
            "{:.3}  {}:{}-{}  {}:{}-{}",
            p.similarity,
            p.chunk1.file.display(),
            p.chunk1.start_line,
            p.chunk1.end_line,
            p.chunk2.file.display(),
            p.chunk2.start_line,
            p.chunk2.end_line
        );
    }
}

pub fn print_no_files_message(lang_filter: Option<Language>, root: &Path) {
    let msg = match lang_filter {
        Some(Language::Python) => "No Python files",
        Some(Language::Rust) => "No Rust files",
        None => "No files",
    };
    println!("{} in {}", msg, root.display());
}

pub struct CoverageGateFailureCtx<'a> {
    pub threshold: usize,
    pub unreferenced: &'a [(std::path::PathBuf, String, usize)],
    pub file_pcts: &'a HashMap<std::path::PathBuf, usize>,
}

#[allow(clippy::implicit_hasher)]
pub fn print_coverage_gate_failure(ctx: &CoverageGateFailureCtx<'_>) {
    // Per-file enforcement: list failing files first, then unreferenced units
    let threshold = ctx.threshold;
    let mut failing: Vec<_> = ctx
        .file_pcts
        .iter()
        .filter(|(_, pct)| **pct < threshold)
        .map(|(f, p)| (f.clone(), *p))
        .collect();
    failing.sort_by(|a, b| a.0.cmp(&b.0));
    println!(
        "GATE_FAILED:test_coverage: {n} file(s) below {threshold}% threshold (per-file enforcement)",
        n = failing.len()
    );
    for (file, pct) in &failing {
        println!("  {}: {pct}% ({threshold}% required)", file.display());
    }
    for (file, name, line) in ctx.unreferenced {
        let pct = ctx.file_pcts.get(file).copied().unwrap_or(0);
        if pct < threshold {
            println!(
                "VIOLATION:test_coverage:{}:{}:{}: {pct}% covered. Add test coverage for this code unit.",
                file.display(),
                line,
                name
            );
        }
    }
    print_final_status(true);
}

pub fn print_violations(viols: &[Violation]) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    for v in viols {
        let _ = writeln!(
            w,
            "VIOLATION:{}:{}:{}:{}: {} {}",
            v.metric,
            v.file.display(),
            v.line,
            v.unit_name,
            v.message,
            v.suggestion
        );
    }
}

pub fn print_final_status(has_violations: bool) {
    if has_violations {
        println!("{VIOLATIONS_FIX_HINT}");
    } else {
        println!("NO VIOLATIONS");
    }
}

pub fn print_duplicates(lang: &str, clusters: &[DuplicateCluster]) {
    use std::io::Write;
    let suggestion = if lang == "Rust" {
        "Extract into a shared function, or use traits/generics."
    } else {
        "Extract common code into a shared function."
    };
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    for c in clusters {
        if let Some(first) = c.chunks.first() {
            let locations: Vec<String> = c
                .chunks
                .iter()
                .map(|ch| format!("{}:{}-{}", ch.file.display(), ch.start_line, ch.end_line))
                .collect();
            let _ = writeln!(
                w,
                "VIOLATION:duplication:{}:{}:{}: {:.0}% similar, {} copies: [{}]. {}",
                first.file.display(),
                first.start_line,
                first.name,
                c.avg_similarity * 100.0,
                c.chunks.len(),
                locations.join(", "),
                suggestion
            );
        }
    }
}

pub fn count_py_unreferenced(parsed: &[ParsedFile]) -> usize {
    if parsed.is_empty() {
        return 0;
    }
    let analysis = analyze_test_refs(&parsed.iter().collect::<Vec<_>>(), None);
    analysis.unreferenced.len()
}

pub fn count_rs_unreferenced(parsed: &[ParsedRustFile]) -> usize {
    if parsed.is_empty() {
        return 0;
    }
    let analysis = analyze_rust_test_refs(&parsed.iter().collect::<Vec<_>>(), None);
    analysis.unreferenced.len()
}

#[cfg(test)]
#[path = "cli_output_tests.rs"]
mod cli_output_tests;
