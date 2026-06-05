use syn::spanned::Spanned;

pub(super) fn compute_line_offsets(content: &str) -> Vec<usize> {
    let mut offsets = vec![0_usize];
    for (idx, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(idx + 1);
        }
    }
    offsets
}

pub(super) fn lc_to_byte(
    content: &str,
    line_offsets: &[usize],
    line: usize,
    column: usize,
) -> Option<usize> {
    assert!(line >= 1, "syn line numbers are 1-indexed");
    let row = line.checked_sub(1)?;
    let line_start = *line_offsets.get(row)?;
    let line_end = line_offsets.get(row + 1).copied().unwrap_or(content.len());
    let line_text = &content[line_start..line_end];
    let mut byte_in_line = 0_usize;
    for (chars_seen, ch) in line_text.chars().enumerate() {
        if chars_seen == column {
            return Some(line_start + byte_in_line);
        }
        byte_in_line += ch.len_utf8();
    }
    Some(line_start + byte_in_line)
}

pub(super) fn ident_byte_span(
    line_offsets: &[usize],
    ident: &syn::Ident,
    content: &str,
) -> Option<(usize, usize)> {
    let span = ident.span();
    let start_lc = span.start();
    let start = lc_to_byte(content, line_offsets, start_lc.line, start_lc.column)?;
    let name = ident.to_string();
    let end = start + name.len();
    if end <= content.len() && content.is_char_boundary(start) && content[start..end] == name {
        return Some((start, end));
    }
    None
}

pub(super) fn item_full_span<T: Spanned>(
    item: &T,
    content: &str,
    line_offsets: &[usize],
) -> Option<(usize, usize)> {
    let span = item.span();
    let start_lc = span.start();
    let end_lc = span.end();
    let start = lc_to_byte(content, line_offsets, start_lc.line, start_lc.column)?;
    let end = lc_to_byte(content, line_offsets, end_lc.line, end_lc.column)?;
    if end > start && end <= content.len() {
        Some((start, end))
    } else {
        None
    }
}
