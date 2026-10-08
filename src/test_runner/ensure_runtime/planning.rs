use crate::test_runner::ensure_runtime::EnsureFromPlanned;
use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::test_selection::RunContext;

pub(crate) fn ensure_language_via_kernel(
    language: kiss::Language,
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    mode: AcceptMode,
) -> Result<SelectorExecutionSummary, String> {
    assert!(ctx.options.jobs > 0, "jobs must be greater than zero");
    let mut planned = ctx.planned.clone();
    for other in kiss::Language::ALL {
        let slot = planned.sel.get_mut(other);
        if other == language {
            *slot = selectors.to_vec();
        } else {
            slot.clear();
        }
    }
    let request = ensure_request_from_planned(EnsureFromPlanned {
        planned: &planned,
        mode,
        lang_filter: Some(language),
        force: ctx.options.force_rerun,
        force_selectors: ctx.planned.prior_failure_selectors.get(language).clone(),
        jobs: ctx.options.jobs,
        extras: ctx.options.extras,
        repo_root_override: None,
        gate: ctx.options.gate.clone(),
    });
    let result = super::ensure_languages_runtime(&request)?;
    Ok(result
        .get(language)
        .map(|row| row.summary.clone())
        .unwrap_or_default())
}

pub(crate) fn ensure_request_from_planned(args: EnsureFromPlanned<'_>) -> EnsureRequest {
    EnsureRequest {
        repo_root: args
            .repo_root_override
            .unwrap_or_else(|| args.planned.repo_root.clone()),
        mode: args.mode,
        lang_filter: args.lang_filter,
        ignore: args.planned.ignore.clone(),
        force: args.force,
        force_selectors: args.force_selectors,
        jobs: args.jobs,
        gate: args.gate,
        extras: LanguageKeyed {
            python: args.extras.python.to_vec(),
            rust: args.extras.rust.to_vec(),
        },
        planned: LanguageKeyed {
            python: args.planned.sel.python.clone(),
            rust: args.planned.sel.rust.clone(),
        },
    }
}
