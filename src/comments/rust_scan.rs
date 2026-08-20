use crate::rust_parsing::ParsedRustFile;
use crate::violation::Violation;

use super::{clap_docs, comment_violation, doc_violation};

#[derive(Clone, Copy)]
enum ScanMode {
    Plain,
    Doc,
}

pub(super) fn append_rust_comment_violations(parsed: &ParsedRustFile, out: &mut Vec<Violation>) {
    append_rust_comment_kind(parsed, out, ScanMode::Plain);
}

pub(super) fn append_rust_doc_violations(parsed: &ParsedRustFile, out: &mut Vec<Violation>) {
    append_rust_comment_kind(parsed, out, ScanMode::Doc);
}

fn append_rust_comment_kind(parsed: &ParsedRustFile, out: &mut Vec<Violation>, mode: ScanMode) {
    let bytes = parsed.source.as_bytes();
    let clap_help = clap_docs::help_doc_ranges(&parsed.ast);
    let mut i = 0;
    while i < bytes.len() {
        if let Some((is_doc, next)) = take_line_comment(bytes, i) {
            push_kind(parsed, out, mode, is_doc, i, &clap_help);
            i = next;
            continue;
        }
        if let Some((is_doc, next)) = take_block_comment(bytes, i) {
            push_kind(parsed, out, mode, is_doc, i, &clap_help);
            i = next;
            continue;
        }
        if matches!(mode, ScanMode::Doc)
            && let Some(next) = take_doc_attribute(bytes, i)
        {
            push_kind(parsed, out, mode, true, i, &clap_help);
            i = next;
            continue;
        }
        if let Some(next) = skip_string_or_char(bytes, i) {
            i = next;
            continue;
        }
        i += 1;
    }
}

fn take_doc_attribute(bytes: &[u8], i: usize) -> Option<usize> {
    let rest = bytes.get(i..)?;
    let start = if rest.starts_with(b"#![doc") {
        i + 6
    } else if rest.starts_with(b"#[doc") {
        i + 5
    } else {
        return None;
    };
    skip_attr_to_bracket_end(bytes, start)
}

fn skip_attr_to_bracket_end(bytes: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 1;
    while i < bytes.len() && depth > 0 {
        if bytes[i] == b'"' {
            i = skip_double_string(bytes, i);
            continue;
        }
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    (depth == 0).then_some(i)
}

fn push_kind(
    parsed: &ParsedRustFile,
    out: &mut Vec<Violation>,
    mode: ScanMode,
    is_doc: bool,
    byte_idx: usize,
    clap_help: &[(usize, usize)],
) {
    let want_doc = matches!(mode, ScanMode::Doc);
    if is_doc != want_doc {
        return;
    }
    if want_doc && clap_docs::is_help_doc(clap_help, byte_idx) {
        return;
    }
    let line = line_number(&parsed.source, byte_idx);
    out.push(if want_doc {
        doc_violation(&parsed.path, line)
    } else {
        comment_violation(&parsed.path, line)
    });
}

fn take_line_comment(bytes: &[u8], i: usize) -> Option<(bool, usize)> {
    if i + 1 >= bytes.len() || bytes[i] != b'/' || bytes[i + 1] != b'/' {
        return None;
    }
    Some((is_doc_line_comment(bytes, i), skip_to_eol(bytes, i + 2)))
}

fn take_block_comment(bytes: &[u8], i: usize) -> Option<(bool, usize)> {
    if i + 1 >= bytes.len() || bytes[i] != b'/' || bytes[i + 1] != b'*' {
        return None;
    }
    Some((is_doc_block_comment(bytes, i), skip_nested_block(bytes, i)))
}

fn is_doc_line_comment(bytes: &[u8], i: usize) -> bool {
    let third = bytes.get(i + 2).copied();
    match third {
        Some(b'!') => true,
        Some(b'/') => bytes.get(i + 3).copied() != Some(b'/'),
        _ => false,
    }
}

fn is_doc_block_comment(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i + 2).copied() {
        Some(b'!') => true,
        Some(b'*') => {
            let fourth = bytes.get(i + 3).copied();
            fourth != Some(b'*') && fourth != Some(b'/')
        }
        _ => false,
    }
}

fn skip_to_eol(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    if i < bytes.len() { i + 1 } else { i }
}

fn skip_nested_block(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    let mut depth = 1;
    while i + 1 < bytes.len() && depth > 0 {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

fn skip_string_or_char(bytes: &[u8], i: usize) -> Option<usize> {
    if let Some((hashes, consumed)) = raw_string_start(bytes, i) {
        return Some(skip_raw_string(bytes, i + consumed, hashes));
    }
    if bytes[i] == b'"' {
        return Some(skip_double_string(bytes, i));
    }
    try_char_literal(bytes, i)
}

fn raw_string_start(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    if bytes[i] != b'r' {
        return None;
    }
    let mut hashes = 0;
    let mut check = i + 1;
    while check < bytes.len() && bytes[check] == b'#' {
        hashes += 1;
        check += 1;
    }
    (check < bytes.len() && bytes[check] == b'"').then_some((hashes, 2 + hashes))
}

fn skip_raw_string(bytes: &[u8], mut i: usize, hashes: usize) -> usize {
    while i < bytes.len() {
        if bytes[i] == b'"' && following_hashes(bytes, i + 1, hashes) {
            return i + 1 + hashes;
        }
        i += 1;
    }
    i
}

fn following_hashes(bytes: &[u8], start: usize, hashes: usize) -> bool {
    bytes
        .get(start..start.saturating_add(hashes))
        .is_some_and(|slice| slice.iter().all(|&b| b == b'#'))
}

fn skip_double_string(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return i + 1;
        }
        i += 1;
    }
    i
}

fn try_char_literal(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes[i] != b'\'' || i + 1 >= bytes.len() {
        return None;
    }
    if bytes[i + 1] == b'\\' {
        let mut end = i + 2;
        while end < bytes.len() && bytes[end] != b'\'' {
            end += 1;
        }
        return (end < bytes.len()).then_some(end + 1);
    }
    (i + 2 < bytes.len() && bytes[i + 2] == b'\'').then_some(3 + i)
}

fn line_number(source: &str, byte_idx: usize) -> usize {
    source.as_bytes()[..byte_idx.min(source.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}
