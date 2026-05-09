use kiss::Language;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    RenameOnly,
    MoveOnly,
    MoveAndRename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    #[allow(dead_code)]
    PythonSimplePackage,
    PythonMethodOwnerSplit,
    PythonImportForms,
    #[allow(dead_code)]
    RustSimpleCrate,
    RustMethodOwnerSplit,
    RustImportForms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioSpec {
    pub name: &'static str,
    pub fixture: FixtureKind,
    pub language: Language,
    pub move_kind: MoveKind,
    pub query: &'static str,
    pub new_name: &'static str,
    pub destination: Option<&'static str>,
    pub should_succeed: bool,
    pub checks_round_trip: bool,
}

pub fn scenario_specs() -> Vec<ScenarioSpec> {
    vec![
        ScenarioSpec {
            name: "python_method_rename",
            fixture: FixtureKind::PythonMethodOwnerSplit,
            language: Language::Python,
            move_kind: MoveKind::RenameOnly,
            query: "pkg/source.py::Worker.run",
            new_name: "execute",
            destination: None,
            should_succeed: true,
            checks_round_trip: true,
        },
        ScenarioSpec {
            name: "python_move_only",
            fixture: FixtureKind::PythonImportForms,
            language: Language::Python,
            move_kind: MoveKind::MoveOnly,
            query: "pkg/source.py::movable_fn",
            new_name: "movable_fn",
            destination: Some("pkg/dest.py"),
            should_succeed: true,
            checks_round_trip: false,
        },
        ScenarioSpec {
            name: "python_move_and_rename",
            fixture: FixtureKind::PythonImportForms,
            language: Language::Python,
            move_kind: MoveKind::MoveAndRename,
            query: "pkg/source.py::move_rename_fn",
            new_name: "relocated_fn",
            destination: Some("pkg/dest.py"),
            should_succeed: true,
            checks_round_trip: false,
        },
        ScenarioSpec {
            name: "rust_method_rename",
            fixture: FixtureKind::RustMethodOwnerSplit,
            language: Language::Rust,
            move_kind: MoveKind::RenameOnly,
            query: "src/lib.rs::Worker.run",
            new_name: "execute",
            destination: None,
            should_succeed: true,
            checks_round_trip: true,
        },
        ScenarioSpec {
            name: "rust_move_only",
            fixture: FixtureKind::RustImportForms,
            language: Language::Rust,
            move_kind: MoveKind::MoveOnly,
            query: "src/source.rs::movable_fn",
            new_name: "movable_fn",
            destination: Some("src/dest.rs"),
            should_succeed: true,
            checks_round_trip: false,
        },
        ScenarioSpec {
            name: "rust_move_and_rename",
            fixture: FixtureKind::RustImportForms,
            language: Language::Rust,
            move_kind: MoveKind::MoveAndRename,
            query: "src/source.rs::move_rename_fn",
            new_name: "relocated_fn",
            destination: Some("src/dest.rs"),
            should_succeed: true,
            checks_round_trip: false,
        },
    ]
}

pub fn fixture_root(kind: FixtureKind) -> &'static Path {
    match kind {
        FixtureKind::PythonSimplePackage => Path::new("tests/fixtures/mv/python/simple_package"),
        FixtureKind::PythonMethodOwnerSplit => {
            Path::new("tests/fixtures/mv/python/method_owner_split")
        }
        FixtureKind::PythonImportForms => Path::new("tests/fixtures/mv/python/import_forms"),
        FixtureKind::RustSimpleCrate => Path::new("tests/fixtures/mv/rust/simple_crate"),
        FixtureKind::RustMethodOwnerSplit => Path::new("tests/fixtures/mv/rust/method_owner_split"),
        FixtureKind::RustImportForms => Path::new("tests/fixtures/mv/rust/import_forms"),
    }
}

#[test]
fn scenario_catalog_covers_both_languages_and_core_move_kinds() {
    let specs = scenario_specs();
    assert!(
        specs.iter().any(|s| s.language == Language::Python),
        "scenario catalog must include Python scenarios"
    );
    assert!(
        specs.iter().any(|s| s.language == Language::Rust),
        "scenario catalog must include Rust scenarios"
    );
    assert!(
        specs.iter().any(|s| s.move_kind == MoveKind::RenameOnly),
        "scenario catalog must include rename-only coverage"
    );
    assert!(
        specs.iter().any(|s| s.move_kind == MoveKind::MoveOnly),
        "scenario catalog must include move-only coverage"
    );
    assert!(
        specs.iter().any(|s| s.move_kind == MoveKind::MoveAndRename),
        "scenario catalog must include move+rename coverage"
    );
}
