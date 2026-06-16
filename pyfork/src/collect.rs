use std::path::Path;
use std::process::Command;

use crate::flags::validate_pytest_extra;

pub fn collect_nodeids(repo_root: &Path, extra: &[String]) -> Result<Vec<String>, String> {
    validate_pytest_extra(extra)?;
    let mut cmd = Command::new("python");
    cmd.args(["-m", "pytest", "--collect-only", "-q"]);
    cmd.args(extra);
    cmd.current_dir(repo_root);
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run pytest collection: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit_code = output.status.code().unwrap_or(-1);
    if !output.status.success() && exit_code != 5 {
        return Err(format!(
            "pytest collection failed\n{stdout}{stderr}"
        ));
    }
    let nodeids: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line.contains("::"))
        .map(str::to_string)
        .collect();
    Ok(nodeids)
}
