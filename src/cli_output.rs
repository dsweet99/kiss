use crate::discovery::Language;
use crate::duplication::DuplicateCluster;
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_test_refs::analyze_rust_test_refs;
use crate::test_refs::analyze_test_refs;
use crate::violation::Violation;
use std::path::Path;

pub fn print_no_files_message(lang_filter: Option<Language>, root: &Path) {
    let msg = match lang_filter {
        Some(Language::Python) => "No Python files",
        Some(Language::Rust) => "No Rust files",
        None => "No files",
    };
    println!("{} in {}", msg, root.display());
}

pub fn print_coverage_gate_failure(coverage: usize, threshold: usize, tested: usize, total: usize, unreferenced: &[(std::path::PathBuf, String, usize)]) {
    println!("GATE_FAILED:test_coverage: {coverage}% coverage (threshold: {threshold}%, {tested}/{total} units tested)");
    println!("Hint: Use --all to bypass coverage gate for exploration");
    for (file, name, line) in unreferenced {
        println!("VIOLATION:test_coverage:{}:{}:{}: Add test coverage for this code unit.", file.display(), line, name);
    }
}

pub fn print_violations(viols: &[Violation]) {
    for v in viols { 
        println!("VIOLATION:{}:{}:{}:{}: {} {}", 
            v.metric, v.file.display(), v.line, v.unit_name, v.message, v.suggestion); 
    }
}

pub fn print_final_status(has_violations: bool) {
    if !has_violations {
        println!("NO VIOLATIONS");
    }
}

pub fn print_duplicates(lang: &str, clusters: &[DuplicateCluster]) {
    let suggestion = if lang == "Rust" {
        "Extract into a shared function, or use traits/generics."
    } else {
        "Extract common code into a shared function."
    };
    for c in clusters {
        if let Some(first) = c.chunks.first() {
            let locations: Vec<String> = c.chunks.iter()
                .map(|ch| format!("{}:{}-{}", ch.file.display(), ch.start_line, ch.end_line))
                .collect();
            println!("VIOLATION:duplication:{}:{}:{}: {:.0}% similar, {} copies: [{}]. {}",
                first.file.display(), first.start_line, first.name,
                c.avg_similarity * 100.0, c.chunks.len(), locations.join(", "), suggestion);
        }
    }
}

pub fn print_py_test_refs(parsed: &[ParsedFile]) -> usize {
    if parsed.is_empty() { return 0; }
    let analysis = analyze_test_refs(&parsed.iter().collect::<Vec<_>>());
    for d in &analysis.unreferenced { 
        println!("WARNING:test_coverage:{}:{}:{}: Code unit may lack test coverage.", d.file.display(), d.line, d.name); 
    }
    analysis.unreferenced.len()
}

pub fn print_rs_test_refs(parsed: &[ParsedRustFile]) -> usize {
    if parsed.is_empty() { return 0; }
    let analysis = analyze_rust_test_refs(&parsed.iter().collect::<Vec<_>>());
    for d in &analysis.unreferenced { 
        println!("WARNING:test_coverage:{}:{}:{}: Code unit may lack test coverage.", d.file.display(), d.line, d.name); 
    }
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
        print_coverage_gate_failure(50, 80, 5, 10, &[(std::path::PathBuf::from("foo.py"), "bar".to_string(), 10)]);
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
}
