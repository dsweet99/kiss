use crate::Language;

use super::ast_models::ParseOutcome;
use super::ast_plan::{ast_definition_span_from_result, cached_parse_outcome};
use super::ast_rust::impl_owner_name;
use super::lex::{LexScan, LexState, StringState, step_lex_state};
use syn::ItemImpl;

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
    path: &std::path::Path,
) -> Option<DefinitionSpan> {
    match cached_parse_outcome(content, path, language) {
        ParseOutcome::Success(result) => ast_definition_span_from_result(&result, method, owner)
            .map(|(start, end)| DefinitionSpan { start, end }),
        ParseOutcome::Fail(_) => None,
    }
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

pub(super) fn find_impl_blocks(content: &str, owner: &str) -> Vec<(usize, usize)> {
    let mut results = Vec::new();
    let mut search_start = 0;
    while let Some(rel) = content[search_start..].find("impl") {
        let start = search_start + rel;
        let prev_ok = start == 0
            || content[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after_impl = start + "impl".len();
        let next_ok = content[after_impl..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if !prev_ok || !next_ok {
            search_start = after_impl;
            continue;
        }
        let Some(open_rel) = content[start..].find('{') else {
            break;
        };
        let open = start + open_rel;
        if let Some(end) = find_brace_block_end(content, open) {
            let candidate = format!("{}{}", &content[start..open], "{}");
            if let Ok(item_impl) = syn::parse_str::<ItemImpl>(&candidate)
                && impl_owner_name(&item_impl.self_ty).as_deref() == Some(owner)
            {
                results.push((start, end));
            }
            search_start = end;
            continue;
        }
        search_start = after_impl;
    }
    results
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
    fn find_definition_span_rust_impl_fn() {
        let src = "struct X;\nimpl X {\n    pub fn m(&self) { let _ = 1; }\n}\n";
        let sp = find_definition_span(
            src,
            "m",
            Some("X"),
            Language::Rust,
            std::path::Path::new("def-rust-impl"),
        )
        .unwrap();
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
        let (lo, hi) = find_impl_blocks(impl_src, "Foo")[0];
        assert!(hi > lo);
        let py = "class C:\n    x = 1\n";
        let (a, b) = find_python_class_block(py, "C").unwrap();
        assert!(b > a);
    }

    #[test]
    fn extend_class_block_multiline() {
        let src = "class Foo:\n    a = 1\n    b = 2\n    c = 3\nbar = 4\n";
        let (start, end) = find_python_class_block(src, "Foo").unwrap();
        let block = &src[start..end];
        assert!(block.contains("a = 1"));
        assert!(block.contains("c = 3"));
        assert!(
            !block.contains("bar = 4"),
            "extend_class_block should stop at dedent"
        );
    }

    #[test]
    fn rust_lexer_is_inside_non_code_states() {
        let st_default = LexState::default();
        assert!(!rust_lexer_is_inside_non_code(&st_default));

        let st_line = LexState {
            line_comment: true,
            ..LexState::default()
        };
        assert!(rust_lexer_is_inside_non_code(&st_line));

        let st_block = LexState {
            block_comment_depth: 1,
            ..LexState::default()
        };
        assert!(rust_lexer_is_inside_non_code(&st_block));

        let st_string = LexState {
            string_state: StringState::Double,
            ..LexState::default()
        };
        assert!(rust_lexer_is_inside_non_code(&st_string));
    }
}
