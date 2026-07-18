//! The standalone, print-friendly HTML report and its autosaving comment
//! boxes (loopback note listener).

use crate::model::TestCase;

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
.meta { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 10px; }
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
@media print { body { background: #fff; padding: 0; }
               .case { box-shadow: none; border-color: #ccc; }
               .searchbar { display: none; }
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
/// to the app's loopback note listener. mode:'no-cors' keeps the file://
/// page happy; a network error means the app was closed.
const NOTE_JS: &str = r#"
(function () {
  var boxes = Array.prototype.slice.call(document.querySelectorAll('.note-box'));
  boxes.forEach(function (box) {
    var status = document.getElementById('ns-' + box.dataset.id);
    var timer = null;
    box.addEventListener('input', function () {
      status.textContent = 'Saving…';
      clearTimeout(timer);
      timer = setTimeout(function () {
        fetch('http://127.0.0.1:' + NOTE_PORT + '/note', {
          method: 'POST',
          mode: 'no-cors',
          headers: { 'Content-Type': 'text/plain' },
          body: JSON.stringify({ org: NOTE_ORG, case_id: Number(box.dataset.id), text: box.value })
        }).then(function () {
          status.textContent = 'Saved ✓';
        }).catch(function () {
          status.textContent = 'Not saved — the app is closed';
        });
      }, 600);
    });
  });
})();
"#;

/// Context for the report's autosaving comment boxes.
pub struct NoteCtx {
    pub port: u16,
    pub org: String,
    /// Existing notes to prefill, keyed by work item id (as a string).
    pub notes: std::collections::HashMap<String, String>,
}

/// Standalone, print-friendly HTML report, ported from v1
/// export_records_to_html (same cards, chips, sticky search filter). With a
/// NoteCtx, every case that has a work item id also gets a comment box that
/// autosaves to the app's local notes over the loopback listener.
pub fn export_queue_to_html(
    queue: &[TestCase],
    path: &str,
    subtitle: &str,
    note_ctx: Option<&NoteCtx>,
) -> Result<(), String> {
    let mut parts: Vec<String> = vec![
        "<!DOCTYPE html>".into(),
        "<html lang=\"en\"><head><meta charset=\"utf-8\">".into(),
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into(),
        format!("<title>Test Cases ({})</title>", queue.len()),
        format!("<style>{HTML_CSS}</style></head><body><div class='page'>"),
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
    for tc in queue {
        parts.push("<div class='case'>".into());
        let wid = tc
            .update_id
            .map(|id| format!("<span class='wid'>#{id}</span>"))
            .unwrap_or_default();
        parts.push(format!("<h2>{wid}{}</h2>", esc(&tc.title)));

        let mut chips = vec![];
        if !tc.automation_status.is_empty() {
            chips.push(format!("<span class='chip status'>{}</span>", esc(&tc.automation_status)));
        }
        if !tc.module_value.is_empty() {
            chips.push(format!("<span class='chip module'>{}</span>", esc(&tc.module_value)));
        }
        for tag in tc.tags.split(';') {
            let tag = tag.trim();
            if !tag.is_empty() {
                chips.push(format!("<span class='chip'>{}</span>", esc(tag)));
            }
        }
        if !chips.is_empty() {
            parts.push(format!("<div class='meta'>{}</div>", chips.join("")));
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
        if let (Some(ctx), Some(id)) = (note_ctx, tc.update_id) {
            let existing = ctx.notes.get(&id.to_string()).map(String::as_str).unwrap_or("");
            parts.push(format!(
                "<div class='note'><label for='nb-{id}'>My comment \
                 <span class='note-status' id='ns-{id}'></span></label>\
                 <textarea class='note-box' id='nb-{id}' data-id='{id}' \
                 placeholder='e.g. Step 3 needs the new confirmation dialog \
                 (saved in the app, on this device only)'>{}</textarea></div>",
                esc(existing)
            ));
        }
        parts.push("</div>".into());
    }
    parts.push(format!("</div><script>{HTML_JS}</script>"));
    if let Some(ctx) = note_ctx {
        parts.push(format!(
            "<script>var NOTE_PORT={};var NOTE_ORG={};{NOTE_JS}</script>",
            ctx.port,
            serde_json::to_string(&ctx.org).unwrap_or_else(|_| "\"\"".into())
        ));
    }
    parts.push("</body></html>".into());
    std::fs::write(path, parts.join("\n")).map_err(|e| e.to_string())
}