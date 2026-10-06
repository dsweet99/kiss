use kiss::Language;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LanguageKeyed<T> {
    pub(crate) python: T,
    pub(crate) rust: T,
}

impl<T> LanguageKeyed<T> {
    pub(crate) fn from_fn(mut f: impl FnMut(Language) -> T) -> Self {
        Self {
            python: f(Language::Python),
            rust: f(Language::Rust),
        }
    }

    pub(crate) fn get(&self, language: Language) -> &T {
        match language {
            Language::Python => &self.python,
            Language::Rust => &self.rust,
        }
    }

    pub(crate) fn get_mut(&mut self, language: Language) -> &mut T {
        match language {
            Language::Python => &mut self.python,
            Language::Rust => &mut self.rust,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn map<U>(self, mut f: impl FnMut(T) -> U) -> LanguageKeyed<U> {
        LanguageKeyed {
            python: f(self.python),
            rust: f(self.rust),
        }
    }
}

impl LanguageKeyed<Vec<String>> {
    pub(crate) fn planned_for(&self, language: Language) -> &[String] {
        self.get(language).as_slice()
    }

    pub(crate) fn as_slices(&self) -> LanguageKeyed<&[String]> {
        LanguageKeyed {
            python: self.python.as_slice(),
            rust: self.rust.as_slice(),
        }
    }

    pub(crate) fn both_empty(&self) -> bool {
        self.python.is_empty() && self.rust.is_empty()
    }
}

impl LanguageKeyed<&[String]> {
    pub(crate) const EMPTY: Self = LanguageKeyed {
        python: &[],
        rust: &[],
    };

    pub(crate) fn owned_vecs(self) -> LanguageKeyed<Vec<String>> {
        LanguageKeyed {
            python: self.python.to_vec(),
            rust: self.rust.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::RunTestCmdArgs;

    #[test]
    fn language_keyed_selects_by_language() {
        let keyed = LanguageKeyed {
            python: vec!["py".into()],
            rust: vec!["rs".into()],
        };
        assert_eq!(keyed.planned_for(Language::Python), &["py".to_string()]);
        assert_eq!(keyed.planned_for(Language::Rust), &["rs".to_string()]);
    }

    #[test]
    fn language_keyed_get_mut_and_map() {
        let mut keyed = LanguageKeyed { python: 1, rust: 2 };
        *keyed.get_mut(Language::Python) = 10;
        *keyed.get_mut(Language::Rust) = 20;
        let mapped = keyed.map(|n| n * 2);
        assert_eq!(mapped.python, 20);
        assert_eq!(mapped.rust, 40);
    }

    #[test]
    fn run_test_cmd_args_carries_language_keyed_extras() {
        // Surface uses one LanguageKeyed extras field, not sibling extra / python_extra.
        let py = vec!["-k".to_string()];
        let rs = vec!["--ignored".to_string()];
        let extras = LanguageKeyed {
            python: py.as_slice(),
            rust: rs.as_slice(),
        };
        let request = crate::test_runner::target_request::request_from_invocation(
            &crate::bin_cli::args::TestInvocation::All,
            None,
            None,
            None,
            None,
            &[],
        );
        let args = RunTestCmdArgs {
            doubles: None,
            invocation: crate::test_runner::target_request::to_compat_invocation(&request),
            target_request: request,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
            jobs: 1,
            extras,
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        };
        assert_eq!(args.extras.python, &["-k".to_string()]);
        assert_eq!(args.extras.rust, &["--ignored".to_string()]);
        assert_eq!(*args.extras.get(Language::Python), &["-k".to_string()][..]);
    }

    #[test]
    fn language_keyed_slice_helpers_round_trip() {
        let owned = LanguageKeyed {
            python: vec!["a".into()],
            rust: vec!["b".into()],
        };
        let slices = owned.as_slices();
        assert!(!slices.owned_vecs().both_empty());
        assert!(LanguageKeyed::<&[String]>::EMPTY.owned_vecs().both_empty());
    }
}
