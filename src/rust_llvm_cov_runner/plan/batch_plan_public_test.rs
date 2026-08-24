use super::{RustCoverageBatchPlan, RustCoverageBatchRequest, build_rust_coverage_batch_plan};

#[test]
fn batch_request_witness_exercises_public_contract_in_module() {
    let req = RustCoverageBatchRequest::witness();
    let cloned = req.clone();

    assert_eq!(cloned, req);
    assert!(format!("{req:?}").contains("RustCoverageBatchRequest"));
    assert_eq!(req.logical_selectors.len(), 2);
    assert_eq!(req.jobs, 4);
    assert!(build_rust_coverage_batch_plan(&req).is_ok());
}

#[test]
fn batch_plan_witness_exercises_public_contract_in_module() {
    let plan = RustCoverageBatchPlan::witness();
    let cloned = plan.clone();

    assert_eq!(cloned, plan);
    assert!(format!("{plan:?}").contains("RustCoverageBatchPlan"));
    assert_eq!(plan.argv[0], "cargo");
    assert_eq!(plan.env["NEXTEST_EXPERIMENTAL_LIBTEST_JSON"], "1");
}
