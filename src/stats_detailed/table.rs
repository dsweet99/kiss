use super::types::UnitMetrics;

pub fn format_detailed_table(units: &[UnitMetrics]) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "{:<40} {:<20} {:<10} {:>5} {:>6} {:>5} {:>5} {:>5} {:>5} {:>6} {:>7} {:>5} {:>7} {:>6} {:>6}\n",
        "File",
        "Name",
        "Kind",
        "Line",
        "Stmts",
        "Args",
        "Ind",
        "Br",
        "Ret",
        "Locals",
        "Methods",
        "Lines",
        "Imports",
        "FanIn",
        "FanOut"
    );
    out.push_str(&"-".repeat(152));
    out.push('\n');
    for u in units {
        let fmt = |v: Option<usize>| v.map_or_else(|| "-".to_string(), |n| n.to_string());
        let _ = writeln!(
            out,
            "{:<40} {:<20} {:<10} {:>5} {:>6} {:>5} {:>5} {:>5} {:>5} {:>6} {:>7} {:>5} {:>7} {:>6} {:>6}",
            super::truncate(&u.file, 40),
            super::truncate(&u.name, 20),
            u.kind,
            u.line,
            fmt(u.statements),
            fmt(u.arguments),
            fmt(u.indentation),
            fmt(u.branches),
            fmt(u.returns),
            fmt(u.locals),
            fmt(u.methods),
            fmt(u.lines),
            fmt(u.imports),
            fmt(u.fan_in),
            fmt(u.fan_out)
        );
    }
    out
}
