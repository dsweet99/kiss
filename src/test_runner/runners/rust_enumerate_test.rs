use super::{rust_dynamic_listing_jobs, rust_file_needs_dynamic_listing};

#[test]
fn dynamic_rust_listing_uses_repository_num_jobs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join(".kissconfig"), "[test]\nnum_jobs = 7\n").expect("config");

    assert_eq!(rust_dynamic_listing_jobs(tmp.path()).expect("jobs"), 7);
}

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
