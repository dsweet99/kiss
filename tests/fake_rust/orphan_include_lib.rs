// Fixture: includes a file via `include!` (no `mod`), but the included file is still "used".
include!("orphan_include_target.rs");

pub fn call() -> i32 {
    do_work()
}

