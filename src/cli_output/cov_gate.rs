use super::{
    coverage_unit_name, extra_coverable_lines_to_reach, format_unreferenced_unit_coverage_message,
};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct CoverageFileStat {
    pub percent: usize,
    pub covered_lines: usize,
    pub total_lines: usize,
}

pub struct CoverageGateFailureCtx<'a> {
    pub threshold: usize,
    pub unreferenced: &'a [(PathBuf, String, usize)],
    pub file_stats: &'a HashMap<PathBuf, CoverageFileStat>,
}

pub struct CodebaseCoverageGateFailureCtx<'a> {
    pub percent: usize,
    pub threshold: usize,
    pub diagnostics: &'a [(PathBuf, usize, CoverageFileStat)],
}

#[allow(clippy::implicit_hasher)]
pub fn print_coverage_gate_failure(ctx: &CoverageGateFailureCtx<'_>) {
    for line in coverage_gate_failure_lines(ctx) {
        println!("{line}");
    }
    println!("{}", super::final_status_message(true));
}

pub fn print_codebase_coverage_gate_failure(ctx: &CodebaseCoverageGateFailureCtx<'_>) {
    for line in codebase_coverage_gate_failure_lines(ctx) {
        println!("{line}");
    }
    println!("{}", super::final_status_message(true));
}

pub fn codebase_coverage_gate_failure_lines(
    ctx: &CodebaseCoverageGateFailureCtx<'_>,
) -> Vec<String> {
    let mut lines = vec![format!(
        "VIOLATION:test_coverage: codebase coverage {}% below {}% threshold",
        ctx.percent, ctx.threshold
    )];
    for (file, line, stat) in ctx.diagnostics {
        let message = format_unreferenced_unit_coverage_message(
            stat.percent,
            stat.covered_lines,
            stat.total_lines,
            ctx.threshold,
        );
        lines.push(format!(
            "VIOLATION:test_coverage:{}:{}:{}: {message}",
            file.display(),
            line,
            coverage_unit_name(file)
        ));
    }
    lines
}

#[allow(clippy::implicit_hasher)]
pub fn coverage_gate_failure_lines(ctx: &CoverageGateFailureCtx<'_>) -> Vec<String> {
    let threshold = ctx.threshold;
    let mut failing: Vec<_> = ctx
        .file_stats
        .iter()
        .filter(|(_, stat)| stat.percent < threshold)
        .map(|(f, s)| (f.clone(), s))
        .collect();
    failing.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = vec![format!(
        "VIOLATION:test_coverage: {n} file(s) below {threshold}% threshold (per-file enforcement)",
        n = failing.len()
    )];
    for (file, stat) in &failing {
        let need = extra_coverable_lines_to_reach(stat.covered_lines, stat.total_lines, threshold);
        lines.push(format!(
            "  {}: {}% ({}/{}; need {need} more to reach {threshold}%)",
            file.display(),
            stat.percent,
            stat.covered_lines,
            stat.total_lines
        ));
    }
    for (file, name, line) in ctx.unreferenced {
        let Some(stat) = ctx.file_stats.get(file) else {
            continue;
        };
        if stat.percent < threshold {
            let message = format_unreferenced_unit_coverage_message(
                stat.percent,
                stat.covered_lines,
                stat.total_lines,
                threshold,
            );
            lines.push(format!(
                "VIOLATION:test_coverage:{}:{}:{}: {message}",
                file.display(),
                line,
                name
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::super::VIOLATIONS_FIX_HINT;
    use super::*;

    #[test]
    fn test_print_coverage_gate_failure_emits_hint() {
        let file_stats: HashMap<PathBuf, CoverageFileStat> = [(
            PathBuf::from("foo.py"),
            CoverageFileStat {
                percent: 50,
                covered_lines: 1,
                total_lines: 2,
            },
        )]
        .into();
        let lines = coverage_gate_failure_lines(&CoverageGateFailureCtx {
            threshold: 80,
            unreferenced: &[(PathBuf::from("foo.py"), "foo".to_string(), 10)],
            file_stats: &file_stats,
        });
        let stdout = lines.join("\n");
        assert!(
            !stdout.contains(VIOLATIONS_FIX_HINT),
            "diagnostic lines omit final status so sibling gates can print once: {stdout}"
        );
        assert!(
            stdout.contains("VIOLATION:test_coverage:"),
            "expected coverage violation in stdout: {stdout}"
        );
        assert!(
            stdout.contains("per-file enforcement"),
            "expected per-file enforcement in stdout: {stdout}"
        );
        assert!(
            stdout.contains("1/2"),
            "covered/total must cross the printer: {stdout}"
        );
        assert_eq!(
            super::super::final_status_message(true),
            VIOLATIONS_FIX_HINT
        );
    }

    #[test]
    fn test_codebase_coverage_gate_failure_lines() {
        let lines = codebase_coverage_gate_failure_lines(&CodebaseCoverageGateFailureCtx {
            percent: 80,
            threshold: 90,
            diagnostics: &[
                (
                    PathBuf::from("good.py"),
                    4,
                    CoverageFileStat {
                        percent: 95,
                        covered_lines: 19,
                        total_lines: 20,
                    },
                ),
                (
                    PathBuf::from("bad.py"),
                    1,
                    CoverageFileStat {
                        percent: 0,
                        covered_lines: 0,
                        total_lines: 2,
                    },
                ),
            ],
        });
        let stdout = lines.join("\n");
        assert!(
            stdout.contains("VIOLATION:test_coverage: codebase coverage 80% below 90% threshold"),
            "stdout:\n{stdout}"
        );
        assert!(
            !stdout.contains("per-file enforcement"),
            "codebase failure must not use per-file enforcement wording.\nstdout:\n{stdout}"
        );
        assert!(
            stdout.contains("VIOLATION:test_coverage:good.py:4:good:"),
            "≥-threshold file with uncovered lines must still appear.\nstdout:\n{stdout}"
        );
        assert!(
            !stdout.contains(VIOLATIONS_FIX_HINT),
            "diagnostic lines omit final status:\n{stdout}"
        );
    }
}
