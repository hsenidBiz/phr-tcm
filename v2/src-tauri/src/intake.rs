//! The intake that runs BEFORE any test case is written.
//!
//! Two phases, both plain request/response - the assistant asks the
//! questions in chat and the developer answers there, dropping in file
//! paths as they go. An earlier design opened a dialog in the app and
//! blocked the tool call until someone filled it in; that bought real
//! file pickers at the cost of hanging the assistant on a window nobody
//! might be looking at, and it only worked when the app had focus.
//!
//! Phase 1 (`questions`): the assistant asks for the checklist and gets
//! it back with the context the app already knows - the current
//! org/project, the org's real Module values, a sensible output folder.
//! So the developer is correcting defaults rather than composing answers
//! from nothing.
//!
//! Phase 2 (`review`): the answers come back and are CHECKED against the
//! filesystem and the org - spec files must exist, the output folder must
//! exist, the module must be a real one. That is most of what a file
//! picker actually buys you, and it is why the assistant cannot quietly
//! invent a path: an invented one fails here by name.
//!
//! Only once the answers pass does a plan file get written.

/// What the developer decided. Every field is theirs - nothing here is
/// inferred by the assistant.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct IntakeAnswers {
    /// Where the finished JSON is to be written.
    pub output_path: String,
    /// Specification documents the cases must be written from.
    #[serde(default)]
    pub spec_paths: Vec<String>,
    /// Which sections are in scope, free text ("3.1-3.4 only").
    #[serde(default)]
    pub sections: String,
    /// PBI whose existing cases supply house style and the duplicate check.
    #[serde(default)]
    pub examples_pbi: Option<i32>,
    /// Whether the assistant should read those cases at all.
    #[serde(default)]
    pub check_examples: bool,
    /// "spec" | "app" | "spec-wins" - which source wins on a conflict.
    /// The question that saves a whole rewrite.
    #[serde(default)]
    pub authority: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub automation_status: String,
    /// Anything explicitly NOT to be covered.
    #[serde(default)]
    pub out_of_scope: String,
    /// House rules, naming, anything else the developer wants respected.
    #[serde(default)]
    pub notes: String,
}

/// One thing to ask, and why it is worth asking.
#[derive(Debug, serde::Serialize)]
pub struct Question {
    pub field: String,
    pub ask: String,
    pub why: String,
    pub required: bool,
}

/// The checklist. Ordered the way it should be asked: the cheap factual
/// ones first, the one that decides everything (`authority`) early enough
/// to matter.
pub fn questions() -> Vec<Question> {
    let q = |field: &str, ask: &str, why: &str, required: bool| Question {
        field: field.into(),
        ask: ask.into(),
        why: why.into(),
        required,
    };
    vec![
        q(
            "output_path",
            "Where should the finished JSON be written? Give the full path, including the file name.",
            "The developer imports this file by hand; it has to land somewhere they expect.",
            true,
        ),
        q(
            "spec_paths",
            "Which specification documents should the cases come from? Paste or drop the full paths - several is fine, and a folder works too.",
            "Cases written from the wrong source get rewritten from scratch.",
            true,
        ),
        q(
            "sections",
            "Which parts of those documents are in scope? (e.g. \"3.1-3.4 only\", or \"everything except section 5\")",
            "Keeps the set to the size that was actually asked for.",
            false,
        ),
        q(
            "authority",
            "If the specification and the running build disagree, which wins - \"spec\", \"app\", or \"spec-wins\" (read both, spec decides)?",
            "The single most expensive thing to get wrong: writing against a build that was still in development means rewriting every case.",
            true,
        ),
        q(
            "examples_pbi",
            "Which PBI holds existing cases to learn the house style from, and to check for duplicates? Give the id, or say to skip it.",
            "Matching the existing style, and not re-writing cases that already exist.",
            false,
        ),
        q(
            "module",
            "Which Module value should these carry? (the allowed list is in this response)",
            "Azure DevOps rejects anything outside the org's picklist.",
            false,
        ),
        q(
            "tags",
            "Which tags? Semicolon-separated. Call get_tags to reuse what the project already has rather than inventing near-duplicates.",
            "Tag sprawl - \"smoke-test\" next to an existing \"smoke\" - is hard to undo later.",
            false,
        ),
        q(
            "automation_status",
            "\"Not Automated\" or \"Planned\"?",
            "Those are the only two values this field accepts.",
            false,
        ),
        q(
            "out_of_scope",
            "Anything explicitly NOT to cover? (e.g. \"not the page behind the Go button\", \"UI covered separately\")",
            "Stops the set drifting into work someone else is doing.",
            false,
        ),
        q(
            "notes",
            "Anything else about how you want these written - naming, granularity, house rules?",
            "The developer's own conventions, which no guide can guess.",
            false,
        ),
    ]
}

/// Everything wrong with the answers, in the developer's terms. Empty
/// means ready to write.
///
/// Paths are checked against the real filesystem: an assistant that
/// invents a plausible-looking path is caught here rather than at the end
/// of the job when the file turns up somewhere nobody expects.
pub fn problems(a: &IntakeAnswers, allowed_modules: &[String]) -> Vec<String> {
    let mut out = vec![];

    let output = a.output_path.trim();
    if output.is_empty() {
        out.push("output_path is required - ask the developer where the JSON should go.".into());
    } else {
        if !output.to_lowercase().ends_with(".json") {
            out.push(format!("output_path '{output}' does not end in .json."));
        }
        if let Some(dir) = std::path::Path::new(output).parent() {
            if !dir.as_os_str().is_empty() && !dir.is_dir() {
                out.push(format!(
                    "the folder for output_path does not exist: {} - confirm the path with the developer rather than creating it.",
                    dir.display()
                ));
            }
        }
    }

    if a.spec_paths.iter().all(|p| p.trim().is_empty()) {
        out.push("spec_paths is required - ask which documents these cases come from.".into());
    }
    for p in a.spec_paths.iter().filter(|p| !p.trim().is_empty()) {
        let path = std::path::Path::new(p.trim());
        if !path.exists() {
            out.push(format!("spec document not found: {p}"));
        }
    }

    match a.authority.trim() {
        "spec" | "app" | "spec-wins" => {}
        "" => out.push(
            "authority is required - ask whether the spec, the running app, or both (spec wins) decides."
                .into(),
        ),
        other => out.push(format!(
            "authority '{other}' is not one of: spec, app, spec-wins."
        )),
    }

    let status = a.automation_status.trim();
    if !status.is_empty() && status != "Not Automated" && status != "Planned" {
        out.push(format!(
            "automation_status '{status}' is not allowed - use \"Not Automated\" or \"Planned\"."
        ));
    }

    let module = a.module.trim();
    if !module.is_empty()
        && !allowed_modules.is_empty()
        && !allowed_modules.iter().any(|m| m.eq_ignore_ascii_case(module))
    {
        out.push(format!(
            "module '{module}' is not an allowed value in this organization - pick one from the list in this response."
        ));
    }

    if a.check_examples && a.examples_pbi.is_none() {
        out.push(
            "check_examples is set but no examples_pbi was given - ask which PBI, or set check_examples to false."
                .into(),
        );
    }

    out
}

fn bullets(text: &str) -> String {
    let items: Vec<String> = text
        .split(['\n', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| format!("- {s}"))
        .collect();
    if items.is_empty() {
        "- (none stated)".to_string()
    } else {
        items.join("\n")
    }
}

/// The plan file: what was agreed, in the developer's words, written next
/// to the output so the decisions outlive the chat that produced them.
/// The first line of every plan this tool writes. `begin` uses it to tell
/// its own file from one that merely shares the name, so re-running never
/// truncates something a developer wrote by hand.
pub const PLAN_HEADING: &str = "# Test case plan - ";

pub fn plan_markdown(a: &IntakeAnswers, feature: &str) -> String {
    let authority = match a.authority.trim() {
        "spec" => "The specification only - ignore how the build currently behaves.",
        "app" => "The implemented application - describe what it actually does.",
        _ => "Both, and the specification wins where they disagree.",
    };
    let specs = if a.spec_paths.iter().all(|p| p.trim().is_empty()) {
        "- (none given)".to_string()
    } else {
        a.spec_paths
            .iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| format!("- `{}`", p.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let examples = match (a.check_examples, a.examples_pbi) {
        (true, Some(pbi)) => format!(
            "Call `get_test_cases` for PBI #{pbi} first: copy the style and \
             granularity of what is there, and drop anything of yours it already covers."
        ),
        (false, Some(pbi)) => format!(
            "PBI #{pbi} was named but the duplicate check was declined - do not spend calls on it."
        ),
        _ => "No existing cases to check against.".to_string(),
    };

    format!(
        "{PLAN_HEADING}{feature}\n\n\
         Agreed with the developer before any case was written.\n\n\
         ## Scope\n\n\
         **Specifications**\n{specs}\n\n\
         **Sections in scope:** {sections}\n\n\
         **Authority:** {authority}\n\n\
         **Out of scope**\n{out_of_scope}\n\n\
         ## Existing coverage\n\n{examples}\n\n\
         ## Output\n\n\
         - **JSON file:** `{output}`\n\
         - **Tags:** {tags}\n\
         - **Module:** {module}\n\
         - **Automation status:** {status}\n\n\
         ## Notes from the developer\n\n{notes}\n\n\
         ## Before handing the file over\n\n\
         1. Draft against the sources above - nothing outside them.\n\
         2. Run `optimize_cases` to spell out navigation, trim expected results \
         and order the cases so the tester changes environment as little as possible.\n\
         3. Run `validate_cases` (pass `path` for a large draft) and fix every warning.\n\
         4. Raise any contradiction found in the specs here rather than \
         resolving it silently.\n",
        feature = if feature.trim().is_empty() { "untitled" } else { feature.trim() },
        specs = specs,
        sections = if a.sections.trim().is_empty() { "everything in the documents above" } else { a.sections.trim() },
        authority = authority,
        out_of_scope = bullets(&a.out_of_scope),
        examples = examples,
        output = a.output_path.trim(),
        tags = if a.tags.trim().is_empty() { "(none)" } else { a.tags.trim() },
        module = if a.module.trim().is_empty() { "(none)" } else { a.module.trim() },
        status = if a.automation_status.trim().is_empty() { "Not Automated" } else { a.automation_status.trim() },
        notes = bullets(&a.notes),
    )
}

/// Where the plan is written: beside the output JSON, same stem.
///
/// Built by string rather than `Path::join`, which would substitute the
/// platform separator and hand back `C:/work\cases-plan.md` for a
/// forward-slash input. The developer gave us a path in one style; they
/// get it back in that style.
pub fn plan_path(output_path: &str) -> String {
    let path = output_path.trim();
    let cut = path.rfind(['/', '\\']);
    let (dir, file) = match cut {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    };
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    let stem = if stem.is_empty() { "test-cases" } else { stem };
    format!("{dir}{stem}-plan.md")
}
