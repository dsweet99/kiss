use crate::test_runner::coverage_decision::LanguageExecutor;
use crate::test_runner::runners::python_backer::PythonModule;
use crate::test_runner::runners::rust_backer::RustModule;

fn dry_run_selector_options() -> crate::test_runner::SelectorRunOptions<'static> {
    crate::test_runner::SelectorRunOptions {
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        plan_duration: std::time::Duration::ZERO,
        gate: kiss::GateConfig::default(),
    }
}

#[path = "language_modules_test.rs"]
mod tests;
