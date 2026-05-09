use super::mv_harness::ScenarioRun;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct OracleBundle {
    pub compile_ok: bool,
    pub import_smoke_ok: bool,
    pub behavior_ok: bool,
    #[allow(dead_code)]
    pub messages: Vec<String>,
}

impl OracleBundle {
    pub const fn ok(&self) -> bool {
        self.compile_ok && self.import_smoke_ok && self.behavior_ok
    }
}

pub fn run_python_oracles(run: &ScenarioRun) -> OracleBundle {
    run_post_move_oracles_from_root(kiss::Language::Python, &run.root)
}

pub fn run_rust_oracles(run: &ScenarioRun) -> OracleBundle {
    run_post_move_oracles_from_root(kiss::Language::Rust, &run.root)
}

pub fn run_post_move_oracles_from_root(language: kiss::Language, root: &Path) -> OracleBundle {
    match language {
        kiss::Language::Python => {
            let py_compile = run_cmd(
                root,
                "python",
                &["-m", "compileall", "-q", "."],
                &[("PYTHONPATH", root.as_os_str())],
            );
            let import_smoke = run_cmd(
                root,
                "python",
                &["-m", "pytest", "--collect-only", "-q"],
                &[("PYTHONPATH", root.as_os_str())],
            );
            let behavior = run_cmd(
                root,
                "python",
                &["-m", "pytest", "-q"],
                &[("PYTHONPATH", root.as_os_str())],
            );
            build_python_bundle(&py_compile, &import_smoke, &behavior)
        }
        kiss::Language::Rust => {
            let check = run_cmd(root, "cargo", &["check", "--quiet"], &[]);
            let test = run_cmd(root, "cargo", &["test", "--quiet"], &[]);
            build_rust_bundle(&check, &test)
        }
    }
}

fn build_python_bundle(
    py_compile: &CommandOutcome,
    import_smoke: &CommandOutcome,
    behavior: &CommandOutcome,
) -> OracleBundle {
    let mut messages = Vec::new();
    if !py_compile.success {
        messages.push(format!("compileall failed:\n{}", py_compile.output));
    }
    if !import_smoke.success {
        messages.push(format!(
            "pytest collection failed:\n{}",
            import_smoke.output
        ));
    }
    if !behavior.success {
        messages.push(format!("pytest run failed:\n{}", behavior.output));
    }
    OracleBundle {
        compile_ok: py_compile.success,
        import_smoke_ok: import_smoke.success,
        behavior_ok: behavior.success,
        messages,
    }
}

fn build_rust_bundle(cargo_check: &CommandOutcome, cargo_test: &CommandOutcome) -> OracleBundle {
    let mut messages = Vec::new();
    if !cargo_check.success {
        messages.push(format!("cargo check failed:\n{}", cargo_check.output));
    }
    if !cargo_test.success {
        messages.push(format!("cargo test failed:\n{}", cargo_test.output));
    }
    OracleBundle {
        compile_ok: cargo_check.success,
        import_smoke_ok: true,
        behavior_ok: cargo_test.success,
        messages,
    }
}

#[derive(Debug)]
struct CommandOutcome {
    success: bool,
    output: String,
}

fn run_cmd(root: &Path, program: &str, args: &[&str], envs: &[(&str, &OsStr)]) -> CommandOutcome {
    let mut command = Command::new(program);
    command.args(args).current_dir(root);
    for &(key, value) in envs {
        command.env(key, value);
    }
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            CommandOutcome {
                success: output.status.success(),
                output: format!("{stdout}{stderr}"),
            }
        }
        Err(err) => CommandOutcome {
            success: false,
            output: err.to_string(),
        },
    }
}
