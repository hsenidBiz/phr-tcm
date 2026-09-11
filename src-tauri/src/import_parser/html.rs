//! The standalone, print-friendly HTML report and its autosaving comment
//! boxes (loopback note listener).
//!
//! The page's stylesheet and scripts are files under `src-tauri/web/` (see
//! the README there). What is left here is the logic that assembles them
//! around the data: escaping, grouping, and the markup for each case.

use crate::model::TestCase;

/// Written entirely against the theme variables `webtheme` emits, so the
/// one stylesheet serves every app theme and both schemes.
///
/// Two habits from that conversion are worth keeping if this is edited:
/// the tinted chips and focus rings are `color-mix` against the live
/// accent rather than the fixed blues/greens/violets they used to be, and
/// every card carries a real border as well as its shadow - a drop shadow
/// is invisible on the OLED theme's black, and the page would otherwise
/// come apart into floating text.
const HTML_CSS: &str = include_str!("../../web/cases-page.css");

const HTML_JS: &str = include_str!("../../web/cases-page.js");

/// Autosaving comment boxes in the report: debounce each textarea and POST
/// to the app's loopback note listener, then say what actually happened.
///
/// The reply is read (a plain CORS fetch - see note_server) rather than
/// fired blind, because a draft comment really can fail to land: the file
/// may have moved, or an assistant may have renamed the case out from
/// under it. "Saved ✓" has to mean saved.
const NOTE_JS: &str = include_str!("../../web/cases-notes.js");

/// JSON for embedding inside a `<script>` element.
///
/// `serde_json` leaves `<` alone, which is fine in a .json file and unsafe
/// here: a test case titled `</script><img onerror=...>` would otherwise
/// close the script element and run as markup. `<\/` is a valid escape in
/// both JavaScript and JSON, so the value the page parses is unchanged.
fn script_json<T: serde::Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_string(value)
        .map(|s| s.replace("</", "<\\/"))
        .unwrap_or_else(|_| fallback.to_string())
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Context for comment boxes on a page of EXISTING Azure DevOps cases.
/// Their comments are a personal scratchpad held by the app, keyed by work
/// item id - nothing is written to Azure DevOps and there is no file.
pub struct NoteCtx {
    pub port: u16,
    /// Shared secret the listener requires - see note_server::start.
    pub token: String,
    pub org: String,
    /// Existing notes to prefill, keyed by work item id (as a string).
    pub notes: std::collections::HashMap<String, String>,
}

/// One JSON file the draft was imported from, and the comment about it as
/// a whole.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct DraftFile {
    pub path: String,
    /// What to call it in the panel - the file name, not the full path.
    pub label: String,
    pub comment: String,
}

/// Context for comment boxes on a page of DRAFT cases: every case gets a
/// box (they have no work item id yet, and that is the point), and the text
/// goes into the case's own `comment` field - the same one the queue card
/// edits - which is written back into the JSON file the case came from.
pub struct DraftNoteCtx {
    pub port: u16,
    /// Shared secret the listener requires - see note_server::start.
    pub token: String,
    /// The file each queued case came from, aligned with `queue`. Empty
    /// where a case was typed by hand and has no file to be written to;
    /// its comment is still kept by the app.
    pub owners: Vec<String>,
    /// The files whose whole-set comments the side panel offers. Empty
    /// means no panel - nothing was imported from a file.
    pub files: Vec<DraftFile>,
}

/// Which kind of page is being built. The two differ in where a comment
/// belongs, which is not something a single flag could express.
pub enum CommentCtx<'a> {
    Ado(&'a NoteCtx),
    Draft(&'a DraftNoteCtx),
}

impl CommentCtx<'_> {
    fn port(&self) -> u16 {
        match self {
            CommentCtx::Ado(c) => c.port,
            CommentCtx::Draft(c) => c.port,
        }
    }

    fn token(&self) -> &str {
        match self {
            CommentCtx::Ado(c) => &c.token,
            CommentCtx::Draft(c) => &c.token,
        }
    }

    /// Which report this page is, so it polls for its OWN revision. The two
    /// are separate documents about separate things: re-exporting a draft
    /// must not tell a page of existing cases that it is out of date.
    fn report_kind(&self) -> &'static str {
        match self {
            CommentCtx::Ado(_) => crate::note_server::REPORT_QUEUE,
            CommentCtx::Draft(_) => crate::note_server::REPORT_DRAFT,
        }
    }
}

/// Standalone, print-friendly HTML report, ported from v1
/// export_records_to_html (same cards, chips, sticky search filter).
///
/// With a `CommentCtx` the page also carries autosaving comment boxes: one
/// per case, plus - for a draft imported from files - a collapsible column
/// of whole-set comments, one per file.
pub fn export_queue_to_html(
    queue: &[TestCase],
    path: &str,
    subtitle: &str,
    ctx: Option<CommentCtx>,
    palette: &crate::webtheme::PagePalette,
    findings: &[crate::findings::Finding],
) -> Result<(), String> {
    // The side column only exists for a draft that came from files. Without
    // it the page keeps its original single centred column.
    let files: &[DraftFile] = match &ctx {
        Some(CommentCtx::Draft(d)) => &d.files,
        _ => &[],
    };
    // A page of DRAFTS - cases not yet written to Azure DevOps. The plain
    // export (no ctx) is a draft too; only the Ado page shows cases that
    // already exist, where "what will importing do" is not a question.
    let draft_page = !matches!(&ctx, Some(CommentCtx::Ado(_)));
    let shell_open = if files.is_empty() { "" } else { "<div class='shell'>" };
    // Identity of each draft box, resolved by the app when the note lands.
    let mut draft_cases: Vec<serde_json::Value> = vec![];

    let mut parts: Vec<String> = vec![
        "<!DOCTYPE html>".into(),
        format!(
            "<html lang=\"en\" data-scheme=\"{}\"><head><meta charset=\"utf-8\">",
            palette.initial_scheme()
        ),
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into(),
        format!("<title>Test Cases ({})</title>", queue.len()),
        format!(
            "<style>{vars}{HTML_CSS}</style></head><body>{switch}{shell_open}<div class='page'>",
            vars = palette.css(),
            switch = crate::webtheme::SWITCH_HTML,
        ),
        "<h1>Test Cases</h1>".into(),
        format!(
            "<p class='subtitle'>{}</p>",
            if subtitle.is_empty() {
                format!("{} test case(s)", queue.len())
            } else {
                esc(subtitle)
            }
        ),
        // Above the search bar and sticky in its own right, so it is seen
        // whether the reviewer is at the top of the page or the bottom.
        "<div id='tc-stale' role='status'><span>The test cases have changed since this page was opened.</span><button type='button' id='tc-stale-go'>Refresh</button></div>".into(),
        "<div class='searchbar'>".into(),
        "<input id='tc-search' type='search' placeholder='Search title, ID, tags, steps, prerequisites...' aria-label='Search test cases'>".into(),
        // Only when at least one case HAS notes - a button that hides
        // nothing is just another thing to read.
        if queue.iter().any(|tc| !tc.reviewer_notes.trim().is_empty()) {
            "<button id='tc-notes' type='button' aria-pressed='false'>Hide reviewer notes</button>"
                .to_string()
        } else {
            String::new()
        },
        "<span id='tc-count'></span></div>".into(),
        "<p id='tc-no-match' class='no-match hidden'>No test cases match your search.</p>".into(),
    ];
    // AI Findings: what an assistant found wrong, in a block of its own
    // above the cases - never inside a case's notes (provenance) or its
    // comment box (the developer's). Open ones only; resolved is done.
    let open: Vec<&crate::findings::Finding> = findings.iter().filter(|f| f.status == "open").collect();
    if !open.is_empty() {
        parts.push(format!(
            "<section class='findings'><h2>AI Findings <span class='count'>{}</span></h2>\
             <p class='lead'>Problems an assistant found while reading. Resolve or dismiss them on the AI Bridge tab.</p>",
            open.len()
        ));
        for f in open {
            let kind = match f.kind.as_str() {
                "test_case" => "Test case",
                "spec" => "Spec",
                "code" => "Code",
                other => other,
            };
            parts.push(format!(
                "<article class='finding'><div class='meta'><span class='kind'>{}</span>\
                 <span class='subject'>{}</span><span class='when'>{}</span></div>\
                 <p class='ftitle'>{}</p><div class='fdetail'>{}</div></article>",
                esc(kind),
                esc(&f.subject),
                esc(&f.created_at[..f.created_at.len().min(10)]),
                esc(&f.title),
                crate::markdown::to_html(&f.detail)
            ));
        }
        parts.push("</section>".into());
    }
    for (idx, tc) in queue.iter().enumerate() {
        parts.push("<div class='case'>".into());
        let wid = tc
            .update_id
            .map(|id| format!("<span class='wid'>#{id}</span>"))
            .unwrap_or_default();
        // What importing this case will DO - the same UPDATE/NEW badge the
        // queue rows carry, so a reviewer reading the page knows which
        // cases will touch existing work items and which will create.
        // Draft pages only: on a page of cases that already live in Azure
        // DevOps the question does not arise.
        let op = if draft_page {
            if tc.update_id.is_some() {
                "<span class='chip op-update'>UPDATE</span>"
            } else {
                "<span class='chip op-new'>NEW</span>"
            }
        } else {
            ""
        };
        // Numbered by position in the page, and NOT renumbered when the
        // search filter hides some: "case 7" has to mean the same thing
        // before and after someone types in the box.
        parts.push(format!(
            "<h2><span class='seq'>{}</span>{op}{wid}{}</h2>",
            idx + 1,
            esc(&tc.title)
        ));

        // One labelled row per kind. A single undifferentiated row of
        // chips left a reader guessing which pill was the module and
        // which were tags - the label says so outright.
        let mut rows = vec![];
        if !tc.automation_status.is_empty() {
            rows.push(format!(
                "<div class='metarow'><span class='metalabel'>Automation Status</span>\
                 <span class='metavals'><span class='chip status'>{}</span></span></div>",
                esc(&tc.automation_status)
            ));
        }
        if !tc.module_value.is_empty() {
            rows.push(format!(
                "<div class='metarow'><span class='metalabel'>Module</span>\
                 <span class='metavals'><span class='chip module'>{}</span></span></div>",
                esc(&tc.module_value)
            ));
        }
        let tags: Vec<String> = tc
            .tags
            .split(';')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| format!("<span class='chip'>{}</span>", esc(t)))
            .collect();
        if !tags.is_empty() {
            rows.push(format!(
                "<div class='metarow'><span class='metalabel'>Tags</span>\
                 <span class='metavals'>{}</span></div>",
                tags.join("")
            ));
        }
        if !rows.is_empty() {
            parts.push(format!("<div class='meta'>{}</div>", rows.join("")));
        }

        // Every case shows a Prerequisites block, even when empty (v1 rule).
        let prereq = tc.preconditions.trim();
        let prereq_html = if prereq.is_empty() {
            "<span class='none'>None</span>".to_string()
        } else {
            esc(prereq)
        };
        parts.push(format!("<p class='pre'><b>Prerequisites:</b> {prereq_html}</p>"));

        // Reviewer notes: where this case came from in the spec. Open by
        // default - the whole reason the field exists is that matching a
        // case to its requirement is the slow part of a review, and notes
        // behind a closed disclosure would not be read. Still a <details>
        // so a long note can be folded away once it has been used.
        //
        // Rendered as markdown, and read-only: it is the reference the
        // reviewer reads, while the comment box below is what they write.
        // Its markdown is turned into HTML in Rust, with any HTML in the
        // SOURCE dropped rather than filtered - see crate::markdown.
        // The wrapper div exists for the closing animation: a grid row can
        // animate 1fr -> 0fr, a <details> cannot animate its own height.
        // The x collapses just this case's notes; the sticky Hide button
        // collapses them all, and Show brings every note back, including
        // individually closed ones - one button that undoes everything
        // beats remembering which x was clicked where.
        if !tc.reviewer_notes.trim().is_empty() {
            parts.push(format!(
                "<div class='rev-wrap'><details class='rev' open><summary>Reviewer notes\
                 <button type='button' class='rev-close' aria-label='Hide these reviewer notes' \
                 title='Hide these reviewer notes'>&#215;</button></summary>\
                 <div class='rev-body'>{}</div></details></div>",
                crate::markdown::to_html(&tc.reviewer_notes)
            ));
        }

        if !tc.steps.is_empty() {
            parts.push("<table><tr><th>#</th><th>Action</th><th>Expected result</th></tr>".into());
            for (i, step) in tc.steps.iter().enumerate() {
                parts.push(format!(
                    "<tr><td class='num'>{}</td><td>{}</td><td>{}</td></tr>",
                    i + 1,
                    esc(&step.action),
                    esc(&step.expected)
                ));
            }
            parts.push("</table>".into());
        }
        match &ctx {
            // An existing case: its note is the app's own scratchpad, so
            // only a case that HAS an id can be keyed at all.
            Some(CommentCtx::Ado(c)) => {
                if let Some(id) = tc.update_id {
                    let existing = c.notes.get(&id.to_string()).map(String::as_str).unwrap_or("");
                    parts.push(note_box(
                        &format!("nb-{id}"),
                        &format!("data-ado='{id}'"),
                        "e.g. Step 3 needs the new confirmation dialog (saved in the app, on this device only)",
                        existing,
                    ));
                }
            }
            // A draft case: every one gets a box, because the comment goes
            // into the case itself rather than into an id-keyed store.
            Some(CommentCtx::Draft(d)) => {
                let owner = d.owners.get(idx).cloned().unwrap_or_default();
                let slot = draft_cases.len();
                draft_cases.push(serde_json::json!({
                    "path": owner, "id": tc.update_id, "title": tc.title,
                }));
                let hint = if d.owners.get(idx).is_some_and(|p| !p.is_empty()) {
                    "Saved into this case in the JSON file"
                } else {
                    "Saved with the draft in the app"
                };
                parts.push(note_box(
                    &format!("nb-d{slot}"),
                    &format!("data-case='{slot}'"),
                    hint,
                    &tc.comment,
                ));
            }
            None => {}
        }
        parts.push("</div>".into());
    }
    // Comment-box identities ride INSIDE the swappable content as inert
    // JSON, not as script vars outside it. When the page pulls a fresh
    // copy of itself and swaps in place, the boxes it adopts come with the
    // identities that match them - a rename must not leave a comment box
    // addressing the title a case had when the tab was opened. A JSON
    // script block never executes, so importing it into the live document
    // cannot double-run anything.
    if ctx.is_some() {
        let file_paths: Vec<serde_json::Value> =
            files.iter().map(|f| serde_json::json!({ "path": f.path })).collect();
        parts.push(format!(
            "<script type='application/json' id='tc-data'>{}</script>",
            script_json(
                &serde_json::json!({ "cases": draft_cases, "files": file_paths }),
                "{\"cases\":[],\"files\":[]}",
            )
        ));
    }
    parts.push("</div>".into());

    // The whole-set comments, one box per file, collapsible and out of the
    // way of the cards.
    if !files.is_empty() {
        parts.push(
            "<details class='aside' open><summary>General comments</summary><div class='aside-body'>"
                .into(),
        );
        for (i, f) in files.iter().enumerate() {
            parts.push(format!(
                "<div class='file'><div class='filename'>{} \
                 <span class='note-status' id='ns-f{i}'></span></div>\
                 <textarea class='note-box' id='nb-f{i}' data-file='{i}' \
                 data-status='ns-f{i}' aria-label='General comments for {}' \
                 placeholder='Notes about this set as a whole - saved into {}'>{}</textarea></div>",
                esc(&f.label),
                esc(&f.label),
                esc(&f.label),
                esc(&f.comment)
            ));
        }
        parts.push("</div></details>".into());
        parts.push("</div>".into()); // .shell
    }

    parts.push(format!("<script>{HTML_JS}</script>"));
    parts.push(format!("<script>{}</script>", crate::webtheme::SWITCH_JS));
    if let Some(c) = &ctx {
        let org = match c {
            CommentCtx::Ado(a) => a.org.clone(),
            CommentCtx::Draft(_) => String::new(),
        };
        parts.push(format!(
            // REPORT_REV is the revision this file was written at. The page
            // compares it with what the app reports now; they diverge the
            // moment the report is re-exported behind an open tab. The
            // comment-box identities are NOT here any more - they live in
            // the #tc-data JSON block inside the page, so a live swap
            // carries them along with the boxes they describe.
            "<script>var NOTE_PORT={};var NOTE_TOKEN={};var NOTE_ORG={};var REPORT_REV={};var REPORT_KIND={};{NOTE_JS}</script>",
            c.port(),
            script_json(&c.token(), "\"\""),
            script_json(&org, "\"\""),
            crate::note_server::revision(c.report_kind()),
            script_json(&c.report_kind(), "\"draft\""),
        ));
    }
    parts.push("</body></html>".into());
    std::fs::write(path, parts.join("\n")).map_err(|e| e.to_string())
}

/// One labelled, autosaving comment box. `hook` is the data attribute that
/// tells the page's script which kind of comment this is and how to address
/// it; the status span beside the label is where the save result lands.
fn note_box(id: &str, hook: &str, placeholder: &str, value: &str) -> String {
    let status = format!("ns-{}", id.trim_start_matches("nb-"));
    format!(
        "<div class='note'><label for='{id}'>My comment \
         <span class='note-status' id='{status}'></span></label>\
         <textarea class='note-box' id='{id}' {hook} data-status='{status}' \
         placeholder='{}'>{}</textarea></div>",
        esc(placeholder),
        esc(value)
    )
}