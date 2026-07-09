use crate::discovery::Language;
use crate::duplication::{DuplicateCluster, DuplicatePair};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::test_refs::analyze_test_refs;
use crate::violation::Violation;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const VIOLATIONS_FIX_HINT: &str =
    "Run 'kiss rules' for more information about fixing violations.";

/// Coverage message for a definition-level unreferenced code unit.
///
/// File-level percentages can read 100% under weighted overlays or integer rounding
/// while individual helpers remain unreferenced; never claim full coverage on the unit line.
pub fn format_unreferenced_unit_coverage_message(file_pct: usize) -> String {
    if file_pct >= 100 {
        "This code unit has no test reference. Add test coverage.".to_string()
    } else {
        format!("{file_pct}% covered. Add test coverage for this code unit.")
    }
}

/// Format a candidate list for display, truncating to `max` items with ellipsis.
pub fn format_candidate_list(candidates: &[String], max: usize) -> String {
    if candidates.len() > max {
        format!("{}…", candidates[..max].join(", "))
    } else {
        candidates.join(", ")
    }
}

/// Whether a path participates in the per-file test-coverage gate.
pub fn is_coverage_gate_file(path: &Path) -> bool {
    !crate::test_refs::is_test_file(path)
        && !crate::test_refs::is_in_test_directory(path)
        && !crate::rust_test_refs::is_rust_test_file(path)
        && !crate::rust_test_refs::is_binary_entry_point(path)
}

/// Returns the minimum per-file coverage percentage, or 100 if no files have definitions.
pub fn min_per_file_coverage(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> usize {
    let map = file_coverage_map(definitions, unreferenced);
    map.values().min().copied().unwrap_or(100)
}

/// Minimum per-file coverage among files subject to the test-coverage gate.
pub fn min_gate_eligible_per_file_coverage(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> usize {
    let map = file_coverage_map(definitions, unreferenced);
    map.iter()
        .filter(|(path, _)| is_coverage_gate_file(path))
        .map(|(_, pct)| *pct)
        .min()
        .unwrap_or(100)
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
    for line in coverage_gate_failure_lines(ctx) {
        println!("{line}");
    }
}

#[allow(clippy::implicit_hasher)]
pub fn coverage_gate_failure_lines(ctx: &CoverageGateFailureCtx<'_>) -> Vec<String> {
    // Per-file enforcement: list failing files first, then unreferenced units
    let threshold = ctx.threshold;
    let mut failing: Vec<_> = ctx
        .file_pcts
        .iter()
        .filter(|(_, pct)| **pct < threshold)
        .map(|(f, p)| (f.clone(), *p))
        .collect();
    failing.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = vec![format!(
        "GATE_FAILED:test_coverage: {n} file(s) below {threshold}% threshold (per-file enforcement)",
        n = failing.len()
    )];
    for (file, pct) in &failing {
        lines.push(format!(
            "  {}: {pct}% ({threshold}% required)",
            file.display()
        ));
    }
    for (file, name, line) in ctx.unreferenced {
        let pct = ctx.file_pcts.get(file).copied().unwrap_or(0);
        if pct < threshold {
            let message = format_unreferenced_unit_coverage_message(pct);
            lines.push(format!(
                "VIOLATION:test_coverage:{}:{}:{}: {message}",
                file.display(),
                line,
                name
            ));
        }
    }
    lines.push(final_status_message(true).to_string());
    lines
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
    println!("{}", final_status_message(has_violations));
}

pub fn final_status_message(has_violations: bool) -> &'static str {
    if has_violations {
        VIOLATIONS_FIX_HINT
    } else {
        "NO VIOLATIONS"
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_violations_fix_hint_matches_plan_text() {
        assert_eq!(
            VIOLATIONS_FIX_HINT,
            "Run 'kiss rules' for more information about fixing violations."
        );
    }

    #[test]
    fn test_print_no_files_message_no_panic() {
        let tmp = TempDir::new().unwrap();
        print_no_files_message(None, tmp.path());
        print_no_files_message(Some(Language::Python), tmp.path());
    }

    #[test]
    fn test_print_coverage_gate_failure_emits_hint() {
        let file_pcts: HashMap<std::path::PathBuf, usize> =
            [(std::path::PathBuf::from("foo.py"), 50)].into();
        let lines = coverage_gate_failure_lines(&CoverageGateFailureCtx {
            threshold: 80,
            unreferenced: &[(std::path::PathBuf::from("foo.py"), "bar".to_string(), 10)],
            file_pcts: &file_pcts,
        });
        let stdout = lines.join("\n");
        assert!(
            stdout.contains(VIOLATIONS_FIX_HINT),
            "expected hint in stdout: {stdout}"
        );
        assert!(
            stdout.contains("GATE_FAILED:test_coverage:"),
            "expected gate failure in stdout: {stdout}"
        );
    }

    #[test]
    fn test_print_violations_empty() {
        print_violations(&[]);
        assert_eq!(final_status_message(false), "NO VIOLATIONS");
        assert_eq!(final_status_message(true), VIOLATIONS_FIX_HINT);
    }

    #[test]
    fn test_print_duplicates_empty() {
        print_duplicates("Test", &[]);
    }

    #[test]
    fn test_format_unreferenced_unit_coverage_message_rounding_cliff() {
        assert_eq!(
            format_unreferenced_unit_coverage_message(100),
            "This code unit has no test reference. Add test coverage."
        );
        assert_eq!(
            format_unreferenced_unit_coverage_message(50),
            "50% covered. Add test coverage for this code unit."
        );
    }

    #[test]
    fn test_file_coverage_map_computes_per_file_pct() {
        let defs = vec![
            (PathBuf::from("a.py"), "f1".into(), 1),
            (PathBuf::from("a.py"), "f2".into(), 5),
            (PathBuf::from("b.py"), "g1".into(), 1),
        ];
        let unref = vec![(PathBuf::from("a.py"), "f2".into(), 5)];
        let map = file_coverage_map(&defs, &unref);
        assert_eq!(map[&PathBuf::from("a.py")], 50);
        assert_eq!(map[&PathBuf::from("b.py")], 100);
    }

    #[test]
    fn test_count_py_unreferenced_empty() {
        assert_eq!(count_py_unreferenced(&[]), 0);
    }

    #[test]
    fn test_count_rs_unreferenced_empty() {
        assert_eq!(count_rs_unreferenced(&[]), 0);
    }

    impl<'a> CoverageGateFailureCtx<'a> {
        fn witness(
            threshold: usize,
            unreferenced: &'a [(PathBuf, String, usize)],
            file_pcts: &'a HashMap<PathBuf, usize>,
        ) -> Self {
            Self {
                threshold,
                unreferenced,
                file_pcts,
            }
        }
    }

    #[test]
    fn witness_coverage_gate_helpers() {
        use std::path::Path;
        let defs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let unref: Vec<(PathBuf, String, usize)> = vec![];
        let file_pcts = HashMap::new();
        assert!(is_coverage_gate_file(Path::new("src/a.py")));
        assert_eq!(min_per_file_coverage(&defs, &unref), 100);
        assert_eq!(min_gate_eligible_per_file_coverage(&defs, &unref), 100);
        let _ = CoverageGateFailureCtx::witness(90, &unref, &file_pcts);
    }
}
