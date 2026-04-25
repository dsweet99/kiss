use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_rust_rename_should_update_use_imports() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("mod.rs");
    let caller_file = tmp.path().join("caller.rs");

    fs::write(&def_file, "pub fn foo() {\n    let _ = 1;\n}\n").unwrap();
    fs::write(
        &caller_file,
        "use crate::mod::foo;\n\nfn call() {\n    foo();\n}\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", def_file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_def = fs::read_to_string(&def_file).unwrap();
    let updated_caller = fs::read_to_string(&caller_file).unwrap();

    assert!(
        updated_def.contains("pub fn bar()"),
        "definition should be renamed to bar; got:\n{updated_def}"
    );
    assert!(
        updated_caller.contains("use crate::mod::bar;"),
        "Rust use-import should be renamed with the symbol; got:\n{updated_caller}"
    );
    assert!(
        updated_caller.contains("bar();"),
        "Rust call site should also be renamed; got:\n{updated_caller}"
    );
}

#[test]
fn regression_rust_rename_should_update_multiline_use_group() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("mod.rs");
    let caller_file = tmp.path().join("caller.rs");

    fs::write(&def_file, "pub fn foo() {}\npub fn other() {}\n").unwrap();
    fs::write(
        &caller_file,
        "use crate::m::{\n    other,\n    foo,\n};\n\nfn call() {\n    foo();\n    other();\n}\n",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", def_file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_caller = fs::read_to_string(&caller_file).unwrap();
    assert!(
        updated_caller.contains("bar,"),
        "multiline use group should rename the symbol; got:\n{updated_caller}"
    );
    assert!(
        updated_caller.contains("bar();"),
        "call site should also be renamed; got:\n{updated_caller}"
    );
    assert!(
        updated_caller.contains("other,"),
        "unrelated symbol in use group should remain unchanged; got:\n{updated_caller}"
    );
}

#[test]
fn regression_rust_rename_should_update_pub_crate_use() {
    let tmp = TempDir::new().unwrap();
    let def_file = tmp.path().join("mod.rs");
    let reexport_file = tmp.path().join("reexport.rs");

    fs::write(&def_file, "pub fn foo() {}\n").unwrap();
    fs::write(&reexport_file, "pub(crate) use crate::m::foo;\n").unwrap();

    let opts = MvOptions {
        query: format!("{}::foo", def_file.display()),
        new_name: "bar".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0);

    let updated_reexport = fs::read_to_string(&reexport_file).unwrap();
    assert!(
        updated_reexport.contains("pub(crate) use crate::m::bar;"),
        "pub(crate) use should rename the symbol; got:\n{updated_reexport}"
    );
}
