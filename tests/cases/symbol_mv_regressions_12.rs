use kiss::symbol_mv::run_mv_command;
use std::fs;
use tempfile::TempDir;

use super::symbol_mv_regressions_11::{py, rs};

#[test]
fn regression_python_function_as_value_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
def my_fn(x):
    return x * 2


vals = list(map(my_fn, [1, 2, 3]))
sorted_vals = sorted([1, 2, 3], key=my_fn)
ref = my_fn
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(py(
            &format!("{}::my_fn", file.display()),
            "doubled",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("def doubled("),
        "definition my_fn should be renamed to doubled; got:\n{updated}"
    );
    assert!(
        updated.contains("map(doubled,"),
        "callback `map(my_fn, ...)` must become `map(doubled, ...)`; got:\n{updated}"
    );
    assert!(
        updated.contains("key=doubled"),
        "kwarg value `key=my_fn` must become `key=doubled`; got:\n{updated}"
    );
    assert!(
        updated.contains("ref = doubled"),
        "assignment RHS `ref = my_fn` must become `ref = doubled`; got:\n{updated}"
    );
    assert!(
        !updated.contains("my_fn"),
        "no occurrence of the old name `my_fn` should remain; got:\n{updated}"
    );
}

#[test]
fn regression_rust_function_as_pointer_value_should_be_renamed() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
pub fn doubler(x: i32) -> i32 { x * 2 }

pub fn run() -> Vec<i32> {
    let v = vec![1, 2, 3];
    let _: Vec<i32> = v.iter().map(|x| doubler(*x)).collect();
    let f = doubler;
    let g: fn(i32) -> i32 = doubler;
    vec![f(1), g(2)]
}
",
    )
    .unwrap();

    assert_eq!(
        run_mv_command(rs(
            &format!("{}::doubler", file.display()),
            "tripler",
            tmp.path(),
        )),
        0,
    );

    let updated = fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("pub fn tripler("),
        "definition doubler should be renamed to tripler; got:\n{updated}"
    );
    assert!(
        updated.contains("|x| tripler(*x)"),
        "call site inside closure must be renamed; got:\n{updated}"
    );
    assert!(
        updated.contains("let f = tripler;"),
        "fn-pointer assignment `let f = doubler;` must become `let f = tripler;`; got:\n{updated}"
    );
    assert!(
        updated.contains("fn(i32) -> i32 = tripler;"),
        "typed fn-pointer binding `let g: fn(i32) -> i32 = doubler;` must be renamed; got:\n{updated}"
    );
    assert!(
        !updated.contains("doubler"),
        "no occurrence of the old name `doubler` should remain; got:\n{updated}"
    );
}
