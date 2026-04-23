pub fn helper(value: i32) -> i32 {
    let label = "helper should stay in strings";
    let _ = label;
    value + 1
}

pub fn caller() -> i32 {
    helper(4)
}
