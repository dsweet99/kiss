use crate::test_runner::ensure_runtime::EnsureFromPlanned;
use crate::test_runner::lang_iface::EnsureRequest;
use crate::test_runner::language_keyed::LanguageKeyed;

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
