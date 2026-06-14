use crate::discovery::Language;
use crate::duplication::{DuplicateCluster, DuplicatePair};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::test_refs::analyze_test_refs;
use crate::violation::Violation;
use std::collections::HashMap;
use std::io::Write;
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
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    let _ = write_coverage_gate_failure(&mut w, ctx);
}

#[allow(clippy::implicit_hasher)]
pub fn write_coverage_gate_failure(
    w: &mut impl Write,
    ctx: &CoverageGateFailureCtx<'_>,
) -> std::io::Result<()> {
    // Per-file enforcement: list failing files first, then unreferenced units
    let threshold = ctx.threshold;
    let mut failing: Vec<_> = ctx
        .file_pcts
        .iter()
        .filter(|(_, pct)| **pct < threshold)
        .map(|(f, p)| (f.clone(), *p))
        .collect();
    failing.sort_by(|a, b| a.0.cmp(&b.0));
    writeln!(
        w,
        "GATE_FAILED:test_coverage: {n} file(s) below {threshold}% threshold (per-file enforcement)",
        n = failing.len()
    )?;
    for (file, pct) in &failing {
        writeln!(w, "  {}: {pct}% ({threshold}% required)", file.display())?;
    }
    for (file, name, line) in ctx.unreferenced {
        let pct = ctx.file_pcts.get(file).copied().unwrap_or(0);
        if pct < threshold {
            writeln!(
                w,
                "VIOLATION:test_coverage:{}:{}:{}: {pct}% covered. Add test coverage for this code unit.",
                file.display(),
                line,
                name
            )?;
        }
    }
    write_final_status(w, true)
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
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    let _ = write_final_status(&mut w, has_violations);
}

pub fn write_final_status(w: &mut impl Write, has_violations: bool) -> std::io::Result<()> {
    if has_violations {
        writeln!(w, "{VIOLATIONS_FIX_HINT}")
    } else {
        writeln!(w, "NO VIOLATIONS")
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
        let mut out = Vec::new();
        write_coverage_gate_failure(
            &mut out,
            &CoverageGateFailureCtx {
                threshold: 80,
                unreferenced: &[(std::path::PathBuf::from("foo.py"), "bar".to_string(), 10)],
                file_pcts: &file_pcts,
            },
        )
        .unwrap();
        let stdout = String::from_utf8(out).unwrap();
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
        let mut clean = Vec::new();
        write_final_status(&mut clean, false).unwrap();
        let clean = String::from_utf8(clean).unwrap();
        assert_eq!(clean.trim(), "NO VIOLATIONS");
        let mut viol = Vec::new();
        write_final_status(&mut viol, true).unwrap();
        let viol = String::from_utf8(viol).unwrap();
        assert!(
            viol.contains(VIOLATIONS_FIX_HINT),
            "expected hint in stdout: {viol}"
        );
    }

    #[test]
    fn test_print_duplicates_empty() {
        print_duplicates("Test", &[]);
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
}
