pub(super) fn find_identifier_occurrences(
    content: &str,
    ident: &str,
) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(found) = content[search_from..].find(ident) {
        let start = search_from + found;
        let end = start + ident.len();
        let left_ok = start == 0 || !is_ident_char(content.as_bytes()[start - 1] as char);
        let right_ok = end == content.len() || !is_ident_char(content.as_bytes()[end] as char);
        if left_ok && right_ok {
            out.push((start, end, line_for_offset(content, start)));
        }
        search_from = end;
    }
    out
}

pub(super) const fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

pub(super) fn line_for_offset(content: &str, offset: usize) -> usize {
    content[..offset].chars().filter(|&c| c == '\n').count() + 1
}

pub(super) fn line_start_offset(content: &str, offset: usize) -> usize {
    content[..offset].rfind('\n').map_or(0, |idx| idx + 1)
}

pub(super) fn previous_line_bounds(content: &str, line_start: usize) -> Option<(usize, usize)> {
    if line_start == 0 {
        return None;
    }
    let prev_end = line_start - 1;
    let prev_start = content[..prev_end].rfind('\n').map_or(0, |idx| idx + 1);
    Some((prev_start, prev_end))
}

#[cfg(test)]
mod identifiers_coverage {
    use super::*;

    #[test]
    fn identifier_helpers_smoke() {
        let s = "foo bar foo";
        assert_eq!(find_identifier_occurrences(s, "foo").len(), 2);
        assert!(is_ident_char('_'));
        assert!(!is_ident_char(' '));
        assert_eq!(line_for_offset("a\nb\nc", 4), 3);
        assert_eq!(line_start_offset("hello\nworld", 6), 6);
        assert_eq!(previous_line_bounds("hello\nworld", 6), Some((0, 5)));
        assert!(previous_line_bounds("x", 0).is_none());
    }
}
