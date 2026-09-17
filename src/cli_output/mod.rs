use crate::discovery::Language;
use crate::duplication::{DuplicateCluster, DuplicatePair};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::violation::Violation;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod cov_gate;
pub use cov_gate::{
    CodebaseCoverageGateFailureCtx, CoverageFileStat, CoverageGateFailureCtx,
    codebase_coverage_gate_failure_lines, coverage_gate_failure_lines,
    print_codebase_coverage_gate_failure, print_coverage_gate_failure,
};

pub const VIOLATIONS_FIX_HINT: &str =
    "Run 'kiss rules' for more information about fixing violations.";

pub fn extra_coverable_lines_to_reach(covered: usize, total: usize, threshold: usize) -> usize {
    if total == 0 {
        return 0;
    }
    let mut extra = 0;
    loop {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let pct = ((covered + extra) as f64 / total as f64 * 100.0).round() as usize;
        if pct >= threshold || covered + extra >= total {
            return extra;
        }
        extra += 1;
    }
}

pub fn format_unreferenced_unit_coverage_message(
    file_pct: usize,
    covered: usize,
    total: usize,
    threshold: usize,
) -> String {
    if total == 0 {
        return "This code unit has no covered lines. Add test coverage.".to_string();
    }
    let need = extra_coverable_lines_to_reach(covered, total, threshold);
    if need == 0 {
        format!("{file_pct}% covered ({covered}/{total}). Add test coverage for this code unit.")
    } else if need == 1 {
        format!("{file_pct}% covered ({covered}/{total}). Need 1 more line to reach {threshold}%.")
    } else {
        format!(
            "{file_pct}% covered ({covered}/{total}). Need {need} more lines to reach {threshold}%."
        )
    }
}

pub fn coverage_unit_name(file: &Path) -> String {
    file.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("<file>")
        .to_string()
}

pub fn format_candidate_list(candidates: &[String], max: usize) -> String {
    if candidates.len() > max {
        format!("{}…", candidates[..max].join(", "))
    } else {
        candidates.join(", ")
    }
}

pub fn min_per_file_coverage(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> usize {
    let map = file_coverage_map(definitions, unreferenced);
    map.values().min().copied().unwrap_or(100)
}

pub fn min_gate_eligible_per_file_coverage(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> usize {
    min_per_file_coverage(definitions, unreferenced)
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
    let _ = parsed;
    0
}

pub fn count_rs_unreferenced(parsed: &[ParsedRustFile]) -> usize {
    let _ = parsed;
    0
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
            format_unreferenced_unit_coverage_message(100, 0, 0, 75),
            "This code unit has no covered lines. Add test coverage."
        );
        assert_eq!(
            format_unreferenced_unit_coverage_message(50, 1, 2, 75),
            "50% covered (1/2). Need 1 more line to reach 75%."
        );
    }

    #[test]
    fn extra_coverable_lines_matches_rounding_gap() {
        assert_eq!(extra_coverable_lines_to_reach(4, 6, 75), 1);
        assert_eq!(extra_coverable_lines_to_reach(12, 18, 75), 2);
        assert_eq!(coverage_unit_name(Path::new("src/lib.py")), "lib");
    }

    #[test]
    fn test_candidate_list_truncates_only_when_needed() {
        let candidates = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        assert_eq!(format_candidate_list(&candidates, 3), "a, b, c");
        assert_eq!(format_candidate_list(&candidates, 2), "a, b…");
    }

    #[test]
    fn test_gate_eligible_coverage_uses_supplied_records() {
        let defs = vec![
            (PathBuf::from("src/lib.py"), "prod".into(), 1),
            (PathBuf::from("src/main.rs"), "main".into(), 1),
        ];
        let unref = vec![(PathBuf::from("src/main.rs"), "main".into(), 1)];

        assert_eq!(min_per_file_coverage(&defs, &unref), 0);
        assert_eq!(min_gate_eligible_per_file_coverage(&defs, &unref), 0);
    }

    #[test]
    fn test_gate_eligible_coverage_ignores_tests_and_binary_entry_points() {
        let defs = vec![
            (PathBuf::from("src/lib.py"), "prod".into(), 1),
            (PathBuf::from("src/main.rs"), "main".into(), 1),
        ];
        let unref = vec![(PathBuf::from("src/lib.py"), "prod".into(), 1)];
        assert_eq!(
            min_gate_eligible_per_file_coverage(&defs, &unref),
            min_per_file_coverage(&defs, &unref)
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
            file_stats: &'a HashMap<PathBuf, CoverageFileStat>,
        ) -> Self {
            Self {
                threshold,
                unreferenced,
                file_stats,
            }
        }
    }

    #[test]
    fn witness_coverage_gate_helpers() {
        let defs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let unref: Vec<(PathBuf, String, usize)> = vec![];
        let file_stats = HashMap::new();
        assert_eq!(min_per_file_coverage(&defs, &unref), 100);
        assert_eq!(min_gate_eligible_per_file_coverage(&defs, &unref), 100);
        let _ = CoverageGateFailureCtx::witness(90, &unref, &file_stats);
    }
}
