use syn::spanned::Spanned;
use tree_sitter::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
}

impl SourcePosition {
    #[must_use]
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    #[must_use]
    pub fn from_syn(span: proc_macro2::Span) -> Self {
        let loc = span.start();
        Self {
            line: loc.line,
            column: loc.column,
        }
    }

    #[must_use]
    pub fn from_syn_end(span: proc_macro2::Span) -> Self {
        let loc = span.end();
        Self {
            line: loc.line,
            column: loc.column,
        }
    }

    #[must_use]
    pub const fn from_tree_sitter(point: Point) -> Self {
        Self {
            line: point.row + 1,
            column: point.column,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceSpan {
    #[must_use]
    pub fn new(start: SourcePosition, end: SourcePosition) -> Self {
        if end < start {
            Self { start, end: start }
        } else {
            Self { start, end }
        }
    }

    #[must_use]
    pub fn from_syn_span(span: proc_macro2::Span) -> Self {
        Self::new(
            SourcePosition::from_syn(span),
            SourcePosition::from_syn_end(span),
        )
    }

    #[must_use]
    pub fn of_syn(node: &impl Spanned) -> Self {
        Self::from_syn_span(node.span())
    }

    #[must_use]
    pub fn from_tree_sitter_node(node: tree_sitter::Node<'_>) -> Self {
        Self::new(
            SourcePosition::from_tree_sitter(node.start_position()),
            SourcePosition::from_tree_sitter(node.end_position()),
        )
    }

    #[must_use]
    pub fn whole_file(source: &str) -> Self {
        let line_count = source.lines().count();
        if line_count == 0 {
            return Self::new(SourcePosition::new(1, 0), SourcePosition::new(1, 0));
        }
        let last = source.lines().last().map_or(0, str::len);
        Self::new(
            SourcePosition::new(1, 0),
            SourcePosition::new(line_count, last),
        )
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }

    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    #[must_use]
    pub fn contains_pos(self, pos: SourcePosition) -> bool {
        self.start <= pos && pos < self.end
    }

    #[must_use]
    pub fn slice_source(self, source: &str) -> String {
        let start = byte_index_at(source, self.start);
        let end = byte_index_at(source, self.end).max(start);
        source.get(start..end).unwrap_or("").to_string()
    }

    #[must_use]
    pub fn touches_line(self, line: usize) -> bool {
        if self.is_empty() {
            return false;
        }
        let last_line = if self.end.column == 0 && self.end.line > self.start.line {
            self.end.line.saturating_sub(1)
        } else {
            self.end.line
        };
        self.start.line <= line && line <= last_line
    }
}

fn byte_index_at(source: &str, pos: SourcePosition) -> usize {
    let mut line = 1usize;
    let mut col = 0usize;
    for (i, ch) in source.char_indices() {
        if line > pos.line || (line == pos.line && col >= pos.column) {
            return i;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf8();
        }
    }
    source.len()
}

#[cfg(test)]
mod span_test {
    use super::*;

    #[test]
    fn whole_file_and_line_touch() {
        let span = SourceSpan::whole_file("a\nb\n");
        assert!(span.touches_line(1));
        assert!(span.touches_line(2));
        let empty = SourceSpan::whole_file("");
        assert!(empty.is_empty() || empty.start.line == 1);
    }

    #[test]
    fn overlap_half_open() {
        let a = SourceSpan::new(SourcePosition::new(1, 0), SourcePosition::new(1, 4));
        let b = SourceSpan::new(SourcePosition::new(1, 4), SourcePosition::new(1, 8));
        assert!(!a.overlaps(b));
        let c = SourceSpan::new(SourcePosition::new(1, 3), SourcePosition::new(1, 5));
        assert!(a.overlaps(c));
        assert!(a.contains_pos(SourcePosition::new(1, 3)));
    }
}
