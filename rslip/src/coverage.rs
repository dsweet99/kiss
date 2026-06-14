use std::collections::BTreeSet;

use crate::CoverageMetadata;

pub fn executable_lines_from_source(source: &str) -> Vec<usize> {
    let mut executable = Vec::new();
    let mut first_statement_seen = false;
    let mut in_module_docstring = false;
    let mut in_multiline_string = false;
    let mut bracket_depth = 0usize;
    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if is_ignored_line(trimmed) {
            continue;
        }
        if consume_multiline_string_line(trimmed, &mut in_multiline_string) {
            continue;
        }
        if consume_module_docstring_line(
            trimmed,
            &mut first_statement_seen,
            &mut in_module_docstring,
        ) {
            continue;
        }
        let continuation_only = bracket_depth > 0;
        let opens_multiline_string = has_unclosed_triple_quote(trimmed);
        if !continuation_only {
            executable.push(line_no);
        }
        bracket_depth = update_bracket_depth(bracket_depth, trimmed);
        if opens_multiline_string {
            in_multiline_string = true;
        }
    }
    executable
}

fn is_ignored_line(trimmed: &str) -> bool {
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn consume_multiline_string_line(trimmed: &str, in_multiline_string: &mut bool) -> bool {
    if !*in_multiline_string {
        return false;
    }
    if has_unclosed_triple_quote(trimmed) {
        *in_multiline_string = false;
    }
    true
}

fn consume_module_docstring_line(
    trimmed: &str,
    first_statement_seen: &mut bool,
    in_module_docstring: &mut bool,
) -> bool {
    if !*first_statement_seen {
        *first_statement_seen = true;
        if starts_triple_quoted_string(trimmed) {
            *in_module_docstring = !ends_single_line_triple_quoted_string(trimmed);
            return true;
        }
        return false;
    }
    if !*in_module_docstring {
        return false;
    }
    if ends_triple_quoted_string(trimmed) {
        *in_module_docstring = false;
    }
    true
}

fn update_bracket_depth(mut depth: usize, line: &str) -> usize {
    for ch in line.chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

fn starts_triple_quoted_string(trimmed: &str) -> bool {
    trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''")
}

fn has_unclosed_triple_quote(trimmed: &str) -> bool {
    trimmed.matches("\"\"\"").count() % 2 == 1 || trimmed.matches("'''").count() % 2 == 1
}

fn ends_triple_quoted_string(trimmed: &str) -> bool {
    trimmed.ends_with("\"\"\"") || trimmed.ends_with("'''")
}

fn ends_single_line_triple_quoted_string(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix("\"\"\"") {
        rest.ends_with("\"\"\"")
    } else if let Some(rest) = trimmed.strip_prefix("'''") {
        rest.ends_with("'''")
    } else {
        false
    }
}

pub fn line_coverage(executable: &[usize], executed: &BTreeSet<usize>) -> CoverageMetadata {
    let executable_set: BTreeSet<_> = executable.iter().copied().collect();
    let executed_lines: Vec<_> = executed
        .iter()
        .copied()
        .filter(|line| executable_set.contains(line))
        .collect();
    let executed_set: BTreeSet<_> = executed_lines.iter().copied().collect();
    let missing_lines: Vec<_> = executable_set
        .iter()
        .copied()
        .filter(|line| !executed_set.contains(line))
        .collect();
    let percent_covered = if executable_set.is_empty() {
        100
    } else {
        ((executed_lines.len() * 100) + (executable_set.len() / 2)) / executable_set.len()
    };
    CoverageMetadata {
        executable_lines: executable_set.into_iter().collect(),
        executed_lines,
        missing_lines,
        percent_covered,
    }
}
