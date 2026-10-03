use super::bind::selector_in_operand_scope;
use super::types::OperandExpr;

fn in_scope(selector: &str, raw: &str) -> bool {
    let operands = [OperandExpr {
        raw: raw.to_string(),
    }];
    selector_in_operand_scope(selector, &[], &operands)
}

#[test]
fn symbol_operand_keeps_only_that_test() {
    assert!(in_scope("test_a.py::test_pass", "test_a.py::test_pass"));
    assert!(in_scope("test_a.py::test_pass[1]", "test_a.py::test_pass"));
    assert!(in_scope("test_a.py::TestX::test_y", "test_a.py::TestX"));
    assert!(!in_scope("test_a.py::test_other", "test_a.py::test_pass"));
    assert!(!in_scope(
        "test_a.py::test_pass_more",
        "test_a.py::test_pass"
    ));
}

#[test]
fn path_operand_keeps_its_file_and_directory() {
    assert!(in_scope("test_a.py::test_pass", "test_a.py"));
    assert!(in_scope("tests/unit/test_x.py::test_y", "tests/unit"));
    assert!(!in_scope("tests/unitx/test_x.py::test_y", "tests/unit"));
    assert!(!in_scope("test_b.py::test_fail", "test_a.py"));
}

#[test]
fn expected_selectors_stay_in_scope() {
    let operands = [OperandExpr {
        raw: "lib_c.py::h".to_string(),
    }];
    let expected = ["test_b.py::test_both_b".to_string()];
    assert!(selector_in_operand_scope(
        "test_b.py::test_both_b",
        &expected,
        &operands
    ));
    assert!(!selector_in_operand_scope(
        "test_b.py::test_fail",
        &expected,
        &operands
    ));
}
