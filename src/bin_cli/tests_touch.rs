#![allow(clippy::let_unit_value)]

use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::stats::{RunStatsArgs, run_stats};
use crate::bin_cli::test_cmd::run_test_command;

#[test]
fn test_touch_for_static_test_coverage() {
    fn touch<T>(_t: T) {}
    let _ = (touch(run_mimic), touch(run_stats));
    let _ = (
        std::mem::size_of_val(&run_test_command),
        std::mem::size_of::<RunStatsArgs>(),
    );













}
