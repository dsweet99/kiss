use super::JobShare;
use kiss::Language;
use std::sync::Barrier;

#[test]
fn both_languages_execute_concurrently_with_full_budget() {
    let share = JobShare::new(4, true);
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let rust = scope.spawn(|| {
            let turn = share.acquire_execute(Language::Rust);
            assert_eq!(turn.jobs, 4);
            assert_eq!(share.covering(Language::Rust), 2);
            barrier.wait();
        });
        let python = share.acquire_execute(Language::Python);
        assert_eq!(python.jobs, 4);
        assert_eq!(share.covering(Language::Python), 2);
        barrier.wait();
        rust.join().unwrap();
    });
}

#[test]
fn one_language_executes_with_full_budget() {
    let share = JobShare::new(4, false);
    assert_eq!(share.acquire_execute(Language::Python).jobs, 4);
    assert_eq!(share.acquire_execute(Language::Rust).jobs, 4);
}
