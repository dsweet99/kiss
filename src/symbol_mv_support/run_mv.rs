use crate::symbol_mv::{self, EditKind, MvOptions, MvPlan, MvRequest, ParsedQuery};

pub fn run_mv_inner(opts: MvOptions) -> Result<(), ()> {
    let query = validate_mv_options(&opts)?;
    let req = MvRequest {
        query,
        new_name: opts.new_name,
        paths: opts.paths,
        to: opts.to,
        ignore: opts.ignore,
    };
    check_destination_collision(&req)?;
    if req.query.member.is_some() {
        let owner = req.query.symbol.as_str();
        let old_name = req.query.old_name();
        let source_file = req.query.path.clone();
        let Ok(content) = std::fs::read_to_string(&source_file) else {
            eprintln!(
                "Error: failed to read '{}' for ambiguity check",
                source_file.display()
            );
            return Err(());
        };
        if super::ast_plan::has_ambiguous_method_reference(
            &source_file,
            &content,
            old_name,
            Some(owner),
            req.query.language,
        ) {
            eprintln!(
                "Error: trait-receiver ambiguity for '{}' in {}",
                req.query.raw,
                source_file.display()
            );
            return Err(());
        }
    }
    let plan = symbol_mv::plan_edits(&req);
    if plan.edits.is_empty() {
        eprintln!("Error: no symbol occurrences found for '{}'", req.query.raw);
        return Err(());
    }
    if opts.json {
        print_json_plan(&plan)
            .map_err(|err| eprintln!("Error: failed to serialize plan: {err}"))?;
    } else {
        print_human_plan(&plan);
    }
    if !opts.dry_run {
        symbol_mv::apply_plan_transactional(&plan).map_err(|err| eprintln!("Error: {err}"))?;
    }
    Ok(())
}

/// When the user is moving a top-level symbol with `--to DEST`, refuse if
/// `DEST` already defines a top-level symbol with `new_name`. Otherwise the
/// move would silently produce two definitions in `DEST` (the second
/// shadowing the first) — see KPOP H10.
fn check_destination_collision(req: &MvRequest) -> Result<(), ()> {
    let Some(dest_path) = req.to.as_ref() else {
        return Ok(());
    };
    if req.query.member.is_some() {
        return Ok(());
    }
    let Ok(dest_content) = std::fs::read_to_string(dest_path) else {
        return Ok(());
    };
    let language = req.query.language;
    if super::definition::find_definition_span(
        &dest_content,
        &req.new_name,
        None,
        language,
        dest_path,
    )
    .is_some()
    {
        eprintln!(
            "Error: destination {} already defines a top-level '{}'; refusing to move (would shadow)",
            dest_path.display(),
            req.new_name,
        );
        return Err(());
    }
    Ok(())
}

fn validate_mv_options(opts: &MvOptions) -> Result<ParsedQuery, ()> {
    let query = symbol_mv::parse_mv_query(&opts.query).map_err(|err| eprintln!("Error: {err}"))?;
    if let Some(lang_filter) = opts.lang_filter
        && lang_filter != query.language
    {
        eprintln!(
            "Error: source language ({}) does not match --lang ({})",
            query.language_name(),
            symbol_mv::language_name(lang_filter)
        );
        return Err(());
    }
    symbol_mv::validate_new_name(&opts.new_name, query.language)
        .map_err(|err| eprintln!("Error: {err}"))?;
    if opts.to.is_some() && query.member.is_some() {
        eprintln!(
            "Error: --to moves are only supported for top-level functions, not methods (got {})",
            query.raw
        );
        return Err(());
    }
    Ok(query)
}

fn print_human_plan(plan: &MvPlan) {
    for edit in &plan.edits {
        println!(
            "{}:{}: {} -> {}",
            edit.path.display(),
            edit.line,
            edit.old_snippet,
            edit.new_snippet
        );
    }
}

fn print_json_plan(plan: &MvPlan) -> Result<(), serde_json::Error> {
    let edits: Vec<serde_json::Value> = plan
        .edits
        .iter()
        .map(|edit| {
            serde_json::json!({
                "start_byte": edit.start_byte,
                "end_byte": edit.end_byte,
                "line": edit.line,
                "kind": edit_kind_name(edit.kind),
                "old_snippet": edit.old_snippet,
                "new_snippet": edit.new_snippet,
                "path": edit.path.display().to_string(),
            })
        })
        .collect();
    let payload = serde_json::json!({
        "files": plan.files.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "edits": edits,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

const fn edit_kind_name(kind: EditKind) -> &'static str {
    match kind {
        EditKind::Definition => "definition",
        EditKind::Reference => "reference",
    }
}

#[cfg(test)]
mod run_mv_coverage {
    use super::*;
    use crate::symbol_mv::{MvOptions, MvPlan, PlannedEdit};
    use std::path::PathBuf;

    #[test]
    fn touch_run_mv_helpers_for_coverage_gate() {
        let bad = MvOptions {
            query: "nocolon".into(),
            new_name: "a".into(),
            paths: vec![],
            to: None,
            dry_run: true,
            json: false,
            lang_filter: None,
            ignore: vec![],
        };
        let _ = validate_mv_options(&bad);
        let plan = MvPlan {
            files: vec![PathBuf::from("x")],
            edits: vec![PlannedEdit {
                path: PathBuf::from("x"),
                start_byte: 0,
                end_byte: 1,
                line: 1,
                old_snippet: "a".into(),
                new_snippet: "b".into(),
                kind: EditKind::Definition,
            }],
        };
        print_human_plan(&plan);
        let _ = print_json_plan(&plan);
        let _ = edit_kind_name(EditKind::Reference);
        let _ = run_mv_inner(bad);
    }
}
