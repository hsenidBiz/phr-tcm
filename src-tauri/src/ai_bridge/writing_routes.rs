//! Writing test cases: optimize, transform, begin, validate, coverage and merge.

use super::*;

/// Reorganise a draft into a run sheet: navigation spelled out as steps,
/// cases ordered so the tester switches environment as little as
/// possible, expected results reduced to the outcome. Azure DevOps is
/// never touched.
///
/// Round 7 §1: the draft may come as a `path` instead of inline JSON -
/// the run-sheet ordering is inherently GLOBAL (one tester_order across
/// the whole set), so this was the one tool where "too big to inline"
/// could not be chunked around: five shards give five sequences all
/// starting at 1, a wrong answer that looks like an answer. `path` and
/// `in_place` match transform_cases exactly; with `in_place: true` the
/// response is the report alone, which removes the return half of the
/// cost as well as the send half.
pub(crate) fn optimize_json(body: &str, target: &str, ctx: &BridgeContext) -> (u16, String) {
    let from_path = q(target, "path").filter(|p| !p.trim().is_empty());
    let in_place = matches!(q(target, "in_place").as_deref(), Some("true") | Some("1"));
    if from_path.is_some() && !body.trim().is_empty() {
        return (
            400,
            serde_json::json!({ "error": "pass the draft as \"json\" OR as \"path\", not both - \
                a silently preferred source is how the wrong draft gets optimized." })
            .to_string(),
        );
    }
    if in_place && from_path.is_none() {
        return (
            400,
            serde_json::json!({ "error": "in_place needs a \"path\" - there is no file to write back to." })
                .to_string(),
        );
    }
    // An in-place rewrite is a read-patch-write, like a comment save, and a
    // comment autosave on the same file must not interleave with it. Take
    // the same lock BEFORE the read and hold it until the write is done.
    let _serialised = in_place
        .then(|| crate::filewatch::NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner()));
    // The exact text the cases were parsed from. The write-back merges into
    // this, never into a second read of a file that may have moved on.
    let mut draft_text: Option<String> = None;
    // Warnings are not failures - a long title or a comma in a tag is worth
    // saying and not worth refusing over - but they must reach the caller,
    // because some of them mean a case was dropped.
    let (cases, import_warnings) = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path} does not exist or is not a file") })
                        .to_string(),
                );
            }
            match crate::import_parser::read_draft(path) {
                Ok((text, v)) => {
                    draft_text = Some(text);
                    (v.cases, v.warnings)
                }
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
        None => match parse_cases_with_warnings(body) {
            Ok(v) => v,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        },
    };
    let file_specs = draft_text
        .as_deref()
        .map(crate::import_parser::specs::read_specs)
        .unwrap_or_default();
    let entry = q(target, "entry");
    let dry_run = matches!(q(target, "dry_run").as_deref(), Some("true") | Some("1"));
    // Default true: regrouping for the tester is what this tool is mostly
    // for. `reorder=false` is what a spec-ordered set passes, so it still
    // gets the navigation and expected-result work without being shuffled.
    let reorder = !matches!(q(target, "reorder").as_deref(), Some("false") | Some("0"));
    // Default true as well. Round 8 §14: an arithmetic set wants the
    // navigation and ordering work WITHOUT its expected results rewritten,
    // and welding the two together made the whole tool unusable there - the
    // reported workaround was to skip it entirely and lose both halves.
    let trim_expected =
        !matches!(q(target, "trim_expected").as_deref(), Some("false") | Some("0"));
    let (optimized, report) =
        crate::optimize::optimize_full(cases, entry.as_deref(), reorder, trim_expected);
    if dry_run {
        // Report only: the caller inspects what WOULD change before
        // committing to the transformed JSON.
        return (
            200,
            serde_json::json!({
                "report": report,
                "import_warnings": import_warnings,
                "note": "Dry run - no test_cases returned. Call again without dry_run to get the transformed JSON.",
            })
            .to_string(),
        );
    }
    let json = match draft_text_for(draft_text.as_deref(), &optimized) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };
    if in_place {
        // Temp file + rename: a half-written draft under a watched path is
        // worse than no write.
        let path = from_path.expect("guarded above");
        if !bridge_may_write(&path, ctx.working_dir.as_deref(), &watched_now()) {
            return (400, serde_json::json!({ "error": IN_PLACE_REFUSAL }).to_string());
        }
        if let Err(e) = crate::ai_tools::atomic_write(std::path::Path::new(&path), &json) {
            return (500, serde_json::json!({ "error": e }).to_string());
        }
        return (
            200,
            serde_json::json!({
                "written_to": path,
                "cases": optimized.len(),
                "report": report,
                "import_warnings": import_warnings,
                "note": "The optimized draft was written back in place - no JSON is echoed. Every case now carries spec_order and tester_order; the app's file watch will pick the change up.",
            })
            .to_string(),
        );
    }
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    let mut response = serde_json::json!({
        "test_cases": doc.get("test_cases").cloned().unwrap_or(doc),
        "report": report,
        // Anything the importer could not read. A case it skipped is
        // simply not in the output, so silence here read as success
        // over a draft that had quietly got shorter.
        "import_warnings": import_warnings,
        "note": "Hand this JSON to the developer as the file to import. Every case now carries spec_order and tester_order - keep both fields exactly as set; the app flips the queue between the two readings. The report explains what was reordered and why. Check import_warnings - a case listed there was NOT read and is not in this output.",
    });
    // The file's `specs` travel with the cases the caller writes back.
    if !file_specs.is_empty() {
        response["specs"] = serde_json::json!(file_specs);
    }
    (200, response.to_string())
}

/// Apply declarative edits to a draft - the restructuring an assistant
/// would otherwise write a throwaway script for.
///
/// Round 5 §10: the draft may come as a `path` instead of inline JSON -
/// the same escape `validate_cases` has, because a 245 KB draft inlined
/// both ways is a 489 KB round-trip to reword some steps, and sharding it
/// forces the hand-rolled reassembly the guide forbids. `path` and inline
/// together is a 400 naming both - the old behaviour, building the result
/// from the inline draft while the path sat ignored, is the exact silent-
/// discard §15 condemns. `in_place: true` writes the transformed draft
/// back to the path atomically and skips echoing the JSON, which for a
/// bulk retag is the whole cost.
pub(crate) fn transform_json(body: &str, ctx: &BridgeContext) -> (u16, String) {
    let doc: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, serde_json::json!({ "error": format!("invalid JSON: {e}") }).to_string()),
    };
    let from_path = doc["path"].as_str().filter(|p| !p.trim().is_empty()).map(str::to_string);
    let has_inline = !doc["test_cases"].is_null();
    let in_place = doc["in_place"].as_bool().unwrap_or(false);
    if from_path.is_some() && has_inline {
        return (
            400,
            serde_json::json!({ "error": "pass the draft as \"json\" OR as \"path\", not both - \
                a silently preferred source is how edits land on the wrong draft." })
            .to_string(),
        );
    }
    if in_place && from_path.is_none() {
        return (
            400,
            serde_json::json!({ "error": "in_place needs a \"path\" - there is no file to write back to." })
                .to_string(),
        );
    }

    // Same lock as a comment save, taken before the read (see optimize_json).
    let _serialised = in_place
        .then(|| crate::filewatch::NOTE_WRITE.lock().unwrap_or_else(|e| e.into_inner()));
    let mut draft_text: Option<String> = None;
    let (cases, import_warnings) = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path} does not exist or is not a file") })
                        .to_string(),
                );
            }
            match crate::import_parser::read_draft(path) {
                Ok((text, v)) => {
                    draft_text = Some(text);
                    (v.cases, v.warnings)
                }
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
        None => {
            // The draft arrives as a JSON *string* (the tool's `json`
            // argument), but a caller posting the array inline works too.
            let draft = match &doc["test_cases"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => {
                    return (
                        400,
                        serde_json::json!({ "error": "empty draft - pass the JSON as \"json\", or a local file via \"path\" for large drafts" })
                            .to_string(),
                    )
                }
                other => other.to_string(),
            };
            match parse_cases_with_warnings(&draft) {
                Ok(v) => v,
                Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
            }
        }
    };
    let (ops, mut ignored) = match crate::transform::parse_ops_full(&doc["operations"]) {
        Ok(o) => o,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };
    // Body arguments this route does not read - echoed, not swallowed.
    if let Some(obj) = doc.as_object() {
        for k in obj.keys() {
            if !["test_cases", "operations", "path", "in_place"].contains(&k.as_str()) {
                ignored.push(format!("request argument \"{k}\" is not read by transform_cases."));
            }
        }
    }
    let (out, mut report) = crate::transform::apply(cases, &ops);
    report.ignored.extend(ignored);
    let file_specs = draft_text
        .as_deref()
        .map(crate::import_parser::specs::read_specs)
        .unwrap_or_default();
    let json = match draft_text_for(draft_text.as_deref(), &out) {
        Ok(j) => j,
        Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
    };

    if in_place {
        // Temp file + rename: a half-written draft under a watched path is
        // worse than no write.
        let path = from_path.expect("guarded above");
        if !bridge_may_write(&path, ctx.working_dir.as_deref(), &watched_now()) {
            return (400, serde_json::json!({ "error": IN_PLACE_REFUSAL }).to_string());
        }
        if let Err(e) = crate::ai_tools::atomic_write(std::path::Path::new(&path), &json) {
            return (500, serde_json::json!({ "error": e }).to_string());
        }
        return (
            200,
            serde_json::json!({
                "written_to": path,
                "cases": out.len(),
                "report": report,
                "import_warnings": import_warnings,
                "note": "The transformed draft was written back in place - no JSON is echoed. The app's file watch will pick the change up.",
            })
            .to_string(),
        );
    }

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    let mut response = serde_json::json!({
        "test_cases": parsed.get("test_cases").cloned().unwrap_or(parsed),
        "report": report,
        "import_warnings": import_warnings,
    });
    // The caller writes the file back itself, so the file's `specs` travel
    // with the cases - or the rewrite would drop them. Absent, not empty,
    // when the draft came inline: nothing to carry.
    if !file_specs.is_empty() {
        response["specs"] = serde_json::json!(file_specs);
    }
    (200, response.to_string())
}

/// The text to write back over a draft file: the new cases inside the
/// file's OWN document (`old` is the exact text they were parsed from), so
/// its `specs`, its whole-set `comments` and any key this app has never
/// heard of survive the rewrite - a tool owns the cases, never the file.
/// Without a file (the draft came inline) it is the standard wrapper.
pub(crate) fn draft_text_for(old: Option<&str>, cases: &[crate::model::TestCase]) -> Result<String, String> {
    match old {
        // A `specs` entry the rule refuses is taken out on the way back, so
        // a tool's write never keeps one. The importer has already warned
        // about each by name.
        Some(old) => crate::import_parser::merge_cases_into_draft(old, cases)
            .and_then(|t| crate::import_parser::specs::without_refused_specs(&t)),
        None => crate::import_parser::queue_to_json_string(cases),
    }
}

/// Hand control to the developer before a single case is written.
///
/// Phase 1 (empty body): the checklist to put to them in chat, with the
/// context this app already knows - org/project, the org's real Module
/// values, a suggested output folder - so they correct defaults instead
/// of composing answers from nothing.
///
/// Phase 2 (answers in the body): the answers are CHECKED - spec files
/// must exist on disk, the output folder must exist, the module must be
/// a real one - and only then is a plan file written. An assistant that
/// invents a plausible path is caught by name here, which is most of
/// what a file picker would have bought.
pub(crate) async fn begin_writing(
    caller: &crate::autorun::agents::Caller,
    body: &str,
    target: &str,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
) -> (u16, String) {
    let feature = q(target, "feature").unwrap_or_default();
    // The tri-state is kept, not flattened. `.known()` returns an empty
    // list for BOTH "this organization has no Module field" and "the
    // request for its values failed", and `problems` skips the module
    // check entirely on an empty list - so a failed fetch silently turned
    // off the very check this tool exists for and still said "ready".
    let allowed = match client {
        Some(c) => allowed_modules(ctx, c).await,
        None => Modules::Unavailable("not signed in to Test Case Manager".into()),
    };
    let modules: Vec<String> = allowed.known().to_vec();

    // No repository, no job: the whole point of the intake is agreeing
    // where the file goes, and that place is now `<repo>/.test-cases`.
    let root = ctx
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let Some(root) = root else {
        return (
            409,
            serde_json::json!({
                "status": "blocked",
                "error": "No working repository is set in Test Case Manager. Ask the developer to open the AI Bridge tab and pick the repository these cases belong to, then call this tool again.",
            })
            .to_string(),
        );
    };
    let cases_dir = match crate::workspace::ensure_cases_dir(&root) {
        Ok(d) => d,
        Err(e) => {
            return (500, serde_json::json!({ "status": "error", "error": e }).to_string());
        }
    };
    let cases_dir_str = cases_dir.to_string_lossy().to_string();

    // Phase 1: nothing sent, so hand back the questions.
    if body.trim().is_empty() || body.trim() == "{}" {
        return (
            200,
            serde_json::json!({
                "status": "questions",
                "feature": feature,
                "ask_the_developer": crate::intake::questions_for(Some(&cases_dir_str)),
                "context": {
                    "organization": ctx.org,
                    "project": ctx.project,
                    "allowed_modules": modules,
                    "output_dir": cases_dir_str,
                    "suggested_output_path": crate::workspace::default_output_path(&root, &feature),
                    "tags_hint": "Call get_tags (with a query filter) to reuse existing tags.",
                },
                "note": "Put these to the developer in chat - one at a time, and let them \
                         paste or drop file paths. Do NOT answer them yourself, and do not \
                         start writing. If the job is for a PBI and you are the assistant the \
                         person talks to, call `get_current_pbi` first: when the developer \
                         named no PBI and TCM has one selected, say \
                         \"PBI <id>, <title>, is selected in TCM - is that the one?\" and wait \
                         for their yes; when they named a different one, ask which to use; \
                         when TCM has none, find it with `search_pbis` or ask for the id. A \
                         subagent never asks: it uses the PBI id it was given. When they have \
                         answered, call this tool again with \
                         their answers to get the plan.",
            })
            .to_string(),
        );
    }

    let mut answers: crate::intake::IntakeAnswers = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "status": "error", "error": format!("could not read the answers: {e}") })
                    .to_string(),
            )
        }
    };

    answers.output_path = crate::workspace::resolve_output(&root, &answers.output_path);
    let problems = crate::intake::problems_in(&answers, &modules, Some(&cases_dir));
    if !problems.is_empty() {
        return (
            200,
            serde_json::json!({
                "status": "needs_answers",
                "problems": problems,
                "allowed_modules": modules,
                "note": "Go back to the developer with these - do not guess a value or a \
                         path to get past them, and do not start writing.",
            })
            .to_string(),
        );
    }

    // Not a `problem` - those block, and a transient fetch failure must not
    // stop a developer whose module is perfectly correct. But it has to be
    // SAID, or "ready" claims a check that never ran.
    let mut unchecked: Vec<String> = vec![];
    if let Modules::Unavailable(why) = &allowed {
        if !answers.module.trim().is_empty() {
            unchecked.push(format!(
                "module '{}' could not be checked against this organization's allowed values ({why}) - confirm it with the developer.",
                answers.module.trim()
            ));
        }
    }

    // The answers are validated by here - `output_path` ends in .json and
    // its folder exists - so this is a real place a file is about to
    // appear. Watching starts now, before the assistant has written a line.
    announce_intake_path(answers.output_path.trim());
    note_writing_job_pbi(caller, &ctx.org, answers.examples_pbi);

    let plan = crate::intake::plan_markdown(&answers, &feature);
    let plan_path = crate::intake::plan_path(&answers.output_path);
    // Advice, not a gate: `None` (never an error) when no spec file could
    // be read at all, which must not block an otherwise-ready intake.
    let scale = crate::intake::job_scale(&answers.spec_paths, &answers.sections);
    // `fs::write` TRUNCATES, and this path is derived from a name the
    // developer typed - so it can land on a file that was never ours.
    // Re-running begin after refining the answers has to keep working, so
    // one of our own plans is replaced; anything else is left alone.
    // Blowing away somebody's notes to leave a plan in their place is not
    // a trade this tool gets to make on its own.
    let refused = matches!(
        std::fs::read_to_string(&plan_path),
        Ok(existing) if !existing.starts_with(crate::intake::PLAN_HEADING)
    );
    let written = !refused && std::fs::write(&plan_path, &plan).is_ok();
    (
        200,
        serde_json::json!({
            "status": "ready",
            "unchecked": unchecked,
            "scale": scale,
            "plan": plan,
            "plan_path": if written { serde_json::json!(plan_path) } else { serde_json::Value::Null },
            "plan_write_error": if written {
                serde_json::Value::Null
            } else if refused {
                serde_json::json!(format!(
                    "{plan_path} already exists and was not written by this tool, so it was left \
                     untouched - the plan is in this response instead. Ask the developer for a \
                     different output_path if it should be saved."
                ))
            } else {
                serde_json::json!(format!("could not write {plan_path} - the plan is in this response instead"))
            },
            "answers": answers,
            "note": "Show this plan to the developer and say, in so many words: check the \
                     plan and tell me if anything needs changing, or say to go ahead. Do \
                     not write a single case until they answer. Then write only what the \
                     plan covers, put the JSON exactly at output_path, and follow the \
                     steps at the end of the plan.",
        })
        .to_string(),
    )
}

/// Validate a draft with the app's REAL importer: case count, warnings,
/// errors. With a signed-in client the Module values are also checked
/// against the org's picklist. Reinstated after field feedback rated it
/// the most trustworthy tool in the set.
///
/// Large drafts: `?path=` reads the draft from a local file instead of
/// the request body, so a 166 KB file needs no splitting. An empty body
/// with no path is an explicit error, never a silent pass - a caller must
/// not be able to mistake "nothing arrived" for "nothing wrong".
/// A reviewer note that reports a problem rather than a provenance. The
/// phrases are the ones assistants actually used when they wrote defects
/// into notes; a hit is an advisory, never a block.
pub(crate) fn finding_like_note(notes: &str) -> Option<String> {
    const PHRASES: [&str; 10] = [
        "contradict", "does not match", "doesn't match", "mismatch", "discrepan",
        "inconsisten", "bug:", "defect", "the code does not", "spec says",
    ];
    let lower = notes.to_lowercase();
    PHRASES
        .iter()
        .find(|p| lower.contains(*p))
        .map(|p| format!("it says \"{p}\""))
}

pub(crate) async fn validate_json(
    body: &str,
    target: &str,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
) -> String {
    let from_path = q(target, "path");
    let parsed = match &from_path {
        Some(path) => {
            if !std::path::Path::new(path).is_file() {
                return serde_json::json!({
                    "cases": 0, "warnings": [],
                    "error": format!("{path} does not exist or is not a file"),
                })
                .to_string();
            }
            crate::import_parser::parse_file(path).map(|p| (p.cases, p.warnings))
        }
        None => {
            if body.trim().is_empty() {
                return serde_json::json!({
                    "cases": 0, "warnings": [],
                    "error": "empty draft - pass the JSON in the body, or a local file via ?path= for large drafts",
                })
                .to_string();
            }
            match parse_cases_with_warnings(body) {
                Ok(v) => Ok(v),
                Err(e) => Err(e),
            }
        }
    };
    let out = match parsed {
        Ok((cases, mut warnings)) => {
            // Judgement calls, kept OUT of `warnings` so "fix every
            // warning" stays followable as written.
            let mut advisories: Vec<String> = Vec::new();
            // Text that a round trip through Azure DevOps will read as
            // markup and remove. `<cycleId>` and friends now survive, but a
            // spec quote naming a real element - "<div>", "<img>" - is
            // genuinely indistinguishable from the rich-text editor's own
            // output, so the author gets told here instead of finding out
            // by diffing an export against their own source.
            for (i, tc) in cases.iter().enumerate() {
                let mut hits: Vec<String> = Vec::new();
                for (n, s) in tc.steps.iter().enumerate() {
                    for (what, text) in [("action", &s.action), ("expected", &s.expected)] {
                        if crate::steps_xml::contains_html_markup(text) {
                            hits.push(format!("step {} {what}", n + 1));
                        }
                    }
                }
                if crate::steps_xml::contains_html_markup(&tc.preconditions) {
                    hits.push("preconditions".to_string());
                }
                // A new file must exist and fit the cap, judged by the same
                // check the picker and the upload use. A path that is not a
                // plain local one was already warned by the importer.
                for (n, s) in tc.steps.iter().enumerate() {
                    for att in s.attachments.iter().filter_map(|a| a.path.as_deref()) {
                        if !crate::test_files::is_plain_local_path(att) {
                            continue;
                        }
                        if let Err(why) = crate::ado::attachments::check_step_file(att) {
                            // check_step_file speaks of an upload that failed; nothing has been tried yet.
                            let why = why
                                .replace("so it was not attached", "so it will not be attached")
                                .replace("cannot be attached", "will not be attached");
                            warnings.push(format!("Test case {} ('{}') step {}: {why}", i + 1, tc.title, n + 1));
                        }
                    }
                }
                // Removal is two-part: the entry goes AND its id is listed.
                for g in &tc.removed_attachments {
                    if let Some(n) = tc
                        .steps
                        .iter()
                        .position(|s| s.attachments.iter().any(|a| a.id.as_deref() == Some(g.as_str())))
                    {
                        warnings.push(format!(
                            "Test case {} ('{}'): attachment {g} is listed in remove_attachments but is still on step {}, so it stays. \
                             Delete its entry from the step too, or use the remove_attachment op.",
                            i + 1,
                            tc.title,
                            n + 1
                        ));
                    }
                }
                if tc.update_id.is_none()
                    && (!tc.removed_attachments.is_empty()
                        || tc.steps.iter().any(|s| s.attachments.iter().any(|a| a.id.is_some())))
                {
                    warnings.push(format!(
                        "Test case {} ('{}'): this case has no id, so it is created new and the existing \
                         attachments (and remove_attachments) it lists are not copied. Attach files by 'path' instead.",
                        i + 1,
                        tc.title
                    ));
                }
                if !hits.is_empty() {
                    warnings.push(format!(
                        "Test case {} ('{}'): {} contains an HTML tag name in angle brackets, \
                         which Azure DevOps stores as markup and will drop on the way back. \
                         Placeholders like <cycleId> are safe; a literal element name is not - \
                         write it as `code`, or in braces.",
                        i + 1,
                        tc.title,
                        hits.join(", ")
                    ));
                }

                // How the text is formatted. validate has no copy of an
                // updated case's current text to compare with, so a case
                // with an id gets only the warnings that plain text read
                // back from Azure DevOps can never trigger (see
                // `case_format::formatting_warnings`).
                warnings.extend(crate::case_format::formatting_warnings(
                    i + 1,
                    tc,
                    tc.update_id.is_none(),
                ));

                // A negative folded into its positive is covered but not
                // visible: nobody auditing by title can see it was tested.
                //
                // An ADVISORY, not a warning - round 3 feedback. The tool's
                // own instruction is "fix every warning", and a judgement
                // call that cannot be cleanly cleared devalues the channel
                // it shares: authors learn to skim past all of it,
                // including the import-blocking warnings that were always
                // reliable.
                if let Some(why) = crate::branchcheck::both_branches_reason(tc) {
                    advisories.push(format!(
                        "Test case {} ('{}') appears to cover both branches of a condition - {}. \
                         Consider splitting it into a positive and a negative, each with a title \
                         stating its own branch. If the transition IS the behaviour under test, \
                         leave it.",
                        i + 1,
                        tc.title,
                        why
                    ));
                }

                // A `Spec:` citation with no verbatim quote and no exemption
                // is a judgement call, not a defect the importer can catch -
                // same register as the both-branches check above. Uses
                // `speccov::parse_citations` (the one parser Task 1 built)
                // rather than a second regex over `reviewer_notes`, so the
                // guide's citation grammar and this check can never drift
                // apart. A code-only citation (no `specs` at all) never
                // trips this - the rule is about quoting a SPEC, not code.
                if let Some(citations) = crate::speccov::parse_citations(&tc.reviewer_notes) {
                    if citations.specs.iter().any(|s| s.quote.is_none() && s.exemption.is_none()) {
                        advisories.push(format!(
                            "Test case {} ('{}') cites a Spec: section with no verbatim quote and \
                             no exemption - {}.",
                            i + 1,
                            tc.title,
                            crate::speccov::bare_citation_hint(&tc.reviewer_notes)
                        ));
                    }
                }

                // The two human fields. A comment on a case with NO id was
                // written by the assistant - a case with an id may carry the
                // developer's own, round-tripped through the file - and a
                // note that reports a problem is a finding in the wrong
                // place. Both judgement calls: said, not blocked.
                if tc.update_id.is_none() && !tc.comment.trim().is_empty() {
                    advisories.push(format!(
                        "Test case {} ('{}') carries a `comment`. That field is the developer's and \
                         an assistant never writes it. If this is a problem you found, move it to the \
                         case's `findings` list.",
                        i + 1,
                        tc.title
                    ));
                }
                if let Some(why) = finding_like_note(&tc.reviewer_notes) {
                    advisories.push(format!(
                        "Test case {} ('{}'): its reviewer_notes read like a problem report ({why}). \
                         reviewer_notes say only what the case checks and where the requirement \
                         lives; a problem is a finding - move it to the case's `findings` list and \
                         take it out of the note.",
                        i + 1,
                        tc.title
                    ));
                }
            }
            if let Some(c) = client {
                let allowed = allowed_modules(ctx, c).await;
                // Say so rather than pass silently: "no warnings" has to
                // mean "checked and fine", not "could not look".
                if let Modules::Unavailable(why) = &allowed {
                    warnings.push(format!(
                        "Module values could not be read from Azure DevOps ({why}), so the Module on each case was NOT checked. Everything else was."
                    ));
                }
                let known = allowed.known();
                if !known.is_empty() {
                    for (i, tc) in cases.iter().enumerate() {
                        let m = tc.module_value.trim();
                        if !m.is_empty() && !known.iter().any(|a| a.eq_ignore_ascii_case(m)) {
                            warnings.push(format!(
                                "Test case {} ('{}'): Module '{}' is not an allowed value in \
                                 this organization - pick one from get_guide (topic writing).",
                                i + 1,
                                tc.title,
                                m
                            ));
                        }
                    }
                }
            }
            let mut doc = serde_json::json!({
                "cases": cases.len(),
                "warnings": warnings,
                "error": serde_json::Value::Null,
            });
            // Only present when there are any, and self-describing: the
            // register matters. A warning must be fixed; an advisory must
            // be READ, decided, and the decision said out loud.
            if !advisories.is_empty() {
                doc["advisories"] = serde_json::json!(advisories);
                doc["advisories_note"] = serde_json::json!(
                    "Advisories are judgement calls, not defects: read each one, decide, and \
                     tell the developer what you decided and why. They do not need to be \
                     'fixed' the way warnings do."
                );
            }
            doc
        }
        Err(e) => serde_json::json!({ "cases": 0, "warnings": [], "error": e }),
    };
    out.to_string()
}

/// Request body for `/check-coverage`: which draft to score against which
/// spec documents. `path` and `json` are a strict XOR - never both, never
/// neither - see `check_coverage_route`'s doc comment for why.
#[derive(serde::Deserialize, Default)]
pub(crate) struct CoverageRequest {
    pub(crate) json: Option<String>,
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) spec_paths: Vec<String>,
    #[serde(default)]
    pub(crate) sections: String,
    #[serde(default)]
    pub(crate) out_of_scope: String,
}

/// Score a draft's `Spec:` citations against one or more spec documents -
/// the coverage math itself lives in `speccov::check_coverage`; this route
/// is only the I/O around it.
///
/// `path` XOR `json` names the draft: both present is refused rather than
/// silently preferring one, because a silently-preferred source is exactly
/// the defect class this whole feature exists to catch (round 5, item 10).
/// Neither present is refused the same way, for the same reason. Every
/// `spec_paths` entry is read from disk; one that does not exist or cannot
/// be read is a 400 naming that path, never a silently empty inventory -
/// an unreadable spec must not read as "fully covered because it has no
/// sections".
pub(crate) async fn check_coverage_route(body: &str) -> (u16, String) {
    let req: CoverageRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "error": format!("could not read the request body: {e}") }).to_string(),
            )
        }
    };

    let has_path = req.path.as_deref().is_some_and(|p| !p.trim().is_empty());
    let has_json = req.json.as_deref().is_some_and(|j| !j.trim().is_empty());
    if has_path && has_json {
        return (
            400,
            serde_json::json!({
                "error": "both `path` and `json` were given - pass exactly one, naming the draft \
                          to score. A silently-preferred source is the defect this tool exists to catch."
            })
            .to_string(),
        );
    }
    if !has_path && !has_json {
        return (
            400,
            serde_json::json!({
                "error": "neither `path` nor `json` was given - pass exactly one, naming the draft to score"
            })
            .to_string(),
        );
    }

    let cases = if has_path {
        let path = req.path.as_deref().unwrap_or_default();
        if !std::path::Path::new(path).is_file() {
            return (
                400,
                serde_json::json!({ "error": format!("{path} does not exist or is not a file") }).to_string(),
            );
        }
        match crate::import_parser::parse_file(path) {
            Ok(parsed) => parsed.cases,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        }
    } else {
        match parse_cases_with_warnings(req.json.as_deref().unwrap_or_default()) {
            Ok((cases, _warnings)) => cases,
            Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
        }
    };

    let mut inventories: Vec<(String, crate::speccov::Inventory)> = Vec::with_capacity(req.spec_paths.len());
    for spec_path in &req.spec_paths {
        match std::fs::read_to_string(spec_path) {
            Ok(text) => inventories.push((spec_path.clone(), crate::speccov::parse_inventory(&text))),
            Err(e) => {
                return (
                    400,
                    serde_json::json!({
                        "error": format!("{spec_path} does not exist or could not be read: {e}")
                    })
                    .to_string(),
                )
            }
        }
    }

    let report = crate::speccov::check_coverage(crate::speccov::CoverageInput {
        inventories,
        cases: &cases,
        sections_scope: &req.sections,
        out_of_scope: &req.out_of_scope,
    });
    (200, report.to_string())
}

/// Request body for `/merge-cases`: which slice files to concatenate, and
/// where the merged draft goes.
#[derive(serde::Deserialize, Default)]
pub(crate) struct MergeRequest {
    #[serde(default)]
    pub(crate) paths: Vec<String>,
    #[serde(default)]
    pub(crate) output_path: String,
}

/// Merge slice files from a fan-out into one draft - the other half of
/// `check_spec_coverage`: that finds gaps in a single draft, this is how a
/// spec too large for one writer gets back to being a single draft at all.
///
/// Every slice is read through `crate::import_parser::parse_file` - the
/// REAL importer, same as every other route that reads a draft - never
/// hand-parsed, so a slice a subagent wrote is judged by the exact rules
/// the app itself will apply on import. One unreadable or unparsable slice
/// fails the WHOLE merge with a 400 naming that path; nothing is written,
/// because a merge missing one slice silently is worse than no merge.
/// `output_path` already existing is refused rather than overwritten - the
/// caller picks a new name rather than this route guessing whether the
/// existing file was meant to survive. Cases are concatenated as-is, with
/// no cross-slice deduplication - a caller merging slices that may overlap
/// should run `optimize_cases` or `transform_cases`' dedupe on the merged
/// file afterward.
///
/// With a working repository set, the merged draft obeys the same rule the
/// intake does: a bare name resolves into `<repo>/.test-cases`, and a path
/// outside that folder is refused. A fan-out's merged file is the file the
/// app then watches and imports, so letting it land anywhere would put the
/// end of the writing job outside the workspace the rest of it lives in.
/// Without a repository nothing changes - a merge is a file operation, not
/// an intake, and it still works.
pub(crate) fn merge_cases_route(body: &str, ctx: &BridgeContext) -> (u16, String) {
    let mut req: MergeRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                400,
                serde_json::json!({ "error": format!("could not read the request body: {e}") }).to_string(),
            )
        }
    };

    if req.paths.is_empty() {
        return (
            400,
            serde_json::json!({ "error": "no `paths` given - name the slice files to merge" }).to_string(),
        );
    }
    if req.output_path.trim().is_empty() {
        return (
            400,
            serde_json::json!({ "error": "no `output_path` given - name where the merged draft goes" })
                .to_string(),
        );
    }
    // The workspace rules, when there is a workspace - before the
    // already-exists check, so a bare name is judged where it will land.
    if let Some(root) = ctx
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
    {
        let cases_dir = match crate::workspace::ensure_cases_dir(&root) {
            Ok(d) => d,
            Err(e) => return (500, serde_json::json!({ "error": e }).to_string()),
        };
        req.output_path = crate::workspace::resolve_output(&root, &req.output_path);
        if !crate::workspace::is_inside(&cases_dir, std::path::Path::new(&req.output_path)) {
            return (
                400,
                serde_json::json!({
                    "error": format!(
                        "output_path must be inside the working repository's .test-cases folder \
                         ({}) - give a file name, or a path under that folder.",
                        cases_dir.display()
                    )
                })
                .to_string(),
            );
        }
    }

    if std::path::Path::new(&req.output_path).exists() {
        return (
            400,
            serde_json::json!({
                "error": format!(
                    "{} already exists - merge_case_files refuses to overwrite it; pick a new \
                     output_path or remove the existing file first",
                    req.output_path
                )
            })
            .to_string(),
        );
    }

    let mut merged: Vec<crate::model::TestCase> = Vec::new();
    let mut per_file: Vec<serde_json::Value> = Vec::with_capacity(req.paths.len());
    let mut warnings: Vec<String> = Vec::new();
    // The slices' documents, once each in first-seen order: the merged file
    // names what every slice named, so its review page has a spec pane.
    let mut specs: Vec<String> = Vec::new();
    // Title -> the slice files it appeared in. A fan-out makes title
    // collisions likely precisely because no slice-writer sees another's
    // output, and `dedupe` is the wrong repair (first-wins would delete a
    // real case) - so the merge REPORTS the collision for a human to
    // settle (round 6 §4: two legitimate different cases converged on one
    // title in 373, found only by hand).
    let mut title_slices: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    // Each slice's whole-set note, under the name of the slice it came from.
    // The response calls the slices "safe to remove", so nothing they hold
    // may be left behind in them.
    let mut notes: Vec<String> = Vec::new();
    let output_dir = std::path::Path::new(&req.output_path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();

    for path in &req.paths {
        match crate::import_parser::read_draft(path) {
            Ok((text, parsed)) => {
                let cases = parsed.cases;
                let file_warnings = parsed.warnings;
                let slice_dir = std::path::Path::new(path)
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default();
                for s in parsed.specs {
                    // A relative entry names a file beside the SLICE; the
                    // merged file may live in another folder.
                    let s = crate::spec_pane::rebase_spec_entry(&s, &slice_dir, &output_dir);
                    if !specs.iter().any(|have| have.eq_ignore_ascii_case(&s)) {
                        specs.push(s);
                    }
                }
                let note = crate::import_parser::comments::general_comment(&text);
                if !note.trim().is_empty() {
                    let name = std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone());
                    notes.push(format!("{name}:\n{}", note.trim()));
                }
                per_file.push(serde_json::json!({ "path": path, "cases": cases.len() }));
                // Prefixed with the slice's own file name - a warning
                // aggregated across several slices is useless if it can't
                // be traced back to which one produced it.
                warnings.extend(file_warnings.into_iter().map(|w| format!("{path}: {w}")));
                for c in &cases {
                    // Keyed case-insensitively, but the warning shows the
                    // AUTHOR'S casing - a lowercased title in the message
                    // reads like the tool mangled it (round 8 dogfooding).
                    title_slices
                        .entry(c.title.trim().to_lowercase())
                        .or_default()
                        .push((c.title.trim().to_string(), path.clone()));
                }
                merged.extend(cases);
            }
            Err(e) => {
                return (
                    400,
                    serde_json::json!({ "error": format!("{path}: {e}") }).to_string(),
                )
            }
        }
    }

    for slices in title_slices.values() {
        if slices.len() > 1 {
            let display_title = &slices[0].0;
            let mut named: Vec<&str> = slices.iter().map(|(_, p)| p.as_str()).collect();
            named.dedup();
            warnings.push(format!(
                "duplicate title: '{display_title}' appears {} times ({}) - if these are \
                 different cases, disambiguate the titles; dedupe would keep the first and \
                 delete the rest.",
                slices.len(),
                named.join(", ")
            ));
        }
    }

    let text = match crate::import_parser::queue_to_json_string(&merged)
        .and_then(|t| if specs.is_empty() { Ok(t) } else { crate::import_parser::specs::patch_specs(&t, &specs) })
        .and_then(|t| {
            if notes.is_empty() {
                Ok(t)
            } else {
                crate::import_parser::comments::patch_general_comment(&t, &notes.join("\n\n"))
            }
        })
    {
        Ok(t) => t,
        Err(e) => return (400, serde_json::json!({ "error": e }).to_string()),
    };

    // Atomic write: write to a sibling temp file, then rename it into
    // place. A crash or error partway through a plain `fs::write` would
    // leave `output_path` existing but truncated, which is worse than the
    // merge never having run at all - the caller would find a file, not
    // know it was cut short, and hand a half-draft to the next step.
    if let Err(e) = crate::ai_tools::atomic_write(std::path::Path::new(&req.output_path), &text) {
        return (
            400,
            serde_json::json!({ "error": format!("could not write {}: {e}", req.output_path) }).to_string(),
        );
    }

    (
        200,
        serde_json::json!({
            "cases": merged.len(),
            "per_file": per_file,
            "warnings": warnings,
            "superseded": req.paths,
            "note": format!(
                "The {} slice file(s) above are now superseded by {} and safe to remove from .test-cases - they are importable and id-less, and the importer will happily offer them.",
                req.paths.len(),
                req.output_path
            ),
        })
        .to_string(),
    )
}

/// Like `parse_cases`, but keeps the importer's warnings too.
pub(crate) fn parse_cases_with_warnings(
    body: &str,
) -> Result<(Vec<crate::model::TestCase>, Vec<String>), String> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join("tcm-v2-bridge");
    let _ = std::fs::create_dir_all(&dir);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("validate-{}-{}.json", std::process::id(), seq));
    std::fs::write(&path, body).map_err(|_| "could not stage the draft".to_string())?;
    let parsed = crate::import_parser::parse_file(path.to_str().unwrap_or_default());
    let _ = std::fs::remove_file(&path);
    parsed.map(|p| (p.cases, p.warnings))
}

