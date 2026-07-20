use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_corpus_exercises_analysis_gate_paths() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    write_nested_repo(repo.path());
    let config = home.path().join("permissive.toml");
    fs::write(
        &config,
        "[gate]\n\
         test_coverage_threshold = 0\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
         \n\
         [python]\n\
         statements_per_function = 1000\n\
         positional_args = 1000\n\
         keyword_only_args = 1000\n\
         max_indentation = 1000\n\
         branches_per_function = 1000\n\
         local_variables = 1000\n\
         returns_per_function = 1000\n\
         return_values_per_function = 1000\n\
         nested_function_depth = 1000\n\
         statements_per_try_block = 1000\n\
         boolean_parameters = 1000\n\
         decorators_per_function = 1000\n\
         calls_per_function = 1000\n\
         methods_per_class = 1000\n\
         statements_per_file = 10000\n\
         lines_per_file = 10000\n\
         functions_per_file = 1000\n\
         interface_types_per_file = 1000\n\
         concrete_types_per_file = 1000\n\
         imported_names_per_file = 1000\n\
         cycle_size = 1000\n\
         indirect_dependencies = 10000\n\
         dependency_depth = 1000\n\
         \n\
         [rust]\n\
         statements_per_function = 1000\n\
         arguments = 1000\n\
         max_indentation = 1000\n\
         branches_per_function = 1000\n\
         local_variables = 1000\n\
         returns_per_function = 1000\n\
         nested_function_depth = 1000\n\
         boolean_parameters = 1000\n\
         attributes_per_function = 1000\n\
         calls_per_function = 1000\n\
         methods_per_class = 1000\n\
         statements_per_file = 10000\n\
         lines_per_file = 10000\n\
         functions_per_file = 1000\n\
         interface_types_per_file = 1000\n\
         concrete_types_per_file = 1000\n\
         imported_names_per_file = 1000\n\
         cycle_size = 1000\n\
         indirect_dependencies = 10000\n\
         dependency_depth = 1000\n",
    )
    .unwrap();

    let out = kiss_binary()
        .arg("stats")
        .arg("--config")
        .arg(&config)
        .arg("--all")
        .arg("10")
        .arg("--ignore")
        .arg("target")
        .arg(repo.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "real-repo analysis corpus should pass. stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("STAT:"),
        "stats --all should print machine-readable rows. stdout:\n{stdout}"
    );
}

fn write_nested_repo(repo: &std::path::Path) {
    fs::create_dir_all(repo.join("pkg").join("nested")).unwrap();
    fs::create_dir_all(repo.join("src").join("inner")).unwrap();
    fs::write(
        repo.join("pkg").join("__init__.py"),
        "from .nested.tool import choose\n",
    )
    .unwrap();
    fs::write(
        repo.join("pkg").join("nested").join("tool.py"),
        "import os\n\n\
         def choose(x):\n\
             try:\n\
                 if x:\n\
                     return os.path.basename(str(x))\n\
                 return 'empty'\n\
             except TypeError:\n\
                 return 'bad'\n",
    )
    .unwrap();
    fs::write(
        repo.join("main.py"),
        "from pkg import choose\n\n\
         def run():\n\
             return choose('value')\n",
    )
    .unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"nested_corpus\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("lib.rs"),
        "pub mod inner;\n\
         pub fn run(x: i32) -> i32 {\n\
             match x {\n\
                 0 => 0,\n\
                 n if n > 0 => inner::math::bump(n),\n\
                 n => n,\n\
             }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("inner").join("mod.rs"),
        "pub mod math;\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("inner").join("math.rs"),
        "pub fn bump(x: i32) -> i32 {\n\
             x + 1\n\
         }\n",
    )
    .unwrap();
}

#[test]
fn cli_corpus_exercises_mixed_repo_reporting_paths() {
    let repo = TempDir::new().unwrap();
    write_mixed_repo(repo.path());

    let mimic_out = repo.path().join("mimic.toml");
    let graph_out = repo.path().join("graph.mmd");
    for args in [
        vec!["stats", "--all", "5", "."],
        vec!["stats", "--table", "."],
        vec!["dry", "--shingle-size", "3", "--minhash-size", "8", "."],
        vec!["mimic", "--out", mimic_out.to_str().unwrap(), "."],
        vec!["viz", graph_out.to_str().unwrap(), "--zoom", "0.5", "."],
        vec!["rules"],
        vec!["config"],
    ] {
        let out = kiss_binary()
            .args(args)
            .current_dir(repo.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "corpus command should pass. stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    assert!(fs::read_to_string(mimic_out).unwrap().contains("[gate]"));
    assert!(fs::read_to_string(graph_out).unwrap().contains("graph"));
}

fn write_mixed_repo(repo: &std::path::Path) {
    fs::create_dir_all(repo.join("pkg")).unwrap();
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(
        repo.join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 0\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n",
    )
    .unwrap();
    fs::write(
        repo.join("app.py"),
        "import pkg.helper\n\n\
         class App:\n\
             def value(self, x):\n\
                 if x > 0:\n\
                     return pkg.helper.bump(x)\n\
                 return 0\n",
    )
    .unwrap();
    fs::write(
        repo.join("pkg").join("helper.py"),
        "def bump(x):\n\
             return x + 1\n",
    )
    .unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"coverage_corpus\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("lib.rs"),
        "mod math;\n\
         pub fn value(x: i32) -> i32 {\n\
             if x > 0 { math::bump(x) } else { 0 }\n\
         }\n",
    )
    .unwrap();
    fs::write(
        repo.join("src").join("math.rs"),
        "pub fn bump(x: i32) -> i32 {\n\
             x + 1\n\
         }\n",
    )
    .unwrap();
}
