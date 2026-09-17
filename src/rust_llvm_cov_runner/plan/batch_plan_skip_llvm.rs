use super::batch_plan::RustCoverageBatchPlan;

pub(crate) fn rewrite_plan_argv_skip_llvm_cov_wrapper(plan: &mut RustCoverageBatchPlan) {
    let argv = &mut plan.argv;
    if argv.len() < 3 {
        return;
    }
    if argv[1] != "llvm-cov" || argv[2] != "nextest" {
        return;
    }
    argv[1] = "nextest".to_string();
    argv[2] = "run".to_string();
    if let Some(idx) = argv.iter().position(|arg| arg == "--no-report") {
        argv.remove(idx);
    }
    let Some(name) = plan.env.get("LLVM_PROFILE_FILE_NAME").cloned() else {
        return;
    };
    let profile = plan.build_target.join(name);
    plan.env.insert(
        "LLVM_PROFILE_FILE".to_string(),
        profile.to_string_lossy().into_owned(),
    );
}
