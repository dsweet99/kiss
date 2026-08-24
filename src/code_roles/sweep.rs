use super::facts::RoleRange;
use super::span::{SourcePosition, SourceSpan};
use super::types::CodeContextSet;

#[must_use]
pub fn normalize_ranges(mut ranges: Vec<RoleRange>) -> Vec<RoleRange> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|r| (r.span.start, r.span.end));
    let bounds = collect_bounds(&ranges);
    if bounds.len() < 2 {
        return ranges;
    }
    emit_atomic_intervals(&ranges, &bounds)
}

fn collect_bounds(ranges: &[RoleRange]) -> Vec<SourcePosition> {
    let mut bounds = Vec::with_capacity(ranges.len() * 2);
    for range in ranges {
        bounds.push(range.span.start);
        bounds.push(range.span.end);
    }
    bounds.sort();
    bounds.dedup();
    bounds
}

fn emit_atomic_intervals(ranges: &[RoleRange], bounds: &[SourcePosition]) -> Vec<RoleRange> {
    let mut out = Vec::new();
    for window in bounds.windows(2) {
        let span = SourceSpan::new(window[0], window[1]);
        if span.is_empty() {
            continue;
        }
        let Some(contexts) = covering_contexts(ranges, span) else {
            continue;
        };
        push_merged(&mut out, RoleRange { span, contexts });
    }
    out
}

fn covering_contexts(ranges: &[RoleRange], span: SourceSpan) -> Option<CodeContextSet> {
    let mut covering: Vec<&RoleRange> = ranges
        .iter()
        .filter(|r| r.span.start <= span.start && span.end <= r.span.end)
        .collect();
    if covering.is_empty() {
        return None;
    }
    covering.sort_by_key(|r| span_area(r.span));
    let min_area = span_area(covering[0].span);
    let mut contexts = CodeContextSet::none();
    for range in covering {
        if span_area(range.span) == min_area {
            contexts = contexts.union(range.contexts);
        }
    }
    Some(contexts)
}

const fn span_area(span: SourceSpan) -> (usize, usize) {
    (
        span.end.line.saturating_sub(span.start.line),
        span.end
            .column
            .saturating_add(1000)
            .saturating_sub(span.start.column),
    )
}

fn push_merged(out: &mut Vec<RoleRange>, next: RoleRange) {
    if let Some(last) = out.last_mut()
        && last.contexts == next.contexts
        && last.span.end == next.span.start
    {
        last.span.end = next.span.end;
        return;
    }
    out.push(next);
}

#[cfg(test)]
mod sweep_test {
    use super::*;
    use crate::code_roles::types::CodeRole;

    fn range(start_line: usize, end_line: usize, ctx: CodeContextSet) -> RoleRange {
        RoleRange {
            span: SourceSpan::new(
                SourcePosition::new(start_line, 0),
                SourcePosition::new(end_line, 0),
            ),
            contexts: ctx,
        }
    }

    #[test]
    fn nested_child_replaces_parent() {
        let parent = range(1, 10, CodeContextSet::production_only());
        let child = range(3, 5, CodeContextSet::test_only());
        let out = normalize_ranges(vec![parent, child]);
        let mid = out
            .iter()
            .find(|r| r.span.start.line == 3)
            .expect("child range");
        assert_eq!(mid.contexts.role(), CodeRole::TestOnly);
        assert!(
            out.iter()
                .any(|r| r.span.start.line == 1 && r.contexts.production)
        );
    }
}
