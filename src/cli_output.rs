//! CLI output formatting functions

use crate::discovery::Language;
use crate::duplication::DuplicateCluster;
use crate::graph::{collect_instability_metrics, DependencyGraph};
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

pub fn print_instability(lang: &str, graph: Option<&DependencyGraph>) {
    let Some(g) = graph else { return; };
    let metrics = collect_instability_metrics(g);
    if metrics.is_empty() { return; }
    
    let top_unstable: Vec<_> = metrics.into_iter().take(10).collect();
    
    println!("\n--- {} Module Instability (top unstable) ---\n", lang);
    println!("  {:30} {:>10} {:>10} {:>12}", "Module", "Instability", "Fan-in", "Fan-out");
    println!("  {:30} {:>10} {:>10} {:>12}", "------", "-----------", "------", "-------");
    for m in &top_unstable {
        println!("  {:30} {:>10.1}% {:>10} {:>12}", m.module_name, m.instability * 100.0, m.fan_in, m.fan_out);
    }
    println!();
    println!("  Instability = Fan-out / (Fan-in + Fan-out)");
    println!("  Lower is more stable (more incoming deps than outgoing).");
}

pub fn print_violations(viols: &[Violation], total: usize) {
    if viols.is_empty() { 
        println!("✓ No violations found in {} files.", total); 
        return; 
    }
    println!("Found {} violations:\n", viols.len());
    for v in viols { 
        println!("{}:{}\n  {}\n  → {}\n", v.file.display(), v.line, v.message, v.suggestion); 
    }
}

pub fn print_duplicates(lang: &str, clusters: &[DuplicateCluster]) {
    if clusters.is_empty() { return; }
    println!("\n--- {} Duplicate Code Detected ({} clusters) ---\n", lang, clusters.len());
    for (i, c) in clusters.iter().enumerate() {
        println!("Cluster {}: {} copies (~{:.0}% similar)", i + 1, c.chunks.len(), c.avg_similarity * 100.0);
        for ch in &c.chunks { 
            println!("  {}:{}-{} ({})", ch.file.display(), ch.start_line, ch.end_line, ch.name); 
        }
        println!();
    }
}

pub fn print_py_test_refs(parsed: &[ParsedFile]) {
    if parsed.is_empty() { return; }
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs);
    if !analysis.unreferenced.is_empty() {
        println!("\n--- Possibly Untested Python Code ({} items) ---\n", analysis.unreferenced.len());
        println!("The following code units are not referenced by any test file.\n(Note: This is static analysis only; actual coverage may differ.)\n");
        for d in &analysis.unreferenced { 
            println!("  {}:{} {} '{}'", d.file.display(), d.line, d.kind.as_str(), d.name); 
        }
    }
}

pub fn print_rs_test_refs(parsed: &[ParsedRustFile]) {
    if parsed.is_empty() { return; }
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs(&refs);
    if !analysis.unreferenced.is_empty() {
        println!("\n--- Possibly Untested Rust Code ({} items) ---\n", analysis.unreferenced.len());
        println!("The following code units are not referenced by any test.\n(Note: This is static analysis only; actual coverage may differ.)\n");
        for d in &analysis.unreferenced { 
            println!("  {}:{} {} '{}'", d.file.display(), d.line, d.kind, d.name); 
        }
    }
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
    fn test_print_instability_no_panic() {
        print_instability("Test", None);
    }

    #[test]
    fn test_print_violations_empty() {
        print_violations(&[], 5);
    }

    #[test]
    fn test_print_duplicates_empty() {
        print_duplicates("Test", &[]);
    }
}

