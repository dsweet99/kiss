
use super::analysis::{LanguageAnalysis, PythonAnalysis, RustAnalysis};
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::rust_units::{RustCodeUnit, extract_rust_code_units};
use crate::units::{CodeUnit, extract_code_units};

pub trait LanguageUnits: LanguageAnalysis {
    type Parsed;
    type Unit;
    fn extract_units(&self, parsed: &Self::Parsed) -> Vec<Self::Unit>;
}

impl LanguageUnits for PythonAnalysis {
    type Parsed = ParsedFile;
    type Unit = CodeUnit;

    fn extract_units(&self, parsed: &Self::Parsed) -> Vec<Self::Unit> {
        extract_code_units(parsed)
    }
}

impl LanguageUnits for RustAnalysis {
    type Parsed = ParsedRustFile;
    type Unit = RustCodeUnit;

    fn extract_units(&self, parsed: &Self::Parsed) -> Vec<Self::Unit> {
        extract_rust_code_units(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang_analysis::{LanguageParser, PythonAnalysis, RustAnalysis};

    #[test]
    fn extract_units_via_language_trait() {
        let tmp = tempfile::tempdir().unwrap();
        let py = tmp.path().join("u.py");
        let rs = tmp.path().join("u.rs");
        std::fs::write(&py, "def g():\n    pass\n").unwrap();
        std::fs::write(&rs, "pub fn g() {}\n").unwrap();
        let py_parsed = PythonAnalysis
            .parse_many(&[py])
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        let rs_parsed = RustAnalysis
            .parse_many(&[rs])
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        assert!(!PythonAnalysis.extract_units(&py_parsed).is_empty());
        assert!(!RustAnalysis.extract_units(&rs_parsed).is_empty());
    }
}
