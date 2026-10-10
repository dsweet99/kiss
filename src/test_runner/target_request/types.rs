#![cfg_attr(not(test), allow(dead_code))]
use serde::{Deserialize, Serialize};

pub(crate) type LangFilter = kiss::Language;

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
    pub(crate) fn set_language(&mut self, lang: Option<kiss::Language>) {
        self.lang = lang;
    }

    pub(crate) fn language(&self) -> Option<kiss::Language> {
        self.lang
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
