//! Data model and dispatch boundary for AST-aware symbol move (Task 1).
//!
//! These types are the canonical AST surface area consumed by `definition.rs`,
//! `reference.rs`, and `edits.rs`. The parser path produces `Definition` and
//! `Reference` records from a parsed file; on parse failure callers fall back
//! to the existing lexical scanners (see `lex.rs`, `signature.rs`).

use crate::Language;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SymbolKind {
    Function,
    Method,
}

#[derive(Clone, Debug)]
pub(super) struct Definition {
    pub name: String,
    pub owner: Option<String>,
    pub kind: SymbolKind,
    pub start: usize,
    pub end: usize,
    pub name_start: usize,
    pub name_end: usize,
    pub language: Language,
}

impl Definition {
    pub(super) fn assert_consistent(&self) {
        assert!(self.start <= self.end, "definition span must be ordered");
        assert!(
            self.name_start >= self.start && self.name_end <= self.end,
            "name span must lie within definition span"
        );
        assert!(
            self.name_end >= self.name_start,
            "name span must be ordered"
        );
        match (self.kind, self.owner.as_ref()) {
            (SymbolKind::Method, Some(_)) | (SymbolKind::Function, None) => {}
            _ => panic!("definition kind/owner mismatch"),
        }
        let _ = self.language;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ReferenceKind {
    Call,
    Import,
    Method,
}

#[derive(Clone, Debug)]
pub(super) struct Reference {
    pub start: usize,
    pub end: usize,
    pub kind: ReferenceKind,
}

#[derive(Clone, Debug)]
pub(super) struct AstResult {
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
}

#[derive(Clone, Debug)]
pub(super) enum ParseOutcome {
    Success(AstResult),
    Fail(FallbackReason),
}

#[derive(Clone, Debug)]
pub(super) enum FallbackReason {
    ParseFailed,
    ParserUnavailable,
}

impl AstResult {
    pub(super) fn matching_definition(
        &self,
        name: &str,
        owner: Option<&str>,
    ) -> Option<&Definition> {
        let hit = self
            .definitions
            .iter()
            .find(|d| d.name == name && d.owner.as_deref() == owner)?;
        hit.assert_consistent();
        Some(hit)
    }
}

#[cfg(test)]
mod ast_models_coverage {
    use super::*;

    #[test]
    fn matching_definition_filters_by_owner() {
        let res = AstResult {
            definitions: vec![
                Definition {
                    name: "f".into(),
                    owner: None,
                    kind: SymbolKind::Function,
                    start: 0,
                    end: 1,
                    name_start: 0,
                    name_end: 1,
                    language: Language::Python,
                },
                Definition {
                    name: "f".into(),
                    owner: Some("C".into()),
                    kind: SymbolKind::Method,
                    start: 2,
                    end: 3,
                    name_start: 2,
                    name_end: 3,
                    language: Language::Python,
                },
            ],
            references: vec![Reference {
                start: 0,
                end: 1,
                kind: ReferenceKind::Call,
            }],
        };
        let bare = res.matching_definition("f", None).unwrap();
        assert!(matches!(bare.kind, SymbolKind::Function));
        let owned = res.matching_definition("f", Some("C")).unwrap();
        assert_eq!(owned.start, 2);
        assert!(res.matching_definition("g", None).is_none());
        let _ = ParseOutcome::Fail(FallbackReason::ParserUnavailable);
        let _ = ParseOutcome::Fail(FallbackReason::ParseFailed);
        let _ = ParseOutcome::Success(res);
    }
}
