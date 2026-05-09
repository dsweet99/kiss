//! Rust-specific lexer steppers and helpers used by `is_code_offset`.
//!
//! Handles Rust line/block comments, raw strings (`r"…"` / `r#…"…"#…`), and
//! single-quote char literals (`'a'`, `'\n'`, etc.).

use super::lex::{LexState, StringState};

pub(super) fn step_rust_code_state(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> usize {
    if idx + 1 < target && bytes[idx] == b'/' && bytes[idx + 1] == b'/' {
        state.line_comment = true;
        return 2;
    }
    if idx + 1 < target && bytes[idx] == b'/' && bytes[idx + 1] == b'*' {
        state.block_comment_depth = 1;
        return 2;
    }
    if let Some((hash_count, consumed)) = try_parse_raw_string_start(bytes, idx, target) {
        state.string_state = StringState::RawString(hash_count);
        return consumed;
    }
    if let Some(consumed) = try_parse_char_literal(bytes, idx, target) {
        return consumed;
    }
    if bytes[idx] == b'"' {
        state.string_state = StringState::Double;
    }
    1
}

pub(super) fn try_parse_raw_string_start(
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> Option<(usize, usize)> {
    if bytes[idx] != b'r' {
        return None;
    }
    let mut hash_count = 0;
    let mut check_idx = idx + 1;
    while check_idx < target && bytes[check_idx] == b'#' {
        hash_count += 1;
        check_idx += 1;
    }
    (check_idx < target && bytes[check_idx] == b'"').then_some((hash_count, 2 + hash_count))
}

pub(super) fn try_parse_char_literal(bytes: &[u8], idx: usize, target: usize) -> Option<usize> {
    if bytes[idx] != b'\'' || idx + 1 >= target {
        return None;
    }
    let next = bytes[idx + 1];
    if next == b'\\' {
        let mut end = idx + 2;
        while end < target && bytes[end] != b'\'' {
            end += 1;
        }
        if end < target && bytes[end] == b'\'' {
            return Some(end - idx + 1);
        }
        return None;
    }
    (idx + 2 < target && bytes[idx + 2] == b'\'').then_some(3)
}

pub(super) fn step_raw_string_state(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
    hash_count: usize,
) -> usize {
    if bytes[idx] != b'"' {
        return 1;
    }
    let mut hashes_found = 0;
    let mut check_idx = idx + 1;
    while check_idx < target && bytes[check_idx] == b'#' && hashes_found < hash_count {
        hashes_found += 1;
        check_idx += 1;
    }
    if hashes_found == hash_count {
        state.string_state = StringState::None;
        1 + hash_count
    } else {
        1
    }
}

#[cfg(test)]
mod lex_rust_coverage {
    use super::super::lex::is_code_offset;
    use super::*;
    use crate::Language;

    #[test]
    fn try_parse_raw_string_start_variants() {
        assert_eq!(
            try_parse_raw_string_start(b"r\"hello\"", 0, 8),
            Some((0, 2))
        );
        assert_eq!(
            try_parse_raw_string_start(b"r##\"content\"##", 0, 14),
            Some((2, 4))
        );
        assert_eq!(try_parse_raw_string_start(b"regular", 0, 7), None);

        let src = r##"let s = r#"inner"#;"##;
        assert!(!is_code_offset(src, 13, Language::Rust));
        assert!(is_code_offset(src, src.len() - 1, Language::Rust));
    }

    #[test]
    fn try_parse_char_literal_variants() {
        assert_eq!(try_parse_char_literal(b"'a'", 0, 3), Some(3));
        assert_eq!(try_parse_char_literal(b"'\\n'", 0, 4), Some(4));
        assert_eq!(try_parse_char_literal(b"x", 0, 1), None);
    }

    #[test]
    fn step_state_variants_are_covered() {
        let mut state = LexState::default();
        let bytes = b"r##\"hello\"##";
        assert_eq!(step_raw_string_state(&mut state, bytes, 0, 12, 2), 1);
        assert_eq!(step_raw_string_state(&mut state, bytes, 9, 12, 2), 3);
        assert_eq!(state.string_state, StringState::None);

        let mut state2 = LexState {
            string_state: StringState::Single,
            ..LexState::default()
        };
        let consumed = step_rust_code_state(&mut state2, b"'x'", 0, 3);
        assert_eq!(consumed, 3);
    }
}
