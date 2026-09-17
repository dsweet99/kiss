use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::rust_include::canonical_path;

use super::facts::{FileRoleFacts, LineRoleRange, RoleRange};
use super::span::SourceSpan;
use super::types::{CodeContextSet, CodeRole, FileComposition};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceRoleIndex {
    files: BTreeMap<PathBuf, FileRoleFacts>,
}

impl SourceRoleIndex {
    #[must_use]
    pub fn new(files: BTreeMap<PathBuf, FileRoleFacts>) -> Self {
        Self { files }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, path: PathBuf, facts: FileRoleFacts) {
        self.files.insert(canonical_path(&path), facts);
    }

    pub fn merge_from(&mut self, other: Self) {
        for (path, facts) in other.files {
            self.files.insert(path, facts);
        }
    }

    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn role_at(&self, path: &Path, line: usize) -> CodeRole {
        let Some(facts) = facts_for(self, path) else {
            return CodeRole::Production;
        };
        line_contexts(&facts.line_ranges, facts.base_contexts, line).role()
    }

    #[must_use]
    pub fn role_for_span(&self, path: &Path, span: SourceSpan) -> CodeRole {
        let Some(facts) = facts_for(self, path) else {
            return CodeRole::Production;
        };
        if span.is_empty() {
            return facts.base_contexts.role();
        }
        span_contexts(&facts.ranges, facts.base_contexts, span).role()
    }

    #[must_use]
    pub fn file_composition(&self, path: &Path) -> FileComposition {
        facts_for(self, path).map_or(FileComposition::ProductionOnly, FileRoleFacts::composition)
    }

    #[must_use]
    pub fn production_lines(&self, path: &Path, candidate_lines: &[usize]) -> Vec<usize> {
        let Some(facts) = facts_for(self, path) else {
            return candidate_lines.to_vec();
        };
        merge_production_lines(&facts.line_ranges, facts.base_contexts, candidate_lines)
    }

    #[must_use]
    pub fn production_segments(&self, path: &Path, span: SourceSpan) -> Vec<SourceSpan> {
        let Some(facts) = facts_for(self, path) else {
            return vec![span];
        };
        production_segments_of(&facts.ranges, facts.base_contexts, span)
    }
}

#[must_use]
pub fn contains_file(roles: &SourceRoleIndex, path: &Path) -> bool {
    facts_for(roles, path).is_some()
}

#[must_use]
pub fn is_test_only_file(roles: &SourceRoleIndex, path: &Path) -> bool {
    roles.file_composition(path) == FileComposition::TestOnly
}

#[must_use]
pub fn contexts_at(roles: &SourceRoleIndex, path: &Path, line: usize) -> CodeContextSet {
    let Some(facts) = facts_for(roles, path) else {
        return CodeContextSet::production_only();
    };
    line_contexts(&facts.line_ranges, facts.base_contexts, line)
}

#[must_use]
pub fn contexts_for_span(roles: &SourceRoleIndex, path: &Path, span: SourceSpan) -> CodeContextSet {
    let Some(facts) = facts_for(roles, path) else {
        return CodeContextSet::production_only();
    };
    if span.is_empty() {
        return facts.base_contexts;
    }
    span_contexts(&facts.ranges, facts.base_contexts, span)
}

#[must_use]
pub fn production_line_count(roles: &SourceRoleIndex, path: &Path, source: &str) -> usize {
    let n = source.lines().count();
    if n == 0 {
        return 0;
    }
    match roles.file_composition(path) {
        FileComposition::ProductionOnly => n,
        FileComposition::TestOnly => 0,
        FileComposition::Mixed => {
            let candidates: Vec<usize> = (1..=n).collect();
            roles.production_lines(path, &candidates).len()
        }
    }
}

#[must_use]
pub fn skip_syn(
    roles: Option<&SourceRoleIndex>,
    path: &Path,
    node: &impl syn::spanned::Spanned,
) -> bool {
    roles.is_some_and(|roles| match roles.file_composition(path) {
        FileComposition::ProductionOnly => false,
        FileComposition::TestOnly => true,
        FileComposition::Mixed => {
            roles.role_for_span(path, SourceSpan::of_syn(node)) == CodeRole::TestOnly
        }
    })
}

fn facts_for<'a>(index: &'a SourceRoleIndex, path: &Path) -> Option<&'a FileRoleFacts> {
    index.files.get(path).or_else(|| {
        let canon = canonical_path(path);
        if canon.as_path() == path {
            None
        } else {
            index.files.get(&canon)
        }
    })
}

fn line_contexts(
    line_ranges: &[LineRoleRange],
    base: CodeContextSet,
    line: usize,
) -> CodeContextSet {
    for range in line_ranges {
        if range.start_line <= line && line <= range.end_line {
            return range.contexts;
        }
    }
    base
}

fn span_contexts(ranges: &[RoleRange], base: CodeContextSet, span: SourceSpan) -> CodeContextSet {
    let mut matched = false;
    let mut contexts = CodeContextSet::none();
    for range in ranges {
        if range.span.overlaps(span) {
            matched = true;
            contexts = contexts.union(range.contexts);
        }
    }
    if matched { contexts } else { base }
}

fn merge_production_lines(
    line_ranges: &[LineRoleRange],
    base: CodeContextSet,
    candidates: &[usize],
) -> Vec<usize> {
    let mut i = 0;
    let mut out = Vec::new();
    for &line in candidates {
        while i < line_ranges.len() && line_ranges[i].end_line < line {
            i += 1;
        }
        let production = if i < line_ranges.len()
            && line_ranges[i].start_line <= line
            && line <= line_ranges[i].end_line
        {
            line_ranges[i].contexts.production
        } else {
            base.production
        };
        if production {
            out.push(line);
        }
    }
    out
}

fn production_segments_of(
    ranges: &[RoleRange],
    base: CodeContextSet,
    span: SourceSpan,
) -> Vec<SourceSpan> {
    if ranges.is_empty() {
        return if base.production {
            vec![span]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    let mut overlapped = false;
    for range in ranges {
        if !range.span.overlaps(span) {
            continue;
        }
        overlapped = true;
        if range.contexts.production {
            let start = range.span.start.max(span.start);
            let end = min_pos(range.span.end, span.end);
            if start < end {
                out.push(SourceSpan::new(start, end));
            }
        }
    }
    if !overlapped && base.production {
        out.push(span);
    }
    merge_spans(out)
}

fn merge_spans(mut spans: Vec<SourceSpan>) -> Vec<SourceSpan> {
    if spans.len() < 2 {
        return spans;
    }
    spans.sort_by_key(|s| (s.start, s.end));
    let mut out: Vec<SourceSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = out.last_mut()
            && last.end >= span.start
        {
            if last.end < span.end {
                last.end = span.end;
            }
        } else {
            out.push(span);
        }
    }
    out
}

fn min_pos(
    a: super::span::SourcePosition,
    b: super::span::SourcePosition,
) -> super::span::SourcePosition {
    if a < b { a } else { b }
}

#[cfg(test)]
mod index_test {
    use super::*;
    use crate::code_roles::facts::RoleRange;
    use crate::code_roles::span::{SourcePosition, SourceSpan};
    use std::path::Path;

    #[test]
    fn missing_path_is_production() {
        let index = SourceRoleIndex::empty();
        let path = Path::new("missing.rs");
        assert_eq!(index.role_at(path, 1), CodeRole::Production);
        assert!(!contains_file(&index, path));
        assert_eq!(
            index.file_composition(path),
            FileComposition::ProductionOnly
        );
        assert_eq!(index.production_lines(path, &[1, 2]), vec![1, 2]);
        assert_eq!(
            contexts_at(&index, path, 1),
            CodeContextSet::production_only()
        );
        assert_eq!(
            contexts_for_span(&index, path, SourceSpan::whole_file("")),
            CodeContextSet::production_only()
        );
    }

    #[test]
    fn span_query_distinguishes_mixed_line() {
        let prod = RoleRange {
            span: SourceSpan::new(SourcePosition::new(1, 0), SourcePosition::new(1, 8)),
            contexts: CodeContextSet::production_only(),
        };
        let test = RoleRange {
            span: SourceSpan::new(SourcePosition::new(1, 8), SourcePosition::new(1, 20)),
            contexts: CodeContextSet::test_only(),
        };
        let mut files = BTreeMap::new();
        files.insert(
            PathBuf::from("mixed.rs"),
            FileRoleFacts::new(CodeContextSet::production_only(), vec![prod, test]),
        );
        let index = SourceRoleIndex::new(files);
        assert!(contains_file(&index, Path::new("mixed.rs")));
        assert_eq!(
            index.role_for_span(Path::new("mixed.rs"), prod.span),
            CodeRole::Production
        );
        assert_eq!(
            index.role_for_span(Path::new("mixed.rs"), test.span),
            CodeRole::TestOnly
        );
        assert_eq!(
            index.role_at(Path::new("mixed.rs"), 1),
            CodeRole::Production
        );
    }

    #[test]
    fn production_segments_omit_test_only_hole() {
        let parent = RoleRange {
            span: SourceSpan::new(SourcePosition::new(1, 0), SourcePosition::new(10, 0)),
            contexts: CodeContextSet::production_only(),
        };
        let hole = RoleRange {
            span: SourceSpan::new(SourcePosition::new(3, 0), SourcePosition::new(5, 0)),
            contexts: CodeContextSet::test_only(),
        };
        let facts = FileRoleFacts::new(
            CodeContextSet::production_only(),
            crate::code_roles::sweep::normalize_ranges(vec![parent, hole]),
        );
        let mut files = BTreeMap::new();
        files.insert(PathBuf::from("mixed.rs"), facts);
        let index = SourceRoleIndex::new(files);
        let segs = index.production_segments(Path::new("mixed.rs"), parent.span);
        assert!(
            segs.iter()
                .all(|s| s.end <= hole.span.start || s.start >= hole.span.end),
            "test-only hole must be absent from production segments, got {segs:?}"
        );
        assert!(
            !segs.is_empty(),
            "production sides of the mixed span remain"
        );
    }
}
