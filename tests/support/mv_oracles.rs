#![allow(dead_code)]

use super::mv_harness::ScenarioRun;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct OracleBundle {
    pub py_compile_ok: bool,
    pub import_smoke_ok: bool,
    pub behavior_ok: bool,
    pub messages: Vec<String>,
}

impl OracleBundle {
    pub const fn ok(&self) -> bool {
        self.py_compile_ok && self.import_smoke_ok && self.behavior_ok
    }
}

pub fn run_python_oracles(run: &ScenarioRun) -> OracleBundle {
    let py_compile = run_python_command(run, &["-m", "compileall", "-q", "."]);
    let import_smoke = run_python_command(run, &["-m", "pytest", "--collect-only", "-q"]);
    let behavior = run_python_command(run, &["-m", "pytest", "-q"]);

    let mut messages = Vec::new();
    if !py_compile.success {
        messages.push(format!("compileall failed:\n{}", py_compile.output));
    }
    if !import_smoke.success {
        messages.push(format!("pytest collection failed:\n{}", import_smoke.output));
    }
    if !behavior.success {
        messages.push(format!("pytest run failed:\n{}", behavior.output));
    }

    OracleBundle {
        py_compile_ok: py_compile.success,
        import_smoke_ok: import_smoke.success,
        behavior_ok: behavior.success,
        messages,
    }
}

pub fn run_rust_oracles(run: &ScenarioRun) -> OracleBundle {
    let cargo_check = run_command(run, "cargo", &["check", "--quiet"], &[]);
    let cargo_test = run_command(run, "cargo", &["test", "--quiet"], &[]);

    let mut messages = Vec::new();
    if !cargo_check.success {
        messages.push(format!("cargo check failed:\n{}", cargo_check.output));
    }
    if !cargo_test.success {
        messages.push(format!("cargo test failed:\n{}", cargo_test.output));
    }

    OracleBundle {
        py_compile_ok: true,
        import_smoke_ok: cargo_check.success,
        behavior_ok: cargo_test.success,
        messages,
    }
}

pub fn run_post_move_oracles_from_root(language: kiss::Language, root: &Path) -> OracleBundle {
    match language {
        kiss::Language::Python => {
            let py_compile =
                run_root_command(root, "python", &["-m", "compileall", "-q", "."], true);
            let import_smoke = run_root_command(
                root,
                "python",
                &["-m", "pytest", "--collect-only", "-q"],
                true,
            );
            let behavior = run_root_command(root, "python", &["-m", "pytest", "-q"], true);
            let mut messages = Vec::new();
            if !py_compile.success {
                messages.push(format!("compileall failed:\n{}", py_compile.output));
            }
            if !import_smoke.success {
                messages.push(format!("pytest collection failed:\n{}", import_smoke.output));
            }
            if !behavior.success {
                messages.push(format!("pytest run failed:\n{}", behavior.output));
            }
            OracleBundle {
                py_compile_ok: py_compile.success,
                import_smoke_ok: import_smoke.success,
                behavior_ok: behavior.success,
                messages,
            }
        }
        kiss::Language::Rust => {
            let cargo_check = run_root_command(root, "cargo", &["check", "--quiet"], false);
            let cargo_test = run_root_command(root, "cargo", &["test", "--quiet"], false);
            let mut messages = Vec::new();
            if !cargo_check.success {
                messages.push(format!("cargo check failed:\n{}", cargo_check.output));
            }
            if !cargo_test.success {
                messages.push(format!("cargo test failed:\n{}", cargo_test.output));
            }
            OracleBundle {
                py_compile_ok: true,
                import_smoke_ok: cargo_check.success,
                behavior_ok: cargo_test.success,
                messages,
            }
        }
    }
}

#[derive(Debug)]
struct CommandOutcome {
    success: bool,
    output: String,
}

fn run_python_command(run: &ScenarioRun, args: &[&str]) -> CommandOutcome {
    run_command(run, "python", args, &[("PYTHONPATH", run.root.as_os_str())])
}

fn run_root_command(
    root: &Path,
    program: &str,
    args: &[&str],
    set_pythonpath: bool,
) -> CommandOutcome {
    let mut command = Command::new(program);
    command.args(args).current_dir(root);
    if set_pythonpath {
        command.env("PYTHONPATH", root);
    }
    let output = command.output();
    match output {
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

fn run_command(
    run: &ScenarioRun,
    program: &str,
    args: &[&str],
    envs: &[(&str, &OsStr)],
) -> CommandOutcome {
    let mut command = Command::new(program);
    command.args(args).current_dir(&run.root);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output();
    match output {
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
