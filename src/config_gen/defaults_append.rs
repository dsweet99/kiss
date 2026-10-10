use std::fmt::Write;

pub fn append_python_defaults(out: &mut String) {
    use crate::defaults::{graph, python};
    let _ = writeln!(out, "[python]");
    let _ = writeln!(out, "max_num_tests = {}", python::MAX_NUM_TESTS);
    let _ = writeln!(
        out,
        "num_jobs_pytest = {}",
        crate::defaults::gate::NUM_JOBS_PYTEST
    );
    let _ = writeln!(
        out,
        "statements_per_function = {}",
        python::STATEMENTS_PER_FUNCTION
    );
    let _ = writeln!(out, "positional_args = {}", python::POSITIONAL_ARGS);
    let _ = writeln!(out, "keyword_only_args = {}", python::KEYWORD_ONLY_ARGS);
    let _ = writeln!(out, "max_indentation = {}", python::MAX_INDENTATION);
    let _ = writeln!(
        out,
        "branches_per_function = {}",
        python::BRANCHES_PER_FUNCTION
    );
    let _ = writeln!(out, "local_variables = {}", python::LOCAL_VARIABLES);
    let _ = writeln!(out, "methods_per_class = {}", python::METHODS_PER_CLASS);
    let _ = writeln!(
        out,
        "nested_function_depth = {}",
        python::NESTED_FUNCTION_DEPTH
    );
    let _ = writeln!(
        out,
        "returns_per_function = {}",
        python::RETURNS_PER_FUNCTION
    );
    let _ = writeln!(
        out,
        "return_values_per_function = {}",
        python::RETURN_VALUES_PER_FUNCTION
    );
    let _ = writeln!(out, "statements_per_file = {}", python::STATEMENTS_PER_FILE);
    let _ = writeln!(out, "lines_per_file = {}", python::LINES_PER_FILE);
    let _ = writeln!(out, "functions_per_file = {}", python::FUNCTIONS_PER_FILE);
    let _ = writeln!(
        out,
        "interface_types_per_file = {}",
        python::INTERFACE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "concrete_types_per_file = {}",
        python::CONCRETE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "imported_names_per_file = {}",
        python::IMPORTS_PER_FILE
    );
    let _ = writeln!(
        out,
        "indirect_dependencies = {}",
        python::INDIRECT_DEPENDENCIES
    );
    let _ = writeln!(out, "dependency_depth = {}", python::DEPENDENCY_DEPTH);
    let _ = writeln!(
        out,
        "statements_per_try_block = {}",
        python::STATEMENTS_PER_TRY_BLOCK
    );
    let _ = writeln!(out, "boolean_parameters = {}", python::BOOLEAN_PARAMETERS);
    let _ = writeln!(
        out,
        "decorators_per_function = {}",
        python::DECORATORS_PER_FUNCTION
    );
    let _ = writeln!(out, "calls_per_function = {}", python::CALLS_PER_FUNCTION);
    let _ = writeln!(out, "cycle_size = {}\n", graph::CYCLE_SIZE);
}

pub fn append_rust_defaults(out: &mut String) {
    use crate::defaults::{graph, rust};
    let _ = writeln!(out, "[rust]");
    let _ = writeln!(out, "max_num_tests = {}", rust::MAX_NUM_TESTS);
    let _ = writeln!(
        out,
        "num_jobs_nextest = {}",
        crate::defaults::gate::NUM_JOBS_NEXTEST
    );
    let _ = writeln!(
        out,
        "statements_per_function = {}",
        rust::STATEMENTS_PER_FUNCTION
    );
    let _ = writeln!(out, "arguments = {}", rust::ARGUMENTS);
    let _ = writeln!(out, "max_indentation = {}", rust::MAX_INDENTATION);
    let _ = writeln!(
        out,
        "branches_per_function = {}",
        rust::BRANCHES_PER_FUNCTION
    );
    let _ = writeln!(out, "local_variables = {}", rust::LOCAL_VARIABLES);
    let _ = writeln!(out, "methods_per_class = {}", rust::METHODS_PER_TYPE);
    let _ = writeln!(
        out,
        "nested_function_depth = {}",
        rust::NESTED_FUNCTION_DEPTH
    );
    let _ = writeln!(out, "returns_per_function = {}", rust::RETURNS_PER_FUNCTION);
    let _ = writeln!(out, "statements_per_file = {}", rust::STATEMENTS_PER_FILE);
    let _ = writeln!(out, "lines_per_file = {}", rust::LINES_PER_FILE);
    let _ = writeln!(out, "functions_per_file = {}", rust::FUNCTIONS_PER_FILE);
    let _ = writeln!(
        out,
        "interface_types_per_file = {}",
        rust::INTERFACE_TYPES_PER_FILE
    );
    let _ = writeln!(
        out,
        "concrete_types_per_file = {}",
        rust::CONCRETE_TYPES_PER_FILE
    );
    let _ = writeln!(out, "imported_names_per_file = {}", rust::IMPORTS_PER_FILE);
    let _ = writeln!(
        out,
        "indirect_dependencies = {}",
        rust::INDIRECT_DEPENDENCIES
    );
    let _ = writeln!(out, "dependency_depth = {}", rust::DEPENDENCY_DEPTH);
    let _ = writeln!(out, "boolean_parameters = {}", rust::BOOLEAN_PARAMETERS);
    let _ = writeln!(
        out,
        "attributes_per_function = {}",
        rust::ATTRIBUTES_PER_FUNCTION
    );
    let _ = writeln!(out, "calls_per_function = {}", rust::CALLS_PER_FUNCTION);
    let _ = writeln!(out, "cycle_size = {}\n", graph::CYCLE_SIZE);
}

pub fn strip_test_section_max_num_tests(text: &str) -> String {
    let mut out = String::new();
    let mut in_test = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_test = trimmed == "[test]";
        }
        if in_test && trimmed.starts_with("max_num_tests") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

pub fn with_default_language_sections(text: &str) -> String {
    let (text, moved) = take_test_job_keys(text);
    let text = strip_test_section_max_num_tests(&text);
    let Ok(table) = text.parse::<toml::Table>() else {
        return place_moved_job_keys(&text, &moved);
    };
    let mut extra = String::new();
    if !table.contains_key("python") {
        append_python_defaults(&mut extra);
    }
    if !table.contains_key("rust") {
        append_rust_defaults(&mut extra);
    }
    let mut out = text.to_string();
    if !extra.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&extra);
    }
    place_moved_job_keys(&out, &moved)
}

struct MovedTestJobs {
    pytest: Option<String>,
    nextest: Option<String>,
    llvm_cov: Option<String>,
}

fn take_test_job_keys(text: &str) -> (String, MovedTestJobs) {
    let mut out = String::new();
    let mut moved = MovedTestJobs {
        pytest: None,
        nextest: None,
        llvm_cov: None,
    };
    let mut in_test = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_test = trimmed == "[test]";
        }
        if in_test {
            if let Some(value) = assignment_value(trimmed, "num_jobs_pytest") {
                moved.pytest = Some(value.to_string());
                continue;
            }
            if let Some(value) = assignment_value(trimmed, "num_jobs_nextest") {
                moved.nextest = Some(value.to_string());
                continue;
            }
            if let Some(value) = assignment_value(trimmed, "num_jobs_llvm_cov") {
                moved.llvm_cov = Some(value.to_string());
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    (out, moved)
}

fn assignment_value<'a>(trimmed: &'a str, key: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(key)?;
    if !rest.starts_with('=') && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('=')?;
    Some(rest.trim())
}

fn place_moved_job_keys(text: &str, moved: &MovedTestJobs) -> String {
    let mut out = text.to_string();
    if let Some(value) = &moved.pytest {
        out = place_section_key(&out, "[python]", "num_jobs_pytest", value);
    }
    let nextest = moved.nextest.as_ref().or(moved.llvm_cov.as_ref());
    if let Some(value) = nextest {
        out = place_section_key(&out, "[rust]", "num_jobs_nextest", value);
    }
    out
}

fn place_section_key(text: &str, header: &str, key: &str, value: &str) -> String {
    let new_line = format!("{key} = {value}");
    let mut out = String::new();
    let mut in_section = false;
    let mut placed = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_section && !placed {
                out.push_str(&new_line);
                out.push('\n');
                placed = true;
            }
            in_section = trimmed == header;
        }
        if in_section && assignment_value(trimmed, key).is_some() {
            out.push_str(&new_line);
            out.push('\n');
            placed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_section && !placed {
        out.push_str(&new_line);
        out.push('\n');
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::with_default_language_sections;

    #[test]
    fn fill_moves_test_job_keys_into_language_sections() {
        let filled = with_default_language_sections(
            "[test]\nnum_jobs = 3\nnum_jobs_pytest = 9\nnum_jobs_llvm_cov = 7\n",
        );
        assert!(
            filled.contains("num_jobs = 3"),
            "shared num_jobs must stay under [test]:\n{filled}"
        );
        assert!(
            filled.contains("num_jobs_pytest = 9"),
            "pytest jobs must move under [python]:\n{filled}"
        );
        assert!(
            filled.contains("num_jobs_nextest = 7"),
            "llvm-cov alias must move under [rust] as num_jobs_nextest:\n{filled}"
        );
        assert!(
            !filled.contains("num_jobs_llvm_cov"),
            "alias must not remain:\n{filled}"
        );
        let test_part = filled.split("[python]").next().unwrap_or("");
        assert!(
            !test_part.contains("num_jobs_pytest") && !test_part.contains("num_jobs_nextest"),
            "moved keys must leave [test]:\n{filled}"
        );
    }
}
