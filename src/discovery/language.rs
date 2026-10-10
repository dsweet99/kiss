use std::path::Path;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
    Rust,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::Python, Self::Rust];

    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| ext.eq_ignore_ascii_case(language.extension()))
    }

    #[must_use]
    pub fn from_path_extension(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.label() == label)
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub fn allowed_by(self, filter: Option<Self>) -> bool {
        filter.is_none_or(|only| only == self)
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        if crate::rust_include::is_rust_source_path(path) {
            Some(Self::Rust)
        } else {
            Self::from_path_extension(path).filter(|language| *language == Self::Python)
        }
    }

    #[must_use]
    pub fn is_rust_path(path: &Path) -> bool {
        crate::rust_include::is_rust_source_path(path)
    }

    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Rust => "rs",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Rust => "rust",
        }
    }
}

#[cfg(test)]
#[path = "language_test.rs"]
mod tests;
