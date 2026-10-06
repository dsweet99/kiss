use super::rust_file_needs_dynamic_listing;

#[test]
fn tokio_test_does_not_need_dynamic_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("lib.rs");
    std::fs::write(&path, "#[tokio::test]\nasync fn foo() {}\n").expect("write");
    assert!(!rust_file_needs_dynamic_listing(&path));
}

#[test]
fn rstest_needs_dynamic_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("lib.rs");
    std::fs::write(&path, "#[rstest]\nfn foo() {}\n").expect("write");
    assert!(rust_file_needs_dynamic_listing(&path));
}

#[test]
fn item_macro_include_does_not_need_dynamic_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("lib.rs");
    std::fs::write(&path, "include!(\"x.rs\");\n#[test]\nfn foo() {}\n").expect("write");
    assert!(!rust_file_needs_dynamic_listing(&path));
}

#[test]
fn should_panic_and_serial_do_not_need_dynamic_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("lib.rs");
    std::fs::write(
        &path,
        "#[test]\n#[should_panic]\nfn boom() {}\n#[test]\n#[serial]\nfn one() {}\n",
    )
    .expect("write");
    assert!(!rust_file_needs_dynamic_listing(&path));
}

#[test]
fn local_macro_rules_generating_tests_needs_dynamic_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("lib.rs");
    std::fs::write(
        &path,
        "macro_rules! cases { ($name:ident) => { #[test] fn $name() {} }; }\ncases!(generated);\n",
    )
    .expect("write");
    assert!(rust_file_needs_dynamic_listing(&path));
}

#[test]
fn defining_source_for_selector_maps_module_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let module = src.join("widget.rs");
    std::fs::write(&lib, "").unwrap();
    std::fs::write(&module, "").unwrap();
    let got = super::defining_source_for_selector(
        &lib,
        "widget::test_it",
        &[lib.clone(), module.clone()],
    );
    assert_eq!(got, module);
}

#[test]
fn flatten_parsed_entries_skips_errors_when_policy_skip() {
    use std::path::PathBuf;

    let ok = Ok((PathBuf::from("a.rs"), vec!["t1".into(), "t2".into()]));
    let err = Err("boom".to_string());
    let entries =
        super::flatten_parsed_entries(vec![ok, err], super::ParseErrorPolicy::Skip).unwrap();
    assert_eq!(entries.len(), 2);
    let hard =
        super::flatten_parsed_entries(vec![Err("boom".into())], super::ParseErrorPolicy::Fail);
    assert!(hard.is_err());
}
