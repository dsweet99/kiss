use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::target_request::{GitFocus, TargetFocus};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NudgeInvocation {
    #[default]
    All,
    Commit,
    Base,
    Main,
    Targets,
}

impl NudgeInvocation {
    #[cfg(test)]
    pub(crate) fn from_test(invocation: &TestInvocation) -> Self {
        match invocation {
            TestInvocation::Commit => Self::Commit,
            TestInvocation::Base => Self::Base,
            TestInvocation::Main => Self::Main,
            TestInvocation::All => Self::All,
            TestInvocation::Targets(_) => Self::Targets,
        }
    }

    pub(crate) fn from_focus(focus: &TargetFocus) -> Self {
        match focus {
            TargetFocus::Workspace => Self::All,
            TargetFocus::Git(GitFocus::Commit) => Self::Commit,
            TargetFocus::Git(GitFocus::AutomaticBase | GitFocus::ExplicitBase { .. }) => Self::Base,
            TargetFocus::Git(
                GitFocus::DefaultMain
                | GitFocus::ConfiguredMain { .. }
                | GitFocus::ExplicitMain { .. },
            ) => Self::Main,
            TargetFocus::Operands(_) => Self::Targets,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_all(self) -> bool {
        matches!(self, Self::All)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Commit => "commit",
            Self::Base => "base",
            Self::Main => "main",
            Self::Targets => "targets",
        }
    }
}
