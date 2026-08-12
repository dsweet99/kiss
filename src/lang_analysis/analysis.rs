//! Shared language-understanding identity.

use crate::discovery::Language;

/// Shared language-understanding identity for analysis stacks.
pub trait LanguageAnalysis {
    fn language(&self) -> Language;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PythonAnalysis;

#[derive(Clone, Copy, Debug, Default)]
pub struct RustAnalysis;

impl LanguageAnalysis for PythonAnalysis {
    fn language(&self) -> Language {
        Language::Python
    }
}

impl LanguageAnalysis for RustAnalysis {
    fn language(&self) -> Language {
        Language::Rust
    }
}
