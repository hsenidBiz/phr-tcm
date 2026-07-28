//! The standalone, print-friendly HTML report and its autosaving comment
//! boxes (loopback note listener).

use crate::model::TestCase;

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

const HTML_CSS: &str = r#"
:root { color-scheme: light; }
* { box-sizing: border-box; }
body { font-family: 'Segoe UI', system-ui, sans-serif; margin: 0; padding: 32px 16px;
       background: #f3f5f8; color: #1f2530; }
.page { max-width: 900px; margin: 0 auto; }
h1 { font-size: 22px; margin: 0 0 4px; }
.subtitle { color: #5c6675; font-size: 13px; margin: 0 0 24px; }
.case { background: #fff; border: 1px solid #dde3ec; border-radius: 10px;
        padding: 18px 22px; margin-bottom: 18px; box-shadow: 0 1px 3px rgba(20,30,50,.05);
        page-break-inside: avoid; }
.case h2 { font-size: 16px; margin: 0 0 8px; }
.case .wid { color: #2a7ab8; font-weight: 600; margin-right: 6px; }
/* Position in the set, so a reviewer can say "case 7" out loud. Tabular
   figures keep the titles aligned once the count passes nine. Counted in
   the page, never stored - it is not a property of the test case. */
.case .seq { color: #8a94a6; font-weight: 600; margin-right: 8px;
             font-variant-numeric: tabular-nums; }
.meta { margin: 0 0 10px; }
/* One labelled row per kind, label column aligned so the three rows read
   as a small table. The label wraps above its values on narrow screens
   rather than squeezing them. */
.metarow { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; margin-bottom: 4px; }
/* Auto-width, not a fixed column: the chips sit right beside their label
   instead of across a gap sized for the longest label. */
.metalabel { flex: 0 0 auto; font-size: 11.5px; font-weight: 600; color: #44506a; }
.metavals { display: flex; flex-wrap: wrap; gap: 6px; min-width: 0; flex: 1 1 200px; }
.chip { font-size: 11.5px; border-radius: 999px; padding: 2px 10px;
        background: #eef2f8; color: #44506a; border: 1px solid #dbe2ee; }
.chip.status { background: #e8f3ea; color: #2f6b3c; border-color: #cfe5d4; }
.chip.module { background: #f0eafa; color: #5b3e9e; border-color: #e0d5f2; }
.pre { font-size: 13px; background: #f7f9fc; border-left: 3px solid #b9c6da;
       padding: 8px 12px; margin: 0 0 12px; white-space: pre-wrap; }
.pre b { color: #44506a; }
.pre .none { color: #8a94a6; font-style: italic; }
table { width: 100%; border-collapse: collapse; font-size: 13px; }
th { text-align: left; background: #f0f3f8; color: #44506a; font-size: 12px;
     padding: 6px 10px; border: 1px solid #e1e7f0; }
td { padding: 7px 10px; border: 1px solid #e7ecf3; vertical-align: top;
     white-space: pre-wrap; }
td.num { width: 34px; text-align: center; color: #7c8698; }
.searchbar { position: sticky; top: 0; z-index: 5; background: #f3f5f8;
             display: flex; align-items: center; gap: 12px; padding: 10px 0 14px; }
#tc-search { flex: 1; font: inherit; font-size: 14px; padding: 9px 14px;
             border: 1px solid #c9d3e2; border-radius: 8px; background: #fff;
             color: inherit; outline: none; }
#tc-search:focus { border-color: #2a7ab8; box-shadow: 0 0 0 3px rgba(42,122,184,.15); }
#tc-count { color: #5c6675; font-size: 12.5px; white-space: nowrap; }
.no-match { color: #5c6675; font-size: 14px; text-align: center;
            padding: 28px 0; border: 1px dashed #c9d3e2; border-radius: 10px; }
.hidden { display: none !important; }
.note { margin-top: 12px; border-top: 1px dashed #c9d3e2; padding-top: 10px; }
.note label { display: flex; align-items: baseline; gap: 8px; font-size: 12.5px;
              font-weight: 600; color: #3c4657; margin-bottom: 4px; }
.note-status { font-weight: 400; font-size: 12px; color: #2a7ab8; }
.note-box { width: 100%; min-height: 44px; resize: vertical; font: inherit;
            font-size: 13px; color: inherit; background: #f7f9fc;
            border: 1px solid #c9d3e2; border-radius: 8px; padding: 8px 10px;
            outline: none; }
.note-box:focus { border-color: #2a7ab8; box-shadow: 0 0 0 3px rgba(42,122,184,.15); }
.note-status.bad { color: #b3261e; }
/* The whole-set comments column. A <details> so collapsing costs no
   JavaScript, and the two-column layout only appears when there is room
   for it - below that the panel stacks above the cases, where it is still
   the first thing read. */
.shell { max-width: 1280px; margin: 0 auto; display: grid; gap: 24px;
         grid-template-columns: minmax(0, 1fr); }
.shell .page { max-width: none; margin: 0; }
@media (min-width: 1120px) {
  .shell { grid-template-columns: minmax(0, 1fr) 320px; align-items: start; }
  .aside { position: sticky; top: 12px; }
}
.aside { background: #fff; border: 1px solid #dde3ec; border-radius: 10px;
         box-shadow: 0 1px 3px rgba(20,30,50,.05); }
.aside > summary { cursor: pointer; list-style: none; padding: 12px 16px;
                   font-size: 13px; font-weight: 600; color: #3c4657;
                   display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.aside > summary::-webkit-details-marker { display: none; }
.aside > summary::after { content: '\25be'; color: #8a94a6; transition: transform .15s ease; }
.aside:not([open]) > summary::after { transform: rotate(-90deg); }
.aside-body { padding: 0 16px 14px; }
.aside .file + .file { margin-top: 14px; padding-top: 14px; border-top: 1px dashed #dde3ec; }
.aside .filename { font-size: 11.5px; font-weight: 600; color: #44506a;
                   word-break: break-all; margin-bottom: 5px;
                   display: flex; align-items: baseline; gap: 8px; }
.aside .note-box { min-height: 96px; }
@media print { body { background: #fff; padding: 0; }
               .case { box-shadow: none; border-color: #ccc; }
               .searchbar { display: none; }
               .shell { display: block; }
               .aside { display: none; }
               .note { display: none; } }
"#;

const HTML_JS: &str = r#"
(function () {
  var input = document.getElementById('tc-search');
  var count = document.getElementById('tc-count');
  var noMatch = document.getElementById('tc-no-match');
  var cards = Array.prototype.slice.call(document.querySelectorAll('.case'));
  var texts = cards.map(function (c) { return c.textContent.toLowerCase(); });
  var total = cards.length;

  function apply() {
    var words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    var shown = 0;
    texts.forEach(function (t, i) {
      var hit = words.every(function (w) { return t.indexOf(w) !== -1; });
      cards[i].classList.toggle('hidden', !hit);
      if (hit) shown++;
    });
    count.textContent = words.length
      ? shown + ' of ' + total + ' shown'
      : total + ' test case' + (total !== 1 ? 's' : '');
    noMatch.classList.toggle('hidden', shown !== 0);
  }

  input.addEventListener('input', apply);
  input.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') { input.value = ''; apply(); }
  });
  apply();
})();
"#;

/// Autosaving comment boxes in the report: debounce each textarea and POST
/// to the app's loopback note listener, then say what actually happened.
///
/// The reply is read (a plain CORS fetch - see note_server) rather than
/// fired blind, because a draft comment really can fail to land: the file
/// may have moved, or an assistant may have renamed the case out from
/// under it. "Saved ✓" has to mean saved.
const NOTE_JS: &str = r#"
(function () {
  function wire(box, status, build) {
    var timer = null;
    box.addEventListener('input', function () {
      status.className = 'note-status';
      status.textContent = 'Saving…';
      clearTimeout(timer);
      timer = setTimeout(function () {
        fetch('http://127.0.0.1:' + NOTE_PORT + '/note', {
          method: 'POST',
          headers: { 'Content-Type': 'text/plain' },
          body: JSON.stringify(build(box.value))
        }).then(function (r) { return r.json(); }).then(function (r) {
          if (r && r.ok) {
            status.className = 'note-status';
            status.textContent = 'Saved ✓';
          } else {
            status.className = 'note-status bad';
            status.textContent = 'Not saved — ' + ((r && r.error) || 'the app refused it');
          }
        }).catch(function () {
          status.className = 'note-status bad';
          status.textContent = 'Not saved — the app is closed';
        });
      }, 600);
    });
  }

  Array.prototype.forEach.call(document.querySelectorAll('[data-ado]'), function (box) {
    wire(box, document.getElementById(box.dataset.status), function (text) {
      return { kind: 'ado', org: NOTE_ORG, case_id: Number(box.dataset.ado), text: text };
    });
  });

  Array.prototype.forEach.call(document.querySelectorAll('[data-case]'), function (box) {
    var t = DRAFT_CASES[Number(box.dataset.case)];
    wire(box, document.getElementById(box.dataset.status), function (text) {
      return { kind: 'case', path: t.path, id: t.id, title: t.title, text: text };
    });
  });

  Array.prototype.forEach.call(document.querySelectorAll('[data-file]'), function (box) {
    var f = DRAFT_FILES[Number(box.dataset.file)];
    wire(box, document.getElementById(box.dataset.status), function (text) {
      return { kind: 'general', path: f.path, text: text };
    });
  });
})();
"#;

/// Context for comment boxes on a page of EXISTING Azure DevOps cases.
/// Their comments are a personal scratchpad held by the app, keyed by work
/// item id - nothing is written to Azure DevOps and there is no file.
pub struct NoteCtx {
    pub port: u16,
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
) -> Result<(), String> {
    // The side column only exists for a draft that came from files. Without
    // it the page keeps its original single centred column.
    let files: &[DraftFile] = match &ctx {
        Some(CommentCtx::Draft(d)) => &d.files,
        _ => &[],
    };
    let shell_open = if files.is_empty() { "" } else { "<div class='shell'>" };
    // Identity of each draft box, resolved by the app when the note lands.
    let mut draft_cases: Vec<serde_json::Value> = vec![];

    let mut parts: Vec<String> = vec![
        "<!DOCTYPE html>".into(),
        "<html lang=\"en\"><head><meta charset=\"utf-8\">".into(),
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into(),
        format!("<title>Test Cases ({})</title>", queue.len()),
        format!("<style>{HTML_CSS}</style></head><body>{shell_open}<div class='page'>"),
        "<h1>Test Cases</h1>".into(),
        format!(
            "<p class='subtitle'>{}</p>",
            if subtitle.is_empty() {
                format!("{} test case(s)", queue.len())
            } else {
                esc(subtitle)
            }
        ),
        "<div class='searchbar'>".into(),
        "<input id='tc-search' type='search' placeholder='Search title, ID, tags, steps, prerequisites...' aria-label='Search test cases'>".into(),
        "<span id='tc-count'></span></div>".into(),
        "<p id='tc-no-match' class='no-match hidden'>No test cases match your search.</p>".into(),
    ];
    for (idx, tc) in queue.iter().enumerate() {
        parts.push("<div class='case'>".into());
        let wid = tc
            .update_id
            .map(|id| format!("<span class='wid'>#{id}</span>"))
            .unwrap_or_default();
        // Numbered by position in the page, and NOT renumbered when the
        // search filter hides some: "case 7" has to mean the same thing
        // before and after someone types in the box.
        parts.push(format!(
            "<h2><span class='seq'>{}</span>{wid}{}</h2>",
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
    if let Some(c) = &ctx {
        let org = match c {
            CommentCtx::Ado(a) => a.org.clone(),
            CommentCtx::Draft(_) => String::new(),
        };
        let file_paths: Vec<serde_json::Value> = files
            .iter()
            .map(|f| serde_json::json!({ "path": f.path }))
            .collect();
        parts.push(format!(
            "<script>var NOTE_PORT={};var NOTE_ORG={};var DRAFT_CASES={};var DRAFT_FILES={};{NOTE_JS}</script>",
            c.port(),
            script_json(&org, "\"\""),
            script_json(&draft_cases, "[]"),
            script_json(&file_paths, "[]"),
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