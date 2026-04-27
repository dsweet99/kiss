use crate::Language;

use super::identifiers::line_start_offset;
use super::identifiers::previous_line_bounds;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum StringState {
    #[default]
    None,
    Single,
    Double,
    TripleSingle,
    TripleDouble,
    RawString(usize),
    /// Python f-string. `depth` tracks the `{ ... }` nesting depth: depth == 0
    /// means we're in the literal text portion (string-like), depth >= 1 means
    /// we're inside a code-bearing brace expression (code-like).
    FStringSingle {
        depth: usize,
    },
    FStringDouble {
        depth: usize,
    },
    FStringTripleSingle {
        depth: usize,
    },
    FStringTripleDouble {
        depth: usize,
    },
}

impl StringState {
    /// True when the byte at the current position is *string literal content*
    /// (not code) for the purposes of identifier-occurrence filtering. The
    /// brace-bearing region of an f-string (`depth >= 1`) counts as code.
    pub(super) const fn is_string_literal(self) -> bool {
        match self {
            Self::None => false,
            Self::FStringSingle { depth }
            | Self::FStringDouble { depth }
            | Self::FStringTripleSingle { depth }
            | Self::FStringTripleDouble { depth } => depth == 0,
            _ => true,
        }
    }
}

#[derive(Default)]
pub(super) struct LexState {
    pub line_comment: bool,
    pub block_comment_depth: usize,
    pub string_state: StringState,
}

pub(super) struct LexScan<'a> {
    pub state: &'a mut LexState,
    pub bytes: &'a [u8],
    pub idx: usize,
    pub target: usize,
    pub language: Language,
}

pub(super) fn is_code_offset(content: &str, target: usize, language: Language) -> bool {
    let bytes = content.as_bytes();
    let mut idx = 0usize;
    let mut state = LexState::default();
    while idx < target {
        let mut scan = LexScan {
            state: &mut state,
            bytes,
            idx,
            target,
            language,
        };
        idx += step_lex_state(&mut scan);
    }
    !(state.line_comment || state.block_comment_depth > 0 || state.string_state.is_string_literal())
}

pub(super) fn step_lex_state(scan: &mut LexScan<'_>) -> usize {
    let state = &mut *scan.state;
    let bytes = scan.bytes;
    let idx = scan.idx;
    let target = scan.target;
    if state.line_comment {
        if bytes[idx] == b'\n' {
            state.line_comment = false;
        }
        return 1;
    }
    if state.block_comment_depth > 0 {
        return step_block_comment(state, bytes, idx, target);
    }
    if let Some(consumed) = step_inside_string_state(state, bytes, idx, target) {
        return consumed;
    }
    step_code_state(scan)
}

/// Dispatch to the appropriate string-state stepper. Returns `None` when not
/// currently inside a string state (caller falls through to code stepping).
fn step_inside_string_state(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> Option<usize> {
    Some(match state.string_state {
        StringState::None => return None,
        StringState::RawString(hash_count) => {
            super::lex_rust::step_raw_string_state(state, bytes, idx, target, hash_count)
        }
        StringState::TripleSingle | StringState::TripleDouble => {
            step_triple_string_state(state, bytes, idx, target)
        }
        StringState::Single | StringState::Double => step_string_state(state, bytes, idx, target),
        StringState::FStringSingle { .. }
        | StringState::FStringDouble { .. }
        | StringState::FStringTripleSingle { .. }
        | StringState::FStringTripleDouble { .. } => {
            super::lex_fstring::step_fstring_state(state, bytes, idx, target)
        }
    })
}

fn step_block_comment(state: &mut LexState, bytes: &[u8], idx: usize, target: usize) -> usize {
    if idx + 1 < target && bytes[idx] == b'/' && bytes[idx + 1] == b'*' {
        state.block_comment_depth += 1;
        2
    } else if idx + 1 < target && bytes[idx] == b'*' && bytes[idx + 1] == b'/' {
        state.block_comment_depth = state.block_comment_depth.saturating_sub(1);
        2
    } else {
        1
    }
}

fn step_string_state(state: &mut LexState, bytes: &[u8], idx: usize, target: usize) -> usize {
    if bytes[idx] == b'\\' && idx + 1 < target {
        return 2;
    }
    match state.string_state {
        StringState::Single if bytes[idx] == b'\'' => state.string_state = StringState::None,
        StringState::Double if bytes[idx] == b'"' => state.string_state = StringState::None,
        _ => {}
    }
    1
}

fn step_triple_string_state(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> usize {
    if bytes[idx] == b'\\' && idx + 1 < target {
        return 2;
    }
    let quote = if state.string_state == StringState::TripleSingle {
        b'\''
    } else {
        b'"'
    };
    if bytes[idx] == quote && idx + 2 < target && bytes[idx + 1] == quote && bytes[idx + 2] == quote
    {
        state.string_state = StringState::None;
        return 3;
    }
    1
}

fn step_code_state(scan: &mut LexScan<'_>) -> usize {
    let state = &mut *scan.state;
    let bytes = scan.bytes;
    let idx = scan.idx;
    let target = scan.target;
    match scan.language {
        Language::Python => step_python_code_state(state, bytes, idx, target),
        Language::Rust => super::lex_rust::step_rust_code_state(state, bytes, idx, target),
    }
}

fn step_python_code_state(state: &mut LexState, bytes: &[u8], idx: usize, target: usize) -> usize {
    if bytes[idx] == b'#' {
        state.line_comment = true;
        return 1;
    }
    if let Some(consumed) =
        super::lex_fstring::try_parse_python_fstring_start(state, bytes, idx, target)
    {
        return consumed;
    }
    open_python_string_at(state, bytes, idx, target).unwrap_or(1)
}

/// Open a regular (non-f-string) Python string literal at `idx`, distinguishing
/// single vs triple quote. Returns `None` if the byte is not `'` or `"`.
fn open_python_string_at(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> Option<usize> {
    let quote = bytes[idx];
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let triple = idx + 2 < target && bytes[idx + 1] == quote && bytes[idx + 2] == quote;
    state.string_state = match (quote, triple) {
        (b'\'', false) => StringState::Single,
        (b'\'', true) => StringState::TripleSingle,
        (_, false) => StringState::Double,
        (_, true) => StringState::TripleDouble,
    };
    Some(if triple { 3 } else { 1 })
}

pub(super) fn rust_item_start(content: &str, offset: usize) -> usize {
    let mut start = line_start_offset(content, offset);
    while let Some((prev_start, prev_end)) = previous_line_bounds(content, start) {
        if content[prev_start..prev_end].trim_start().starts_with("#[") {
            start = prev_start;
        } else {
            break;
        }
    }
    start
}

#[cfg(test)]
mod lex_coverage {
    use super::*;
    use crate::Language;

    #[test]
    fn is_code_offset_basics_and_lex_helpers() {
        assert!(is_code_offset("x", 0, Language::Python));
        assert!(is_code_offset("x = 1", 0, Language::Python));
        assert!(!is_code_offset("# comment\nx", 2, Language::Python));
        assert!(is_code_offset("let x = 1;", 0, Language::Rust));
        assert!(!is_code_offset("// comment\nx", 3, Language::Rust));
        assert!(!is_code_offset("'''hello'''", 4, Language::Python));
        assert!(!is_code_offset("r#\"hello\"#", 4, Language::Rust));
        assert!(!is_code_offset("let s = \"hello\";", 10, Language::Rust));

        let mut st = LexState::default();
        let bytes = b"//\nx";
        let mut scan = LexScan {
            state: &mut st,
            bytes,
            idx: 0,
            target: bytes.len(),
            language: Language::Rust,
        };
        step_lex_state(&mut scan);
        let mut scan2 = LexScan {
            state: &mut st,
            bytes: b"x",
            idx: 0,
            target: 1,
            language: Language::Python,
        };
        step_code_state(&mut scan2);
        step_block_comment(&mut LexState::default(), b"/*", 0, 2);
        let mut st3 = LexState {
            string_state: StringState::Single,
            ..LexState::default()
        };
        step_string_state(&mut st3, b"'", 0, 1);
        assert_eq!(rust_item_start("#[inline]\nfn a() {}", 15), 0);
    }

    #[test]
    fn triple_string_states() {
        assert!(!is_code_offset("'''abc'''", 4, Language::Python));
        assert!(is_code_offset("'''abc'''", 0, Language::Python));
        assert!(!is_code_offset(r#""""xyz""""#, 4, Language::Python));
        assert!(!is_code_offset("'''a\\'b'''", 5, Language::Python));
    }

    #[test]
    fn comments_and_strings() {
        let nested = "/* outer /* inner */ still comment */ code";
        assert!(!is_code_offset(nested, 10, Language::Rust));
        assert!(!is_code_offset(nested, 25, Language::Rust));
        assert!(is_code_offset(
            nested,
            nested.find("code").unwrap(),
            Language::Rust
        ));

        let py = "x = 1 # a comment\ny = 2";
        assert!(is_code_offset(py, 0, Language::Python));
        assert!(!is_code_offset(py, 8, Language::Python));
        assert!(is_code_offset(
            py,
            py.find("y = 2").unwrap(),
            Language::Python
        ));

        assert!(!is_code_offset("x = 'hello'", 6, Language::Python));
        assert!(!is_code_offset("let s = \"world\";", 10, Language::Rust));

        let line_cmt = "// line comment\ncode";
        assert!(!is_code_offset(line_cmt, 5, Language::Rust));
        assert!(is_code_offset(
            line_cmt,
            line_cmt.find("code").unwrap(),
            Language::Rust
        ));

        let mut state = LexState {
            string_state: StringState::TripleSingle,
            ..LexState::default()
        };
        assert_eq!(step_triple_string_state(&mut state, b"'''abc'''", 0, 7), 3);
        state.string_state = StringState::Single;
        assert_eq!(step_string_state(&mut state, b"'abc", 0, 4), 1);
        state.string_state = StringState::None;
        assert_eq!(step_code_state(&mut LexScan {
            state: &mut state,
            bytes: b"x#y",
            idx: 1,
            target: 3,
            language: Language::Rust,
        }), 1);
        let mut inside = LexState {
            string_state: StringState::FStringDouble { depth: 1 },
            ..LexState::default()
        };
        assert_eq!(step_inside_string_state(&mut inside, b"{x", 0, 2), Some(1));
        assert_eq!(open_python_string_at(&mut inside, b"\"x\"", 0, 3), Some(1));
    }
}
