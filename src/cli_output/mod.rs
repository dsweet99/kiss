use crate::discovery::Language;
use crate::duplication::{DuplicateCluster, DuplicatePair};
use crate::violation::Violation;
use std::path::Path;

pub const VIOLATIONS_FIX_HINT: &str =
    "Run 'kiss rules' for more information about fixing violations.";

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

pub fn format_violation(v: &Violation) -> String {
    format!(
        "VIOLATION:{}:{}:{}:{}: {} {}",
        v.metric,
        v.file.display(),
        v.line,
        v.unit_name,
        v.message,
        v.suggestion
    )
}

pub fn print_violations(viols: &[Violation]) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    for v in viols {
        let _ = writeln!(w, "{}", format_violation(v));
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
}
