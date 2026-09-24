use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LangFilter {
    Python,
    Rust,
}

impl LangFilter {
    pub(crate) fn from_language(language: kiss::Language) -> Self {
        match language {
            kiss::Language::Python => Self::Python,
            kiss::Language::Rust => Self::Rust,
        }
    }

    pub(crate) fn to_language(self) -> kiss::Language {
        match self {
            Self::Python => kiss::Language::Python,
            Self::Rust => kiss::Language::Rust,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GitFocus {
    Commit,
    AutomaticBase,
    ExplicitBase { branch: String },
    DefaultMain,
    ConfiguredMain { name: String },
    ExplicitMain { branch: String },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct OperandExpr {
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TargetFocus {
    Workspace,
    Git(GitFocus),
    Operands(Vec<OperandExpr>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetRequest {
    pub focus: TargetFocus,
    pub lang: Option<LangFilter>,
    pub ignore: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompatKind {
    Workspace,
    Commit,
    Base,
    Main,
    Operands,
}

impl Default for TargetRequest {
    fn default() -> Self {
        Self {
            focus: TargetFocus::Workspace,
            lang: None,
            ignore: Vec::new(),
        }
    }
}

impl TargetRequest {
    #[cfg(test)]
    pub(crate) fn with_lang(mut self, lang: Option<kiss::Language>) -> Self {
        self.lang = lang.map(LangFilter::from_language);
        self
    }

    pub(crate) fn language(&self) -> Option<kiss::Language> {
        self.lang.map(LangFilter::to_language)
    }

    pub(crate) fn compat_kind(&self) -> CompatKind {
        match &self.focus {
            TargetFocus::Workspace => CompatKind::Workspace,
            TargetFocus::Git(GitFocus::Commit) => CompatKind::Commit,
            TargetFocus::Git(GitFocus::AutomaticBase | GitFocus::ExplicitBase { .. }) => {
                CompatKind::Base
            }
            TargetFocus::Git(
                GitFocus::DefaultMain
                | GitFocus::ConfiguredMain { .. }
                | GitFocus::ExplicitMain { .. },
            ) => CompatKind::Main,
            TargetFocus::Operands(_) => CompatKind::Operands,
        }
    }
}
