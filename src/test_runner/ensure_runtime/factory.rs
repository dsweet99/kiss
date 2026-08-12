//! Composition root: construct language modules and call the ensure kernel.

use crate::test_runner::coverage_decision::SupportedLanguage;
use crate::test_runner::ensure_runtime::ensure_runtime_cache;
use crate::test_runner::lang_iface::{EnsureRequest, EnsureRuntimeResult, LanguageRuntime};
use crate::test_runner::lang_python::PythonRuntime;
use crate::test_runner::lang_rust::RustRuntime;

pub(crate) fn ensure_languages_runtime(
    request: &EnsureRequest,
) -> Result<EnsureRuntimeResult, String> {
    let python = PythonRuntime;
    let rust = RustRuntime;
    let mut modules: Vec<&dyn LanguageRuntime> = Vec::new();
    if request.requires(SupportedLanguage::language(&python)) {
        modules.push(&python);
    }
    if request.requires(SupportedLanguage::language(&rust)) {
        modules.push(&rust);
    }
    ensure_runtime_cache(request, &modules)
}
