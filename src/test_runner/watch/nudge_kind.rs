use serde::{Deserialize, Serialize};

use crate::bin_cli::args::TestInvocation;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NudgeInvocation {
    #[default]
    All,
    Commit,
    Base,
    Main,
}

impl NudgeInvocation {
    pub(crate) fn from_test(invocation: &TestInvocation) -> Self {
        match invocation {
            TestInvocation::Commit => Self::Commit,
            TestInvocation::Base => Self::Base,
            TestInvocation::Main => Self::Main,
            TestInvocation::All | TestInvocation::Targets(_) => Self::All,
        }
    }

    pub(crate) fn to_test(self) -> Option<TestInvocation> {
        match self {
            Self::All => None,
            Self::Commit => Some(TestInvocation::Commit),
            Self::Base => Some(TestInvocation::Base),
            Self::Main => Some(TestInvocation::Main),
        }
    }

    pub(crate) fn is_all(self) -> bool {
        matches!(self, Self::All)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Commit => "commit",
            Self::Base => "base",
            Self::Main => "main",
        }
    }
}
