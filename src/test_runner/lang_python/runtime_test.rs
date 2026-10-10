use super::PythonRuntime;
use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest, LanguageRuntime};
use crate::test_runner::test_selection::SupportedLanguage;

#[test]
fn python_runtime_language_and_no_generation_publish() {
    let rt = PythonRuntime;
    assert_eq!(rt.language(), kiss::Language::Python);
    let src = include_str!("runtime.rs");
    assert!(!src.contains("python_generation_publish"));
}

#[test]
fn python_runtime_empty_run_and_identity_paths() {
    let rt = PythonRuntime;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let req = EnsureRequest {
        repo_root: tmp.path().to_path_buf(),
        mode: AcceptMode::All,
        lang_filter: Some(kiss::Language::Python),
        ignore: vec![],
        force: false,
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["a".into()],
            rust: vec![],
        },
    };
    let batch = super::runtime::run_python_selectors(&req, &[], &mut |_| {}).expect("empty");
    assert_eq!(batch.summary.total, 0);
    let _ = rt.list(&req).map(|listing| listing.identity);
}
