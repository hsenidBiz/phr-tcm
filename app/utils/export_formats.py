"""HTML (human-readable) and JSON (AI-editable) test case exports.

Both exporters consume plain "record" dicts so the Review queue (TestCase
objects) and the Edit tab (raw work-item field dicts) share one code path:

    {"id": int | None, "title": str, "tags": str, "automation_status": str,
     "module": str, "preconditions": str,
     "steps": [{"action": str, "expected": str}, ...]}

The JSON document written by ``export_records_to_json`` is the canonical AI
round-trip format: ``import_parser.parse_file`` reads it back, and a kept
``id`` updates that work item instead of creating a duplicate — same contract
as the TestCaseID column in the spreadsheet format.
"""

import html
import json
import re
from pathlib import Path

AI_FORMAT_NAME = "azure-devops-test-cases"
AI_FORMAT_VERSION = 1

AI_INSTRUCTIONS = (
    "Each entry in test_cases is one Azure DevOps Test Case. Edit this file "
    "freely but keep it valid JSON with this exact structure. Rules: keep "
    "'id' unchanged so re-importing UPDATES that existing work item; set 'id' "
    "to null to CREATE a new test case. 'title' is required (max 255 chars). "
    "'steps' is an ordered list; every step needs a non-empty 'action', "
    "'expected' may be an empty string. 'automation_status' must be exactly "
    "'Not Automated' or 'Planned'. 'tags' is a single semicolon-separated "
    "string — commas are not allowed in tags. 'module' and 'preconditions' "
    "are free text and may be empty strings."
)

_KNOWN_EXTS = (".html", ".json", ".xlsx")


def resolve_export_path(path: str, selected_filter: str) -> tuple:
    """Normalize a save-dialog result to (path, ext).

    ``ext`` is one of '.html', '.json', '.xlsx'. When the typed file name has
    no recognised extension, the selected filter's extension is appended so
    the exported file always opens with the right application.
    """
    suffix = Path(path).suffix.lower()
    if suffix == ".htm":
        suffix = ".html"
    if suffix in _KNOWN_EXTS:
        return path, suffix
    m = re.search(r"\*(\.\w+)", selected_filter or "")
    ext = m.group(1).lower() if m else ".html"
    if ext not in _KNOWN_EXTS:
        ext = ".html"
    return path + ext, ext


# --------------------------------------------------------------------- #
#  Record builders                                                       #
# --------------------------------------------------------------------- #

def queue_to_records(queue: list) -> list:
    """TestCase objects (Review queue) → export records."""
    return [{
        "id": tc.update_id,
        "title": tc.title,
        "tags": tc.tags,
        "automation_status": tc.automation_status,
        "module": tc.module_value,
        "preconditions": tc.preconditions,
        "steps": [{"action": s.action, "expected": s.expected} for s in tc.steps],
    } for tc in queue]


def cases_to_records(cases: list, module_ref: str | None,
                     preconditions_ref: str | None) -> list:
    """DevOps API case dicts (Edit tab) → export records."""
    from app.utils.xml_builder import parse_steps_xml
    records = []
    for tc in cases:
        steps = parse_steps_xml(tc.get("Microsoft.VSTS.TCM.Steps", "") or "")
        records.append({
            "id": tc.get("_id") or None,
            "title": tc.get("System.Title", "") or "",
            "tags": tc.get("System.Tags", "") or "",
            "automation_status": tc.get("Microsoft.VSTS.TCM.AutomationStatus", "")
                                 or "Not Automated",
            "module": (tc.get(module_ref, "") if module_ref else "") or "",
            "preconditions": (tc.get(preconditions_ref, "") if preconditions_ref else "") or "",
            "steps": [{"action": s.action, "expected": s.expected} for s in steps],
        })
    return records


# --------------------------------------------------------------------- #
#  JSON export (AI round-trip format)                                    #
# --------------------------------------------------------------------- #

def export_records_to_json(records: list, path: str):
    doc = {
        "format": AI_FORMAT_NAME,
        "version": AI_FORMAT_VERSION,
        "instructions": AI_INSTRUCTIONS,
        "test_cases": records,
    }
    with open(path, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=2, ensure_ascii=False)
        f.write("\n")


# --------------------------------------------------------------------- #
#  HTML export (human-readable report)                                   #
# --------------------------------------------------------------------- #

def _esc(text) -> str:
    return html.escape(str(text or ""), quote=True)


_HTML_CSS = """
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
@media print { body { background: #fff; padding: 0; }
               .case { box-shadow: none; border-color: #ccc; }
               .searchbar { display: none; } }
"""

# Filters the report client-side: every space-separated word must appear
# somewhere in a card's text (title, #id, tags, steps, preconditions).
_HTML_JS = """
(function () {
  var input = document.getElementById('tc-search');
  var count = document.getElementById('tc-count');
  var noMatch = document.getElementById('tc-no-match');
  var cards = Array.prototype.slice.call(document.querySelectorAll('.case'));
  var texts = cards.map(function (c) { return c.textContent.toLowerCase(); });
  var total = cards.length;

  function apply() {
    var words = input.value.toLowerCase().split(/\\s+/).filter(Boolean);
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
"""


def export_records_to_html(records: list, path: str, subtitle: str = ""):
    """Write a standalone, print-friendly HTML report of the given records."""
    parts = [
        "<!DOCTYPE html>",
        '<html lang="en"><head><meta charset="utf-8">',
        '<meta name="viewport" content="width=device-width, initial-scale=1">',
        f"<title>Test Cases ({len(records)})</title>",
        f"<style>{_HTML_CSS}</style></head><body><div class='page'>",
        "<h1>Test Cases</h1>",
        f"<p class='subtitle'>{_esc(subtitle) or f'{len(records)} test case(s)'}</p>",
        "<div class='searchbar'>",
        "<input id='tc-search' type='search' "
        "placeholder='Search title, ID, tags, steps, prerequisites…' "
        "aria-label='Search test cases'>",
        "<span id='tc-count'></span></div>",
        "<p id='tc-no-match' class='no-match hidden'>No test cases match your search.</p>",
    ]
    for rec in records:
        parts.append("<div class='case'>")
        wid = f"<span class='wid'>#{_esc(rec.get('id'))}</span>" if rec.get("id") else ""
        parts.append(f"<h2>{wid}{_esc(rec.get('title'))}</h2>")

        chips = []
        status = rec.get("automation_status")
        if status:
            chips.append(f"<span class='chip status'>{_esc(status)}</span>")
        module = rec.get("module")
        if module:
            chips.append(f"<span class='chip module'>{_esc(module)}</span>")
        for tag in (rec.get("tags") or "").split(";"):
            tag = tag.strip()
            if tag:
                chips.append(f"<span class='chip'>{_esc(tag)}</span>")
        if chips:
            parts.append(f"<div class='meta'>{''.join(chips)}</div>")

        # Every case shows a Prerequisites block, even when the field is empty,
        # so reviewers can see at a glance that none were specified.
        prereq = (rec.get("preconditions") or "").strip()
        prereq_html = _esc(prereq) if prereq else "<span class='none'>None</span>"
        parts.append(f"<p class='pre'><b>Prerequisites:</b> {prereq_html}</p>")

        steps = rec.get("steps") or []
        if steps:
            parts.append("<table><tr><th>#</th><th>Action</th><th>Expected result</th></tr>")
            for i, step in enumerate(steps, start=1):
                parts.append(
                    f"<tr><td class='num'>{i}</td>"
                    f"<td>{_esc(step.get('action'))}</td>"
                    f"<td>{_esc(step.get('expected'))}</td></tr>"
                )
            parts.append("</table>")
        parts.append("</div>")
    parts.append(f"</div><script>{_HTML_JS}</script></body></html>")

    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(parts))
