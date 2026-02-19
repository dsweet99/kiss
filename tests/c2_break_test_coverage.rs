use kiss::parsing::{ParsedFile, create_parser, parse_file};
use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use kiss::test_refs::analyze_test_refs;
use kiss::rust_test_refs::analyze_rust_test_refs;
use std::io::Write;
use std::path::Path;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

// ---------------------------------------------------------------------------
// C2 Break #1 — Bare-name collision inflates coverage (false positive)
//
// The checker matches definitions to references by bare name only.
// If two files each define a function with the same name (e.g. `process`),
// a test that exercises only ONE of them marks BOTH as covered.
//
// Prediction: the `process` in `billing.py` should be UNREFERENCED because
// the test only exercises the `process` in `auth.py`.
// ---------------------------------------------------------------------------
#[test]
fn c2_cov_1_bare_name_collision_inflates_coverage() {
    let auth = {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "def process():\n    return 'auth'\n").unwrap();
        parse_py(tmp.path())
    };
    let billing = {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "def process():\n    return 'billing'\n").unwrap();
        parse_py(tmp.path())
    };
    let test_file = {
        let mut tmp = tempfile::Builder::new().prefix("test_").suffix(".py").tempfile().unwrap();
        write!(
            tmp,
            "from auth import process\n\ndef test_auth():\n    process()\n"
        )
        .unwrap();
        parse_py(tmp.path())
    };

    let files: Vec<&ParsedFile> = vec![&auth, &billing, &test_file];
    let analysis = analyze_test_refs(&files);

    let billing_unreferenced = analysis
        .unreferenced
        .iter()
        .any(|d| d.name == "process" && d.file == billing.path);

    assert!(
        billing_unreferenced,
        "billing.process should be UNREFERENCED — the test only exercises auth.process. \
         But bare-name matching marked both as covered.\n\
         unreferenced: {:#?}\n\
         test_references: {:?}",
        analysis.unreferenced, analysis.test_references
    );
}

// ---------------------------------------------------------------------------
// C2 Break #2 — Framework import reclassifies production file as test file
//
// A production file that `import pytest` (e.g. a helper using pytest.raises
// for validation) is silently classified as a test file. Its definitions
// vanish from the denominator and its identifiers flood the reference set.
//
// Prediction: the definitions in `validator.py` (which imports pytest)
// should still appear in `analysis.definitions`.
// ---------------------------------------------------------------------------
#[test]
fn c2_cov_2_framework_import_reclassifies_production_as_test() {
    let validator = {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(
            tmp,
            "import pytest\n\ndef validate(x):\n    if x < 0:\n        pytest.fail('negative')\n    return x\n"
        )
        .unwrap();
        parse_py(tmp.path())
    };

    let files: Vec<&ParsedFile> = vec![&validator];
    let analysis = analyze_test_refs(&files);

    let has_validate_def = analysis.definitions.iter().any(|d| d.name == "validate");

    assert!(
        has_validate_def,
        "validator.py defines `validate` but it was reclassified as a test file \
         because it imports pytest. Its definitions vanished from the pool.\n\
         definitions: {:#?}",
        analysis.definitions
    );
}

// ---------------------------------------------------------------------------
// C2 Break #3 — Test helpers outside test_* scope are invisible
//
// `collect_references` only scans inside `test_*` functions and `Test*`
// classes. A common pattern: test_* functions delegate to a shared helper
// `run_scenario(MyService)`, and the actual production-code references
// are inside that helper — invisible to the scanner.
//
// Prediction: `MyService` should appear in `test_references` because
// it is called (indirectly) from a test function.
// ---------------------------------------------------------------------------
#[test]
fn c2_cov_3_test_helper_refs_outside_test_scope_invisible() {
    let service = {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "class MyService:\n    def run(self):\n        return 42\n").unwrap();
        parse_py(tmp.path())
    };
    let test_file = {
        let mut tmp = tempfile::Builder::new().prefix("test_").suffix(".py").tempfile().unwrap();
        write!(
            tmp,
            "from service import MyService\n\n\
             def run_scenario(svc_class):\n\
             \x20\x20\x20\x20return svc_class().run()\n\n\
             def test_service():\n\
             \x20\x20\x20\x20result = run_scenario(MyService)\n\
             \x20\x20\x20\x20assert result == 42\n"
        )
        .unwrap();
        parse_py(tmp.path())
    };

    let files: Vec<&ParsedFile> = vec![&service, &test_file];
    let analysis = analyze_test_refs(&files);

    assert!(
        analysis.unreferenced.is_empty(),
        "MyService is passed to run_scenario() from test_service(), so it should be \
         considered covered. But `run_scenario` is outside `test_*` scope, so references \
         inside it are invisible.\n\
         unreferenced: {:#?}\n\
         test_references: {:?}",
        analysis.unreferenced, analysis.test_references
    );
}

// ---------------------------------------------------------------------------
// C2 Break #4 — Nested/inner function definitions are invisible (Python)
//
// `collect_definitions` stops recursing at `function_definition`. Functions
// defined inside other functions (closures, factory builders, decorators)
// are never counted as definitions. A module can hide complex logic inside
// nested functions, and the coverage checker will never notice it's untested.
//
// Prediction: `inner_logic` should appear in `analysis.definitions`.
// ---------------------------------------------------------------------------
#[test]
fn c2_cov_4_nested_function_definitions_invisible() {
    let module = {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(
            tmp,
            "def outer():\n\
             \x20\x20\x20\x20def inner_logic():\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return do_expensive_computation()\n\
             \x20\x20\x20\x20return inner_logic()\n"
        )
        .unwrap();
        parse_py(tmp.path())
    };

    let files: Vec<&ParsedFile> = vec![&module];
    let analysis = analyze_test_refs(&files);

    let has_inner = analysis
        .definitions
        .iter()
        .any(|d| d.name == "inner_logic");

    assert!(
        has_inner,
        "inner_logic is a real function with complex logic, but `collect_definitions` \
         does not recurse past the outer function boundary, so it's invisible.\n\
         definitions: {:#?}",
        analysis.definitions
    );
}

// ---------------------------------------------------------------------------
// C2 Break #5 — Compound #[cfg(...)] attributes bypass test detection (Rust)
//
// `has_cfg_test_attribute` only recognizes `#[cfg(test)]` parsed as a single
// ident. Compound forms like `#[cfg(all(test, feature = "..."))]` are not
// recognized. Definitions inside such blocks are counted as production code,
// and test references inside them are not collected.
//
// Prediction: a function inside `#[cfg(all(test, feature = "integration"))]`
// should be recognized as test code. Its references should be collected, and
// its functions should NOT appear as production definitions.
// ---------------------------------------------------------------------------
#[test]
fn c2_cov_5_compound_cfg_test_bypasses_detection() {
    let rust_file = {
        let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        write!(
            tmp,
            "fn production_fn() {{}}\n\n\
             #[cfg(all(test, feature = \"integration\"))]\n\
             mod integration_tests {{\n\
             \x20\x20\x20\x20fn test_it() {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20production_fn();\n\
             \x20\x20\x20\x20}}\n\
             }}\n"
        )
        .unwrap();
        parse_rust_file(tmp.path()).unwrap()
    };

    let files: Vec<&ParsedRustFile> = vec![&rust_file];
    let analysis = analyze_rust_test_refs(&files);

    let has_production_fn_ref = analysis.test_references.contains("production_fn");
    let test_it_is_definition = analysis.definitions.iter().any(|d| d.name == "test_it");

    assert!(
        has_production_fn_ref,
        "production_fn is called inside #[cfg(all(test, ...))] mod, but compound cfg \
         is not recognized as a test module. References inside it are not collected.\n\
         test_references: {:?}",
        analysis.test_references
    );
    assert!(
        !test_it_is_definition,
        "test_it is inside a #[cfg(all(test, ...))] block and should NOT be a production \
         definition, but the compound cfg attribute is not recognized.\n\
         definitions: {:#?}",
        analysis.definitions
    );
}
