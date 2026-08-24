use super::span::SourceSpan;
use super::types::{CodeContextSet, FileComposition};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRange {
    pub span: SourceSpan,
    pub contexts: CodeContextSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRoleFacts {
    pub base_contexts: CodeContextSet,
    pub ranges: Vec<RoleRange>,
    pub line_ranges: Vec<LineRoleRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRoleRange {
    pub start_line: usize,
    pub end_line: usize,
    pub contexts: CodeContextSet,
}

impl FileRoleFacts {
    #[must_use]
    pub fn new(base_contexts: CodeContextSet, ranges: Vec<RoleRange>) -> Self {
        let line_ranges = project_line_ranges(base_contexts, &ranges);
        Self {
            base_contexts,
            ranges,
            line_ranges,
        }
    }

    #[must_use]
    pub fn composition(&self) -> FileComposition {
        composition_from_facts(self.base_contexts, &self.ranges)
    }
}

pub(crate) fn composition_from_facts(
    base: CodeContextSet,
    ranges: &[RoleRange],
) -> FileComposition {
    let has_test_only = (base.is_test_only() && ranges.is_empty())
        || ranges.iter().any(|r| r.contexts.is_test_only());
    let has_production = base.production || ranges.iter().any(|r| r.contexts.production);
    match (has_test_only, has_production) {
        (true, true) => FileComposition::Mixed,
        (true, false) => FileComposition::TestOnly,
        _ => FileComposition::ProductionOnly,
    }
}

fn project_line_ranges(base: CodeContextSet, ranges: &[RoleRange]) -> Vec<LineRoleRange> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let max_line = ranges
        .iter()
        .map(|r| r.span.end.line.max(r.span.start.line))
        .max()
        .unwrap_or(1);
    let mut per_line: Vec<Option<CodeContextSet>> = vec![None; max_line + 1];
    for range in ranges {
        for (line, slot) in per_line.iter_mut().enumerate().skip(1) {
            if range.span.touches_line(line) {
                *slot = Some(slot.unwrap_or(CodeContextSet::none()).union(range.contexts));
            }
        }
    }
    merge_line_slots(base, &per_line)
}

fn merge_line_slots(
    base: CodeContextSet,
    per_line: &[Option<CodeContextSet>],
) -> Vec<LineRoleRange> {
    let mut out = Vec::new();
    let mut current: Option<LineRoleRange> = None;
    for (line, slot) in per_line.iter().enumerate().skip(1) {
        let ctx = slot.unwrap_or(base);
        match current {
            Some(mut cur) if cur.contexts == ctx && cur.end_line + 1 == line => {
                cur.end_line = line;
                current = Some(cur);
            }
            Some(cur) => {
                out.push(cur);
                current = Some(LineRoleRange {
                    start_line: line,
                    end_line: line,
                    contexts: ctx,
                });
            }
            None => {
                current = Some(LineRoleRange {
                    start_line: line,
                    end_line: line,
                    contexts: ctx,
                });
            }
        }
    }
    if let Some(cur) = current {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod facts_test {
    use super::*;
    use crate::code_roles::span::{SourcePosition, SourceSpan};
    use crate::code_roles::types::CodeRole;

    #[test]
    fn mixed_file_composition() {
        let prod = RoleRange {
            span: SourceSpan::new(SourcePosition::new(1, 0), SourcePosition::new(2, 0)),
            contexts: CodeContextSet::production_only(),
        };
        let test = RoleRange {
            span: SourceSpan::new(SourcePosition::new(3, 0), SourcePosition::new(4, 0)),
            contexts: CodeContextSet::test_only(),
        };
        let facts = FileRoleFacts::new(CodeContextSet::production_only(), vec![prod, test]);
        assert_eq!(facts.composition(), FileComposition::Mixed);
        assert_eq!(facts.line_ranges[0].contexts.role(), CodeRole::Production);
    }
}
