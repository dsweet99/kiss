mod part_a;
mod part_b;
mod part_c;

use std::sync::LazyLock;

pub(crate) static FORKSERVER_CONTROLLER: LazyLock<String> = LazyLock::new(|| {
    let mut script = String::new();
    script.push_str(part_a::FORKSERVER_CONTROLLER_A);
    script.push_str(part_b::FORKSERVER_CONTROLLER_B);
    script.push_str(part_c::FORKSERVER_CONTROLLER_C);
    script
});
