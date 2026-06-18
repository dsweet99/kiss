use std::collections::BTreeSet;

use crate::CoverageMetadata;

pub fn executable_lines_from_source(source: &str) -> Vec<usize> {
    let mut scan = ExecutableLineScan::default();
    for (idx, line) in source.lines().enumerate() {
        scan.visit_line(idx + 1, line);
    }
    scan.executable
}

#[derive(Default)]
struct ExecutableLineScan {
    executable: Vec<usize>,
    first_statement_seen: bool,
    in_module_docstring: bool,
    in_multiline_string: bool,
    suite_docstring_parent_indent: Option<usize>,
    bracket_depth: usize,
    bracket_continuation_is_import: bool,
    bracket_continuation_is_suite_header: bool,
    bracket_continuation_suite_parent_indent: Option<usize>,
}

impl ExecutableLineScan {
    fn visit_line(&mut self, line_no: usize, line: &str) {
        let trimmed = line.trim();
        if self.consume_non_executable_line(line, trimmed) {
            return;
        }
        let in_bracket_continuation = self.bracket_depth > 0;
        let opens_multiline_string = has_unclosed_triple_quote(trimmed);
        if should_record_executable_line(LineRecordContext {
            in_bracket_continuation,
            bracket_continuation_is_import: self.bracket_continuation_is_import,
            bracket_continuation_is_suite_header: self.bracket_continuation_is_suite_header,
            trimmed,
        }) {
            self.executable.push(line_no);
        }
        let new_bracket_depth = update_bracket_depth(self.bracket_depth, trimmed);
        if self.bracket_depth == 0 && new_bracket_depth > 0 {
            self.bracket_continuation_is_import = is_import_continuation_start(trimmed);
            if is_suite_header_continuation_start(trimmed) {
                self.bracket_continuation_is_suite_header = true;
                self.bracket_continuation_suite_parent_indent = Some(indent_width(line));
            }
        } else if new_bracket_depth == 0 {
            if self.bracket_continuation_is_suite_header && trimmed.ends_with(':') {
                self.suite_docstring_parent_indent = self.bracket_continuation_suite_parent_indent;
            }
            self.bracket_continuation_is_import = false;
            self.bracket_continuation_is_suite_header = false;
            self.bracket_continuation_suite_parent_indent = None;
        }
        self.bracket_depth = new_bracket_depth;
        if opens_multiline_string {
            self.in_multiline_string = true;
        }
        if opens_docstring_suite(trimmed) {
            self.suite_docstring_parent_indent = Some(indent_width(line));
        }
    }

    fn consume_non_executable_line(&mut self, line: &str, trimmed: &str) -> bool {
        if is_ignored_line(trimmed) {
            return true;
        }
        if consume_multiline_string_line(trimmed, &mut self.in_multiline_string) {
            return true;
        }
        if consume_module_docstring_line(
            trimmed,
            &mut self.first_statement_seen,
            &mut self.in_module_docstring,
        ) {
            return true;
        }
        self.consume_suite_docstring_line(line, trimmed)
    }

    fn consume_suite_docstring_line(&mut self, line: &str, trimmed: &str) -> bool {
        let indent = indent_width(line);
        if !consume_suite_docstring_line(trimmed, indent, &mut self.suite_docstring_parent_indent) {
            return false;
        }
        if !ends_single_line_triple_quoted_string(trimmed) {
            self.in_multiline_string = true;
        }
        true
    }
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

fn consume_suite_docstring_line(
    trimmed: &str,
    indent: usize,
    parent_indent: &mut Option<usize>,
) -> bool {
    let Some(parent) = *parent_indent else {
        return false;
    };
    *parent_indent = None;
    indent > parent && starts_triple_quoted_string(trimmed)
}

fn opens_docstring_suite(trimmed: &str) -> bool {
    (trimmed.starts_with("def ")
        || trimmed.starts_with("async def ")
        || trimmed.starts_with("class "))
        && trimmed.ends_with(':')
}

fn indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

fn update_bracket_depth(mut depth: usize, line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'#' => break,
            b'\'' | b'"' => idx = skip_string_literal(bytes, idx),
            b'(' | b'[' | b'{' => {
                depth += 1;
                idx += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                idx += 1;
            }
            _ => idx += 1,
        }
    }
    depth
}

fn skip_string_literal(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    if bytes.get(start + 1) == Some(&quote) && bytes.get(start + 2) == Some(&quote) {
        return skip_triple_quoted_string(bytes, start + 3, quote);
    }
    let mut idx = start + 1;
    while idx < bytes.len() {
        if bytes[idx] == b'\\' {
            idx = (idx + 2).min(bytes.len());
        } else if bytes[idx] == quote {
            return idx + 1;
        } else {
            idx += 1;
        }
    }
    bytes.len()
}

fn skip_triple_quoted_string(bytes: &[u8], mut idx: usize, quote: u8) -> usize {
    while idx + 2 < bytes.len() {
        if bytes[idx] == quote && bytes[idx + 1] == quote && bytes[idx + 2] == quote {
            return idx + 3;
        }
        idx += 1;
    }
    bytes.len()
}

fn is_import_continuation_start(trimmed: &str) -> bool {
    (trimmed.starts_with("from ") || trimmed.starts_with("import ")) && trimmed.ends_with('(')
}

fn is_suite_header_continuation_start(trimmed: &str) -> bool {
    (trimmed.starts_with("def ")
        || trimmed.starts_with("async def ")
        || trimmed.starts_with("class "))
        && !trimmed.ends_with(':')
}

struct LineRecordContext<'a> {
    trimmed: &'a str,
    in_bracket_continuation: bool,
    bracket_continuation_is_import: bool,
    bracket_continuation_is_suite_header: bool,
}

fn should_record_executable_line(ctx: LineRecordContext<'_>) -> bool {
    if is_non_executable_structural_header(ctx.trimmed) {
        return false;
    }
    if starts_parenthesized_with_continuation(ctx.trimmed) {
        return false;
    }
    if ctx.in_bracket_continuation && ctx.bracket_continuation_is_suite_header {
        return false;
    }
    !ctx.in_bracket_continuation
        || (!ctx.bracket_continuation_is_import && !is_delimiter_only_continuation(ctx.trimmed))
}

fn is_non_executable_structural_header(trimmed: &str) -> bool {
    matches!(trimmed, "else:" | "finally:")
        || (trimmed.starts_with("match ") && trimmed.ends_with(':'))
}

fn starts_parenthesized_with_continuation(trimmed: &str) -> bool {
    (trimmed.starts_with("with (") || trimmed.starts_with("async with (")) && trimmed.ends_with('(')
}

fn is_delimiter_only_continuation(trimmed: &str) -> bool {
    !trimmed.is_empty()
        && trimmed
            .trim_end_matches(':')
            .trim_end_matches(',')
            .chars()
            .all(|ch| matches!(ch, ')' | ']' | '}'))
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

#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[test]
    fn function_docstrings_are_not_executable_lines() {
        let executable = executable_lines_from_source(concat!(
            "def f():\n",
            "    \"\"\"Function documentation.\"\"\"\n",
            "    return 1\n",
        ));
        assert_eq!(executable, vec![1, 3]);
    }

    #[test]
    fn class_docstrings_are_not_executable_lines() {
        let executable = executable_lines_from_source(concat!(
            "class C:\n",
            "    \"\"\"Class documentation.\"\"\"\n",
            "    def m(self):\n",
            "        return 1\n",
        ));
        assert_eq!(executable, vec![1, 3, 4]);
    }

    #[test]
    fn multiline_expression_items_are_executable_lines() {
        let executable = executable_lines_from_source(concat!(
            "values = [\n",
            "    one(),\n",
            "    two(),\n",
            "]\n",
        ));
        assert_eq!(executable, vec![1, 2, 3]);
    }

    #[test]
    fn structural_else_headers_are_not_executable_lines() {
        let executable = executable_lines_from_source(concat!(
            "if flag:\n",
            "    value = 1\n",
            "else:\n",
            "    value = 2\n",
        ));
        assert_eq!(executable, vec![1, 2, 4]);
    }

    #[test]
    fn match_subject_line_is_not_executable_when_cases_run() {
        let executable = executable_lines_from_source(concat!(
            "match value:\n",
            "    case 1:\n",
            "        label = 'one'\n",
            "    case _:\n",
            "        label = 'other'\n",
        ));
        assert_eq!(executable, vec![2, 3, 4, 5]);
    }

    #[test]
    fn parenthesized_with_opening_line_is_not_executable() {
        let executable = executable_lines_from_source(concat!(
            "with (\n",
            "    first() as one,\n",
            "    second() as two,\n",
            "):\n",
            "    use(one, two)\n",
        ));
        assert_eq!(executable, vec![2, 3, 5]);
    }

    #[test]
    fn multiline_function_header_docstring_is_not_executable() {
        let executable = executable_lines_from_source(concat!(
            "def f(\n",
            "    x,\n",
            "):\n",
            "    \"\"\"Function documentation.\"\"\"\n",
            "    return x\n",
        ));
        assert_eq!(executable, vec![1, 5]);
    }

    #[test]
    fn brackets_inside_strings_do_not_start_continuations() {
        let executable = executable_lines_from_source(concat!("text = \"(\"\n", "value = 1\n",));
        assert_eq!(executable, vec![1, 2]);
    }

    #[test]
    fn brackets_inside_strings_do_not_affect_later_import_continuations() {
        let executable = executable_lines_from_source(concat!(
            "text = \"(\"\n",
            "from pkg import (\n",
            "    alpha,\n",
            ")\n",
            "value = alpha\n",
        ));
        assert_eq!(executable, vec![1, 2, 5]);
    }
}
