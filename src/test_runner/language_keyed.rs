//! Language-keyed values: one abstract slot filled by Python and Rust implementations.
//!
//! Prefer this over parallel `python_*` / `rust_*` product fields on shared types.

use kiss::Language;

/// Pair of language-specific values behind a single abstract concept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LanguageKeyed<T> {
    pub(crate) python: T,
    pub(crate) rust: T,
}

impl<T> LanguageKeyed<T> {
    pub(crate) fn get(&self, language: Language) -> &T {
        match language {
            Language::Python => &self.python,
            Language::Rust => &self.rust,
        }
    }

    #[allow(dead_code)] // mutator for future language-keyed plan updates
    pub(crate) fn get_mut(&mut self, language: Language) -> &mut T {
        match language {
            Language::Python => &mut self.python,
            Language::Rust => &mut self.rust,
        }
    }

    #[allow(dead_code)] // mapper for transforming both language slots uniformly
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut keyed = LanguageKeyed {
            python: 1,
            rust: 2,
        };
        *keyed.get_mut(Language::Python) = 10;
        *keyed.get_mut(Language::Rust) = 20;
        let mapped = keyed.map(|n| n * 2);
        assert_eq!(mapped.python, 20);
        assert_eq!(mapped.rust, 40);
    }
}
