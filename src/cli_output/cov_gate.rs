use super::{final_status_message, format_unreferenced_unit_coverage_message};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct CoverageGateFailureCtx<'a> {
    pub threshold: usize,
    pub unreferenced: &'a [(PathBuf, String, usize)],
    pub file_pcts: &'a HashMap<PathBuf, usize>,
}

/// Diagnostics for codebase-scope gate failure: `(file, first_uncovered_line, file_pct)`.
pub struct CodebaseCoverageGateFailureCtx<'a> {
    pub percent: usize,
    pub threshold: usize,
    pub diagnostics: &'a [(PathBuf, usize, usize)],
}

#[allow(clippy::implicit_hasher)]
pub fn print_coverage_gate_failure(ctx: &CoverageGateFailureCtx<'_>) {
    for line in coverage_gate_failure_lines(ctx) {
        println!("{line}");
    }
}

pub fn print_codebase_coverage_gate_failure(ctx: &CodebaseCoverageGateFailureCtx<'_>) {
    for line in codebase_coverage_gate_failure_lines(ctx) {
        println!("{line}");
    }
}

pub fn codebase_coverage_gate_failure_lines(ctx: &CodebaseCoverageGateFailureCtx<'_>) -> Vec<String> {
    let mut lines = vec![format!(
        "GATE_FAILED:test_coverage: codebase coverage {}% below {}% threshold",
        ctx.percent, ctx.threshold
    )];
    for (file, line, pct) in ctx.diagnostics {
        let message = format_unreferenced_unit_coverage_message(*pct);
        lines.push(format!(
            "VIOLATION:test_coverage:{}:{}:<file>: {message}",
            file.display(),
            line
        ));
    }
    lines.push(final_status_message(true).to_string());
    lines
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::VIOLATIONS_FIX_HINT;

    #[test]
    fn test_print_coverage_gate_failure_emits_hint() {
        let file_pcts: HashMap<PathBuf, usize> = [(PathBuf::from("foo.py"), 50)].into();
        let lines = coverage_gate_failure_lines(&CoverageGateFailureCtx {
            threshold: 80,
            unreferenced: &[(PathBuf::from("foo.py"), "bar".to_string(), 10)],
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
        assert!(
            stdout.contains("per-file enforcement"),
            "expected per-file enforcement in stdout: {stdout}"
        );
    }

    #[test]
    fn test_codebase_coverage_gate_failure_lines() {
        let lines = codebase_coverage_gate_failure_lines(&CodebaseCoverageGateFailureCtx {
            percent: 80,
            threshold: 90,
            diagnostics: &[
                (PathBuf::from("good.py"), 4, 95),
                (PathBuf::from("bad.py"), 1, 0),
            ],
        });
        let stdout = lines.join("\n");
        assert!(
            stdout.contains(
                "GATE_FAILED:test_coverage: codebase coverage 80% below 90% threshold"
            ),
            "stdout:\n{stdout}"
        );
        assert!(
            !stdout.contains("per-file enforcement"),
            "codebase failure must not use per-file enforcement wording.\nstdout:\n{stdout}"
        );
        assert!(
            stdout.contains("VIOLATION:test_coverage:good.py:4:<file>:"),
            "≥-threshold file with uncovered lines must still appear.\nstdout:\n{stdout}"
        );
        assert!(stdout.contains(VIOLATIONS_FIX_HINT), "stdout:\n{stdout}");
    }
}
