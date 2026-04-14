use crate::Language;

use super::lex::{rust_item_start, step_lex_state, LexScan, LexState, StringState};

#[derive(Clone, Copy)]
pub struct DefinitionSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl DefinitionSpan {
    pub const fn contains(self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

pub fn find_definition_span(
    content: &str,
    method: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<DefinitionSpan> {
    match language {
        Language::Python => find_python_definition_span(content, method, owner),
        Language::Rust => find_rust_definition_span(content, method, owner),
    }
}

fn find_python_definition_span(
    content: &str,
    method: &str,
    owner: Option<&str>,
) -> Option<DefinitionSpan> {
    let (range_start, range_end) = owner
        .and_then(|class_name| find_python_class_block(content, class_name))
        .unwrap_or((0, content.len()));
    let scope = &content[range_start..range_end];
    let needle = format!("def {method}(");
    let lines = split_lines_with_offsets(scope);
    let mut def_start = None;
    let mut def_indent = 0;

    for (idx, (line_offset, line)) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if def_start.is_none() && trimmed.starts_with(&needle) {
            def_start = Some(range_start + decorated_start(&lines, idx, indent, *line_offset));
            def_indent = indent;
        } else if let Some(start) = def_start
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && indent <= def_indent
        {
            return Some(DefinitionSpan {
                start,
                end: range_start + line_offset,
            });
        }
    }

    def_start.map(|start| DefinitionSpan {
        start,
        end: range_end,
    })
}

fn split_lines_with_offsets(scope: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    scope
        .split_inclusive('\n')
        .map(|line| {
            let current = offset;
            offset += line.len();
            (current, line.strip_suffix('\n').unwrap_or(line))
        })
        .collect()
}

fn decorated_start(lines: &[(usize, &str)], idx: usize, indent: usize, fallback: usize) -> usize {
    let mut start = fallback;
    let mut back = idx;
    while back > 0 {
        let (prev_offset, prev_line) = lines[back - 1];
        let prev_trimmed = prev_line.trim_start();
        let prev_indent = prev_line.len() - prev_trimmed.len();
        if prev_indent == indent && prev_trimmed.starts_with('@') {
            start = prev_offset;
            back -= 1;
        } else {
            break;
        }
    }
    start
}

fn find_rust_definition_span(
    content: &str,
    method: &str,
    owner: Option<&str>,
) -> Option<DefinitionSpan> {
    let (lo, hi) = owner
        .and_then(|type_name| find_impl_block(content, type_name))
        .unwrap_or((0, content.len()));
    let scope = &content[lo..hi];
    let fn_start = [format!("fn {method}("), format!("pub fn {method}(")]
        .iter()
        .find_map(|candidate| scope.find(candidate))
        .map(|pos| lo + pos)?;
    let open = fn_start + content[fn_start..].find('{')?;
    find_brace_block_end(content, open).map(|end| DefinitionSpan {
        start: rust_item_start(content, fn_start),
        end,
    })
}

pub(super) fn find_brace_block_end(content: &str, open_brace: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut idx = open_brace;
    let mut state = LexState::default();
    while idx < bytes.len() {
        if !rust_lexer_is_inside_non_code(&state) {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(idx + 1);
                    }
                }
                _ => {}
            }
        }
        let mut scan = LexScan {
            state: &mut state,
            bytes,
            idx,
            target: bytes.len(),
            language: Language::Rust,
        };
        idx += step_lex_state(&mut scan);
    }
    None
}

fn rust_lexer_is_inside_non_code(state: &LexState) -> bool {
    state.line_comment || state.block_comment_depth > 0 || state.string_state != StringState::None
}

pub(super) fn find_impl_block(content: &str, owner: &str) -> Option<(usize, usize)> {
    let start = content.find(&format!("impl {owner}"))?;
    let open = start + content[start..].find('{')?;
    find_brace_block_end(content, open).map(|end| (start, end))
}

pub(super) fn find_python_class_block(content: &str, class_name: &str) -> Option<(usize, usize)> {
    let prefix = format!("class {class_name}");
    let mut offset = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&prefix) && trimmed[prefix.len()..].starts_with([':', '(', ' ']) {
            let base_indent = line.len() - trimmed.len();
            let start = offset;
            let line_end = offset + line.len() + 1;
            let end = extend_class_block(content, line_end, base_indent);
            return Some((start, end.min(content.len())));
        }
        offset += line.len() + 1;
    }
    None
}

fn extend_class_block(content: &str, mut end: usize, base_indent: usize) -> usize {
    for next_line in content[end..].lines() {
        let next_trimmed = next_line.trim_start();
        if !next_trimmed.is_empty() && !next_trimmed.starts_with('#') {
            let indent = next_line.len() - next_trimmed.len();
            if indent <= base_indent {
                break;
            }
        }
        end += next_line.len() + 1;
    }
    end
}

#[cfg(test)]
mod definition_coverage {
    use super::*;

    #[test]
    fn find_definition_span_python_class_method() {
        let src = "class C:\n    @decorated\n    def m(self):\n        pass\n";
        let sp = find_definition_span(src, "m", Some("C"), Language::Python).unwrap();
        assert!(sp.contains(sp.start));
        assert!(sp.end > sp.start);
    }

    #[test]
    fn find_definition_span_rust_impl_fn() {
        let src = "struct X;\nimpl X {\n    pub fn m(&self) { let _ = 1; }\n}\n";
        let sp = find_definition_span(src, "m", Some("X"), Language::Rust).unwrap();
        assert!(sp.end > sp.start);
    }

    #[test]
    fn brace_and_impl_helpers() {
        let src = "{ a { b } }";
        let open = src.find('{').unwrap();
        assert_eq!(find_brace_block_end(src, open), Some(src.len()));
        let src_with_string = "fn foo() { let s = \"}\"; foo(); }";
        let open_with_string = src_with_string.find('{').unwrap();
        assert_eq!(
            find_brace_block_end(src_with_string, open_with_string),
            Some(src_with_string.len())
        );
        let impl_src = "impl Foo { fn x() {} }";
        let (lo, hi) = find_impl_block(impl_src, "Foo").unwrap();
        assert!(hi > lo);
        let py = "class C:\n    x = 1\n";
        let (a, b) = find_python_class_block(py, "C").unwrap();
        assert!(b > a);
    }
}
