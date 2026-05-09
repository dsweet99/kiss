//! Python f-string lexer support for `is_code_offset`.
//!
//! Treats the literal text portion of an f-string as "string-like" and the
//! code-bearing brace expressions (`{ … }`) as "code-like", so the
//! identifier-occurrence filter in `kiss mv` can rewrite references inside
//! `f"… {expr} …"` braces.

use super::lex::{LexState, StringState};

/// Detect a Python f-string opener at `idx` (covers `f`, `F`, `rf`, `fr`,
/// and case variants). On success: mutates `state` into the matching
/// `FString*` variant and returns the number of bytes consumed (prefix +
/// opening quote(s)). Returns `None` for non-f-string code.
pub(super) fn try_parse_python_fstring_start(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> Option<usize> {
    let (prefix_len, quote_idx) = parse_python_fstring_prefix(bytes, idx, target)?;
    let quote = *bytes.get(quote_idx)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let triple =
        quote_idx + 2 < target && bytes[quote_idx + 1] == quote && bytes[quote_idx + 2] == quote;
    state.string_state = match (quote, triple) {
        (b'\'', false) => StringState::FStringSingle { depth: 0 },
        (b'\'', true) => StringState::FStringTripleSingle { depth: 0 },
        (_, false) => StringState::FStringDouble { depth: 0 },
        (_, true) => StringState::FStringTripleDouble { depth: 0 },
    };
    Some(prefix_len + if triple { 3 } else { 1 })
}

/// Returns `(prefix_len, quote_idx)` if the bytes at `idx` start with a
/// Python f-string prefix (`f`, `F`, `rf`, `fr`, etc.). Rejects
/// identifier-internal positions (e.g., the `f` in `something_f"x"`).
fn parse_python_fstring_prefix(bytes: &[u8], idx: usize, target: usize) -> Option<(usize, usize)> {
    if idx > 0 {
        let prev = bytes[idx - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return None;
        }
    }
    let c0 = *bytes.get(idx)?;
    let c1 = bytes.get(idx + 1).copied();
    let prefix_len = match (c0, c1) {
        (b'f' | b'F', Some(b'r' | b'R')) | (b'r' | b'R', Some(b'f' | b'F')) => 2,
        (b'f' | b'F', _) => 1,
        _ => return None,
    };
    let quote_idx = idx + prefix_len;
    (quote_idx < target).then_some((prefix_len, quote_idx))
}

pub(super) fn step_fstring_state(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> usize {
    let (quote, triple, depth) = decode_fstring_state(state.string_state);
    if depth == 0 {
        step_fstring_text(state, bytes, idx, target, quote, triple)
    } else {
        step_fstring_code(state, bytes, idx, depth)
    }
}

const fn decode_fstring_state(s: StringState) -> (u8, bool, usize) {
    match s {
        StringState::FStringSingle { depth } => (b'\'', false, depth),
        StringState::FStringDouble { depth } => (b'"', false, depth),
        StringState::FStringTripleSingle { depth } => (b'\'', true, depth),
        StringState::FStringTripleDouble { depth } => (b'"', true, depth),
        _ => (0, false, 0),
    }
}

const fn set_fstring_depth(state: &mut LexState, new_depth: usize) {
    state.string_state = match state.string_state {
        StringState::FStringSingle { .. } => StringState::FStringSingle { depth: new_depth },
        StringState::FStringDouble { .. } => StringState::FStringDouble { depth: new_depth },
        StringState::FStringTripleSingle { .. } => {
            StringState::FStringTripleSingle { depth: new_depth }
        }
        StringState::FStringTripleDouble { .. } => {
            StringState::FStringTripleDouble { depth: new_depth }
        }
        other => other,
    };
}

fn step_fstring_text(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
    quote: u8,
    triple: bool,
) -> usize {
    let b = bytes[idx];
    if let Some(consumed) = matches_two_byte_text_escape(b, bytes, idx, target) {
        return consumed;
    }
    if b == b'{' {
        set_fstring_depth(state, 1);
        return 1;
    }
    close_fstring_text_quote(state, bytes, idx, target, quote, triple).unwrap_or(1)
}

/// Detect a two-byte escape inside f-string text: a backslash followed by
/// any byte (`\X`), or a doubled brace (`{{` / `}}`). Returns the number of
/// bytes to consume (always 2) when matched, or `None` otherwise.
const fn matches_two_byte_text_escape(
    b: u8,
    bytes: &[u8],
    idx: usize,
    target: usize,
) -> Option<usize> {
    if idx + 1 >= target {
        return None;
    }
    let next = bytes[idx + 1];
    let matched = (b == b'\\') || (b == b'{' && next == b'{') || (b == b'}' && next == b'}');
    if matched { Some(2) } else { None }
}

/// If the byte at `idx` is the f-string's closing quote, exit the f-string
/// state and return the number of bytes consumed (1 for single, 3 for
/// triple). Returns `None` if not a closing quote.
fn close_fstring_text_quote(
    state: &mut LexState,
    bytes: &[u8],
    idx: usize,
    target: usize,
    quote: u8,
    triple: bool,
) -> Option<usize> {
    if bytes[idx] != quote {
        return None;
    }
    if triple {
        let triple_close = idx + 2 < target && bytes[idx + 1] == quote && bytes[idx + 2] == quote;
        if !triple_close {
            return None;
        }
        state.string_state = StringState::None;
        Some(3)
    } else {
        state.string_state = StringState::None;
        Some(1)
    }
}

/// Step inside the code-bearing brace expression of an f-string. We only
/// track `{` / `}` to maintain the nesting depth so we know when the
/// expression closes. Identifier-occurrence filtering treats every byte
/// here as "code", which is exactly what we want for `kiss mv`.
fn step_fstring_code(state: &mut LexState, bytes: &[u8], idx: usize, depth: usize) -> usize {
    match bytes[idx] {
        b'{' => set_fstring_depth(state, depth + 1),
        b'}' => set_fstring_depth(state, depth - 1),
        _ => {}
    }
    1
}

#[cfg(test)]
mod lex_fstring_coverage_private {
    use super::*;

    #[test]
    fn touch_fstring_internals() {
        let src = r#"f"abc {x + y:{width}}"#;
        let idx = src.find('x').unwrap();
        let mut state = LexState {
            string_state: StringState::FStringSingle { depth: 0 },
            ..LexState::default()
        };
        assert_eq!(
            step_fstring_text(&mut state, src.as_bytes(), 0, src.len(), b'"', false),
            1
        );
        assert_eq!(step_fstring_code(&mut state, src.as_bytes(), idx, 0), 1);

        let mut state2 = LexState::default();
        assert_eq!(
            try_parse_python_fstring_start(&mut state2, src.as_bytes(), 0, src.len()),
            Some(2)
        );
        assert_eq!(
            parse_python_fstring_prefix(src.as_bytes(), 0, src.len()),
            Some((1, 1))
        );
        assert_eq!(
            step_fstring_state(
                &mut state2,
                src.as_bytes(),
                src.find('{').unwrap(),
                src.len()
            ),
            1
        );
        state2.string_state = StringState::FStringSingle { depth: 1 };
        assert_eq!(step_fstring_code(&mut state2, src.as_bytes(), idx, 1), 1);
    }
}

#[cfg(test)]
mod lex_fstring_coverage {
    use super::super::lex::is_code_offset;
    use crate::Language;

    #[test]
    fn fstring_brace_contents_are_code() {
        let src = "f\"value={helper()}\"";
        let helper_at = src.find("helper").unwrap();
        assert!(
            is_code_offset(src, helper_at, Language::Python),
            "byte inside f-string braces must be classified as code"
        );

        let value_at = src.find("value").unwrap();
        assert!(
            !is_code_offset(src, value_at, Language::Python),
            "byte in f-string literal text must remain string-like"
        );

        let after = "f\"x={a}\" + b";
        let b_at = after.rfind('b').unwrap();
        assert!(is_code_offset(after, b_at, Language::Python));
    }

    #[test]
    fn fstring_handles_prefix_and_quote_variants() {
        let s1 = "f'{x}'";
        assert!(is_code_offset(s1, s1.find('x').unwrap(), Language::Python));

        let s2 = "f\"\"\"a={x}\"\"\"";
        assert!(is_code_offset(s2, s2.find('x').unwrap(), Language::Python));

        let s3 = "F\"{y}\"";
        assert!(is_code_offset(s3, s3.find('y').unwrap(), Language::Python));

        let s4 = "rf\"{z}\"";
        assert!(is_code_offset(s4, s4.find('z').unwrap(), Language::Python));
        let s5 = "fr\"{w}\"";
        assert!(is_code_offset(s5, s5.find('w').unwrap(), Language::Python));
    }

    #[test]
    fn fstring_handles_escaped_braces_and_nesting() {
        let escaped = "f\"{{x}}={y}\"";
        let first_x = escaped.find('x').unwrap();
        assert!(
            !is_code_offset(escaped, first_x, Language::Python),
            "x inside `{{x}}` is literal text, not code"
        );
        let y_at = escaped.find('y').unwrap();
        assert!(is_code_offset(escaped, y_at, Language::Python));

        let nested = "f\"{x:{width}}\"";
        assert!(is_code_offset(
            nested,
            nested.find('x').unwrap(),
            Language::Python
        ));
        assert!(is_code_offset(
            nested,
            nested.find("width").unwrap(),
            Language::Python
        ));
        let trailing = "f\"{x:{w}}\" + tail";
        assert!(is_code_offset(
            trailing,
            trailing.find("tail").unwrap(),
            Language::Python
        ));
    }

    #[test]
    fn non_fstring_f_identifier_is_not_misparsed() {
        let src = "from a import b";
        assert!(is_code_offset(
            src,
            src.find('a').unwrap(),
            Language::Python
        ));

        let src2 = "helper_f\"x\"";
        assert!(!is_code_offset(
            src2,
            src2.find('x').unwrap(),
            Language::Python
        ));
    }

    #[test]
    fn fstring_triple_close_at_end_exits_state() {
        let src = "f\"\"\"abc\"\"\"";
        let after_end = src.len();
        assert!(is_code_offset(src, after_end, Language::Python));
    }
}
