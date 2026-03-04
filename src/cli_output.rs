use crate::discovery::Language;
use crate::duplication::{DuplicateCluster, DuplicatePair};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::test_refs::analyze_test_refs;
use crate::violation::Violation;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Format a candidate list for display, truncating to `max` items with ellipsis.
pub fn format_candidate_list(candidates: &[String], max: usize) -> String {
    if candidates.len() > max {
        format!("{}…", candidates[..max].join(", "))
    } else {
        candidates.join(", ")
    }
}

pub fn file_coverage_map(
    definitions: &[(PathBuf, String, usize)],
    unreferenced: &[(PathBuf, String, usize)],
) -> HashMap<PathBuf, usize> {
    let mut defs_per_file: HashMap<PathBuf, usize> = HashMap::new();
    let mut unref_per_file: HashMap<PathBuf, usize> = HashMap::new();
    for (file, _, _) in definitions {
        *defs_per_file.entry(file.clone()).or_default() += 1;
    }
    for (file, _, _) in unreferenced {
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

#[allow(clippy::implicit_hasher)]
pub fn print_coverage_gate_failure(
    coverage: usize,
    threshold: usize,
    tested: usize,
    total: usize,
    unreferenced: &[(std::path::PathBuf, String, usize)],
    file_pcts: &HashMap<std::path::PathBuf, usize>,
) {
    println!(
        "GATE_FAILED:test_coverage: {coverage}% coverage (threshold: {threshold}%, {tested}/{total} units tested)"
    );
    println!("Hint: Use --all to bypass coverage gate for exploration");
    for (file, name, line) in unreferenced {
        let pct = file_pcts.get(file).copied().unwrap_or(0);
        println!(
            "VIOLATION:test_coverage:{}:{}:{}: {pct}% covered. Add test coverage for this code unit.",
            file.display(),
            line,
            name
        );
    }
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
    if !has_violations {
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_print_no_files_message_no_panic() {
        let tmp = TempDir::new().unwrap();
        print_no_files_message(None, tmp.path());
        print_no_files_message(Some(Language::Python), tmp.path());
    }

    #[test]
    fn test_print_coverage_gate_failure_no_panic() {
        let file_pcts: HashMap<std::path::PathBuf, usize> =
            [(std::path::PathBuf::from("foo.py"), 50)].into();
        print_coverage_gate_failure(
            50,
            80,
            5,
            10,
            &[(std::path::PathBuf::from("foo.py"), "bar".to_string(), 10)],
            &file_pcts,
        );
    }

    #[test]
    fn test_print_violations_empty() {
        print_violations(&[]);
        print_final_status(false);
        print_final_status(true);
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
}
