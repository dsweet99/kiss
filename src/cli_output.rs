//! CLI output formatting functions

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

pub fn print_coverage_gate_failure(coverage: usize, threshold: usize, tested: usize, total: usize) {
    println!("❌ Test coverage too low to safely suggest refactoring.\n");
    println!("   Test reference coverage: {}% (threshold: {}%)", coverage, threshold);
    println!("   Functions with test references: {} / {}\n", tested, total);
    println!("   Add tests for untested code, then run kiss again.");
    println!("   Or use --all to bypass this check and proceed anyway.");
}


pub fn print_violations(viols: &[Violation], _total: usize, dup_count: usize) {
    if viols.is_empty() && dup_count == 0 { 
        println!("NO VIOLATIONS"); 
        return; 
    }
    for v in viols { 
        println!("VIOLATION:{}:{}: {} {}. {} {}", 
            v.file.display(), v.line, v.value, v.metric, v.message, v.suggestion); 
    }
}

pub fn print_duplicates(lang: &str, clusters: &[DuplicateCluster]) {
    let suggestion = if lang == "Rust" {
        "Extract into a shared function, or use traits/generics if the pattern varies by type."
    } else {
        "Extract common code into a shared function."
    };
    for c in clusters {
        if let Some(first) = c.chunks.first() {
            let locations: Vec<String> = c.chunks.iter()
                .map(|ch| format!("{}:{}-{}", ch.file.display(), ch.start_line, ch.end_line))
                .collect();
            println!("VIOLATION:{}:{}: {:.0}% duplication. {} copies of similar code: [{}]. {}",
                first.file.display(), first.start_line, c.avg_similarity * 100.0, 
                c.chunks.len(), locations.join(", "), suggestion);
        }
    }
}

pub fn print_py_test_refs(parsed: &[ParsedFile]) -> usize {
    if parsed.is_empty() { return 0; }
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs);
    if !analysis.unreferenced.is_empty() {
        println!("\n--- Possibly Untested Python Code ({} items) ---\n", analysis.unreferenced.len());
        println!("The following code units are not referenced by any test file.\n(Note: This is static analysis only; actual coverage may differ.)\n");
        for d in &analysis.unreferenced { 
            println!("  {}:{} {} '{}'", d.file.display(), d.line, d.kind.as_str(), d.name); 
        }
        println!("\nAdd tests that directly call these items, or remove them if they are dead code.");
    }
    analysis.unreferenced.len()
}

pub fn print_rs_test_refs(parsed: &[ParsedRustFile]) -> usize {
    if parsed.is_empty() { return 0; }
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs(&refs);
    if !analysis.unreferenced.is_empty() {
        println!("\n--- Possibly Untested Rust Code ({} items) ---\n", analysis.unreferenced.len());
        println!("The following code units are not referenced by any test.\n(Note: This is static analysis only; actual coverage may differ.)\n");
        for d in &analysis.unreferenced { 
            println!("  {}:{} {} '{}'", d.file.display(), d.line, d.kind, d.name); 
        }
        println!("\nAdd tests that directly reference these items, or remove them if they are dead code.");
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
        print_coverage_gate_failure(50, 80, 5, 10);
    }

    #[test]
    fn test_print_violations_empty() {
        print_violations(&[], 5, 0);
    }

    #[test]
    fn test_print_duplicates_empty() {
        print_duplicates("Test", &[]);
    }
}

