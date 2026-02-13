// Fixture: uses `crate::...` import, which should create an internal edge.
use crate::orphan_crate_use_target::do_work;

pub fn call() -> i32 {
    do_work()
}

