//! Fixture: production functions "covered" only by syntactic witnesses that
//! never run at runtime. Each bypass family mirrors cases in
//! `src/rust_test_refs/tests_coverage_witness.rs` but lives in `fake_rust/` for
//! integration tests and human inspection.

pub fn actually_covered() -> i32 {
    1
}

pub fn fn_value_only() -> i32 {
    let mut total = 0;
    for step in 0..60 {
        total += step;
    }
    total
}

pub fn dead_branch_only() -> i32 {
    let mut total = 0;
    for step in 0..60 {
        total += step;
    }
    total
}

pub fn stringify_only() -> i32 {
    let mut total = 0;
    for step in 0..60 {
        total += step;
    }
    total
}

pub fn uncalled_helper_only() -> i32 {
    let mut total = 0;
    for step in 0..60 {
        total += step;
    }
    total
}

pub fn closure_only() -> i32 {
    let mut total = 0;
    for step in 0..60 {
        total += step;
    }
    total
}

macro_rules! stringify_cheat {
    ($t:tt) => {
        stringify!($t)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_actually_covered() {
        assert_eq!(actually_covered(), 1);
    }

    #[test]
    fn witness_fn_value() {
        let _ = fn_value_only;
    }

    #[test]
    fn witness_dead_branch() {
        if false {
            let _ = dead_branch_only();
        }
    }

    #[test]
    fn witness_stringify() {
        stringify_cheat!(stringify_only);
        assert!(true);
    }

    fn witness_farm() {
        if false {
            let _ = uncalled_helper_only();
        }
    }

    #[test]
    fn witness_uncalled_helper() {
        assert!(true);
    }

    #[test]
    fn witness_closure() {
        let _ = || {
            closure_only();
        };
    }
}
