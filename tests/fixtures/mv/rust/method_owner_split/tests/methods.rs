use mv_method_owner_split::{Reviewer, Worker};

fn call(worker: &Worker, reviewer: &Reviewer) -> (i32, i32) {
    (worker.run(4), reviewer.run(4))
}

#[test]
fn methods_stay_distinct() {
    let worker = Worker;
    let reviewer = Reviewer;
    assert_eq!(call(&worker, &reviewer), (5, 6));
}
