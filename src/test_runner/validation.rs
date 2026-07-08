use std::collections::BTreeSet;

use kiss::Language;

use super::{PlannedSelectors, runners};
use crate::test_git::TestChangeMode;

#[path = "validation/tiny_recall.rs"]
mod tiny_recall;

pub(crate) use tiny_recall::run_tiny_recall_fixture;

pub struct ValidateSelectionCmdArgs<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub dry_run: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub fixture: Option<&'a str>,
    pub config_main_branch: Option<&'a str>,
}

impl ValidateSelectionCmdArgs<'_> {
    pub(crate) fn change_mode(&self) -> TestChangeMode {
        self.mode
    }

    pub(crate) fn main_branch_arg(&self) -> Option<&str> {
        self.main_branch_cli
    }

    pub(crate) fn base_branch_arg(&self) -> Option<&str> {
        self.base_branch_cli
    }

    pub(crate) fn normalized_lang_filter(&self) -> Option<Language> {
        self.lang_filter
    }

    pub(crate) fn planning_extra_args(&self) -> &[String] {
        self.extra
    }

    pub(crate) fn planning_ignore_args(&self) -> &[String] {
        self.ignore
    }

    pub(crate) fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn fixture_name(&self) -> Option<&str> {
        self.fixture
    }

    pub(crate) fn has_positive_jobs(&self) -> bool {
        self.jobs > 0
    }

    pub(crate) fn validate_dry_run_request(&self) -> Result<(), String> {
        if let Some(fixture) = self.fixture_name() {
            if fixture == "tiny-recall" {
                return validate_positive_jobs(self);
            }
            return Err(format!(
                "error: kiss test validate-selection fixture '{fixture}' is not implemented yet"
            ));
        }
        if !self.is_dry_run() {
            return Err(
                "error: kiss test validate-selection currently supports --dry-run only".into(),
            );
        }
        validate_positive_jobs(self)
    }
}

fn validate_positive_jobs(args: &ValidateSelectionCmdArgs<'_>) -> Result<(), String> {
    if args.has_positive_jobs() {
        Ok(())
    } else {
        Err("error: kiss test validate-selection jobs must be greater than zero".into())
    }
}

pub(crate) struct ValidationReport {
    pub(crate) selected_python: usize,
    pub(crate) selected_rust: usize,
    pub(crate) full_python: usize,
    pub(crate) full_rust: usize,
    pub(crate) python_population_required: bool,
    pub(crate) rust_population_required: bool,
}

impl ValidationReport {
    pub(crate) fn selected_for_language(&self, language: Language) -> usize {
        match language {
            Language::Python => self.selected_python,
            Language::Rust => self.selected_rust,
        }
    }

    pub(crate) fn full_for_language(&self, language: Language) -> usize {
        match language {
            Language::Python => self.full_python,
            Language::Rust => self.full_rust,
        }
    }

    pub(crate) fn has_selected_tests(&self) -> bool {
        self.selected_total() > 0
    }

    pub(crate) fn has_full_universe(&self) -> bool {
        self.full_total() > 0
    }

    pub(crate) fn selected_total(&self) -> usize {
        self.selected_python + self.selected_rust
    }

    pub(crate) fn full_total(&self) -> usize {
        self.full_python + self.full_rust
    }

    pub(crate) fn selection_ratio(&self) -> Option<f64> {
        if !self.has_full_universe() {
            None
        } else if !self.has_selected_tests() {
            Some(0.0)
        } else {
            Some(self.selected_total() as f64 / self.full_total() as f64)
        }
    }

    pub(crate) fn rust_population_required(&self) -> bool {
        self.rust_population_required
    }

    pub(crate) fn python_population_required(&self) -> bool {
        self.python_population_required
    }

    pub(crate) fn print(&self, dry_run: bool) {
        println!("KISS TEST VALIDATION");
        println!("dry_run={dry_run}");
        println!(
            "selected_python={}",
            self.selected_for_language(Language::Python)
        );
        println!(
            "selected_rust={}",
            self.selected_for_language(Language::Rust)
        );
        println!("full_python={}", self.full_for_language(Language::Python));
        println!("full_rust={}", self.full_for_language(Language::Rust));
        println!("selected_total={}", self.selected_total());
        println!("full_total={}", self.full_total());
        println!(
            "python_population_required={}",
            self.python_population_required()
        );
        println!(
            "rust_population_required={}",
            self.rust_population_required()
        );
        match self.selection_ratio() {
            Some(ratio) => println!("selection_ratio={ratio:.6}"),
            None => println!("selection_ratio=0"),
        }
    }
}

pub(crate) fn validation_report(
    planned: &PlannedSelectors,
    lang_filter: Option<Language>,
) -> Result<ValidationReport, String> {
    let include_python = lang_filter != Some(Language::Rust);
    let include_rust = lang_filter != Some(Language::Python);
    let full_python = if include_python {
        runners::enumerate_workspace_python_selectors(&planned.repo_root, &planned.ignore)?
    } else {
        Vec::new()
    };
    let full_rust = if include_rust {
        runners::enumerate_workspace_rust_selectors(&planned.repo_root, &planned.ignore)?
    } else {
        Vec::new()
    };
    let mut selected_python: BTreeSet<String> = planned.py_sel.iter().cloned().collect();
    let mut selected_rust: BTreeSet<String> = planned.rs_sel.iter().cloned().collect();
    let python_population_required = planned.python_population_required;
    let rust_population_required = !planned.rust_source_population_paths.is_empty();
    if python_population_required {
        selected_python.extend(full_python.iter().cloned());
    }
    if rust_population_required {
        selected_rust.extend(full_rust.iter().cloned());
    }
    if !include_python {
        selected_python.clear();
    }
    if !include_rust {
        selected_rust.clear();
    }
    Ok(ValidationReport {
        selected_python: selected_python.len(),
        selected_rust: selected_rust.len(),
        full_python: full_python.len(),
        full_rust: full_rust.len(),
        python_population_required,
        rust_population_required,
    })
}
