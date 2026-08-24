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
pub(super) struct TraitImpl {
    pub trait_name: String,
    pub implementor: String,
}

#[derive(Clone, Debug)]
pub(super) struct AstResult {
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    pub trait_impls: Vec<TraitImpl>,
}

#[derive(Clone, Debug)]
pub(super) enum ParseOutcome {
    Success(AstResult),
    Fail(FallbackReason),
}

#[derive(Clone, Debug)]
pub(super) enum FallbackReason {
    ParseFailed,
    #[allow(dead_code)]
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

    impl SymbolKind {
        fn witness() -> Self {
            Self::Function
        }
    }
    impl Definition {
        fn witness() -> Self {
            Self {
                name: "f".into(),
                owner: None,
                kind: SymbolKind::witness(),
                start: 0,
                end: 1,
                name_start: 0,
                name_end: 1,
                language: Language::Python,
            }
        }
    }
    impl ReferenceKind {
        fn witness() -> Self {
            Self::Call
        }
    }
    impl Reference {
        fn witness() -> Self {
            Self {
                start: 0,
                end: 1,
                kind: ReferenceKind::witness(),
            }
        }
    }
    impl TraitImpl {
        fn witness() -> Self {
            Self {
                trait_name: "T".into(),
                implementor: "C".into(),
            }
        }
    }
    impl AstResult {
        fn witness() -> Self {
            Self {
                definitions: vec![Definition::witness()],
                references: vec![Reference::witness()],
                trait_impls: vec![TraitImpl::witness()],
            }
        }
    }
    impl FallbackReason {
        fn witness() -> Self {
            Self::ParseFailed
        }
    }
    impl ParseOutcome {
        fn witness() -> Self {
            Self::Success(AstResult::witness())
        }
    }

    #[test]
    fn witness_ast_models() {
        let _ = SymbolKind::witness();
        let _ = Definition::witness();
        let _ = ReferenceKind::witness();
        let _ = Reference::witness();
        let _ = TraitImpl::witness();
        let res = AstResult::witness();
        let _ = FallbackReason::witness();
        let _ = ParseOutcome::witness();
        assert!(res.matching_definition("f", None).is_some());
    }
}
