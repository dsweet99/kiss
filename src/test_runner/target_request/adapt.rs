use crate::bin_cli::args::TestInvocation;
use crate::test_git::TestChangeMode;
use crate::test_runner::RunTestCmdArgs;

use super::canon::canonicalize_target_request;
use super::types::{CompatKind, GitFocus, LangFilter, OperandExpr, TargetFocus, TargetRequest};

pub(crate) fn is_workspace_focus(focus: &TargetFocus) -> bool {
    matches!(focus, TargetFocus::Workspace)
}

pub(crate) fn request_from_focus(
    focus: TargetFocus,
    lang: Option<kiss::Language>,
    ignore: &[String],
) -> TargetRequest {
    canonicalize_target_request(
        TargetRequest {
            focus,
            lang: lang.map(LangFilter::from_language),
            ignore: ignore.to_vec(),
        },
        None,
    )
}

#[cfg(test)]
pub(crate) fn operands_request(
    raws: &[String],
    lang: Option<kiss::Language>,
    ignore: &[String],
) -> TargetRequest {
    request_from_focus(
        TargetFocus::Operands(
            raws.iter()
                .map(|raw| OperandExpr { raw: raw.clone() })
                .collect(),
        ),
        lang,
        ignore,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn workspace_request(lang: Option<kiss::Language>, ignore: &[String]) -> TargetRequest {
    request_from_focus(TargetFocus::Workspace, lang, ignore)
}

pub(crate) fn is_workspace_run(args: &RunTestCmdArgs<'_>) -> bool {
    is_workspace_focus(&request_from_run_args(args).focus)
}

pub(crate) fn change_mode_from_focus(focus: &TargetFocus) -> TestChangeMode {
    match focus {
        TargetFocus::Git(GitFocus::Commit) => TestChangeMode::Commit,
        TargetFocus::Git(GitFocus::AutomaticBase | GitFocus::ExplicitBase { .. }) => {
            TestChangeMode::Base
        }
        TargetFocus::Git(
            GitFocus::DefaultMain | GitFocus::ConfiguredMain { .. } | GitFocus::ExplicitMain { .. },
        ) => TestChangeMode::Main,
        TargetFocus::Workspace | TargetFocus::Operands(_) => TestChangeMode::Commit,
    }
}

pub(crate) fn operand_raws(focus: &TargetFocus) -> Option<Vec<String>> {
    match focus {
        TargetFocus::Operands(operands) => {
            Some(operands.iter().map(|operand| operand.raw.clone()).collect())
        }
        _ => None,
    }
}

pub(crate) fn request_from_run_args(args: &RunTestCmdArgs<'_>) -> TargetRequest {
    args.target_request.clone()
}

pub(crate) fn request_from_invocation(
    invocation: &TestInvocation,
    main_branch_cli: Option<&str>,
    base_branch_cli: Option<&str>,
    config_main_branch: Option<&str>,
    lang: Option<kiss::Language>,
    ignore: &[String],
) -> TargetRequest {
    request_from_focus(
        focus_from_invocation(
            invocation,
            main_branch_cli,
            base_branch_cli,
            config_main_branch,
        ),
        lang,
        ignore,
    )
}

pub(crate) fn focus_from_invocation(
    invocation: &TestInvocation,
    main_branch_cli: Option<&str>,
    base_branch_cli: Option<&str>,
    config_main_branch: Option<&str>,
) -> TargetFocus {
    match invocation {
        TestInvocation::All => TargetFocus::Workspace,
        TestInvocation::Commit => TargetFocus::Git(GitFocus::Commit),
        TestInvocation::Base => base_focus(base_branch_cli),
        TestInvocation::Main => main_focus(main_branch_cli, config_main_branch),
        TestInvocation::Targets(operands) => TargetFocus::Operands(
            operands
                .iter()
                .map(|raw| OperandExpr { raw: raw.clone() })
                .collect(),
        ),
    }
}

fn base_focus(base_branch_cli: Option<&str>) -> TargetFocus {
    match base_branch_cli {
        Some(branch) => TargetFocus::Git(GitFocus::ExplicitBase {
            branch: branch.to_string(),
        }),
        None => TargetFocus::Git(GitFocus::AutomaticBase),
    }
}

fn main_focus(main_branch_cli: Option<&str>, config_main_branch: Option<&str>) -> TargetFocus {
    if let Some(branch) = main_branch_cli {
        return TargetFocus::Git(GitFocus::ExplicitMain {
            branch: branch.to_string(),
        });
    }
    match config_main_branch {
        Some(name) => TargetFocus::Git(GitFocus::ConfiguredMain {
            name: name.to_string(),
        }),
        None => TargetFocus::Git(GitFocus::DefaultMain),
    }
}

pub(crate) fn compat_matches(request: &TargetRequest, invocation: &TestInvocation) -> bool {
    request.compat_kind() == invocation_kind(invocation)
}

fn invocation_kind(invocation: &TestInvocation) -> CompatKind {
    match invocation {
        TestInvocation::All => CompatKind::Workspace,
        TestInvocation::Commit => CompatKind::Commit,
        TestInvocation::Base => CompatKind::Base,
        TestInvocation::Main => CompatKind::Main,
        TestInvocation::Targets(_) => CompatKind::Operands,
    }
}

pub(crate) fn to_compat_invocation(request: &TargetRequest) -> TestInvocation {
    match &request.focus {
        TargetFocus::Workspace => TestInvocation::All,
        TargetFocus::Git(GitFocus::Commit) => TestInvocation::Commit,
        TargetFocus::Git(GitFocus::AutomaticBase | GitFocus::ExplicitBase { .. }) => {
            TestInvocation::Base
        }
        TargetFocus::Git(
            GitFocus::DefaultMain | GitFocus::ConfiguredMain { .. } | GitFocus::ExplicitMain { .. },
        ) => TestInvocation::Main,
        TargetFocus::Operands(operands) => {
            TestInvocation::Targets(operands.iter().map(|operand| operand.raw.clone()).collect())
        }
    }
}
