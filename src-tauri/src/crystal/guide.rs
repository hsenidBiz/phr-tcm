//! What an AI assistant reads before working on a Crystal Report.
//!
//! The report tools hand the assistant the bundle's own vocabulary - its
//! IR, its edit ops, its layout plan - so this guide is mostly about the
//! things that vocabulary does not say for itself: the units, where ids
//! come from, that every accepted edit is already saved, and what the
//! bundle cannot do yet, so the assistant says so instead of trying.

/// The guide body, as `get_guide` (topic reports) answers it. Static: it documents
/// the tools and the bundle, not anything about the open report.
pub const GUIDE: &str = r#"# Working with Crystal Reports

TCM holds one open report at a time. Opening another replaces it. Every tool below works on the open report unless it takes a path of its own.

## The loop

1. `open_report` with the report's absolute path. It answers with the report's IR (`crpt-ir/1`): its areas, sections and every object, each with an id.
2. `read_report` to look again, or to look at less: one section (its entry and its objects) or one object, by id or name. Use it on a large report instead of reading the whole IR every time.
3. `edit_report` with a batch of changes. It answers with the new IR.
4. `preview_report` to render the report again. It answers with the page count and the renderer's notes, never the PDF itself; the Crystal Reports screen shows the pages.
5. `check_report` before you call the work done. It runs the bundle's offline load checks and lists errors (Crystal Reports would refuse or garble the file) and warnings (suspect, but usually tolerated).

`compare_reports` takes two `.rpt` paths and lists every difference between them, one `path: before -> after` line each. `open_report` with `create` (true, or `{ title, author, comments }`) creates a blank report at a path - never over a file that is already there - and opens it. `get_guide` with topic `reports` answers with this guide.

## Ids

Object ids (`o0`, `o1`, ...) come from the IR returned by the last call. An edit can renumber them, so after any edit - accepted or refused - use the ids in the IR that call returned, never ones from before it. A designer name works anywhere an id does when it picks out one object.

## Units

- Lengths and positions are twips: 1440 twips = 1 inch, 20 twips = 1 point. Object bounds are relative to their section.
- Font sizes are points.
- Colours are `#RRGGBB`, except the native `set_text_color` op, which takes `r`, `g` and `b` (0-255) separately.

## Two ways to say what to change

`edit_report` takes exactly one of these.

**Native ops** (`ops`): a list of objects, each with an `op` in snake_case. An object is picked with `{"id": "o12"}`, `{"name": "Text3"}` or `{"index": 12}`; a section with `{"area": "ReportHeaderArea1", "section": "ReportHeaderSection1"}` (the area may be left out when the section name is unique).

```json
[
  { "op": "set_text", "object": { "name": "Title" }, "text": "Goal Alignment" },
  { "op": "set_bounds", "object": { "id": "o12" }, "left": 1440, "width": 2880 },
  { "op": "set_font", "object": { "id": "o12" }, "size_pt": 12, "bold": true },
  { "op": "set_text_color", "object": { "id": "o12" }, "r": 0, "g": 51, "b": 102 }
]
```

The ops: `set_text`, `set_object_name`, `set_bounds`, `set_font`, `set_text_color`, `set_alignment`, `set_fill_color`, `set_line_color`, `set_border`, `set_can_grow`, `set_suppress`, `set_number_format`, `set_tooltip`, `set_hyperlink`, `set_css_class`, `set_z_order`, `remove_object`, `copy_object`, `add_text`, `add_line`, `add_box`, `add_field`, `add_special_field`, `add_picture`, `add_subreport`, `set_subreport_link`, `set_section_height`, `set_section_background`, `set_section_break`, `add_section`, `remove_section`, `add_group`, `add_sort`, `add_running_total`, `set_formula`, `set_record_selection`, `add_table`, `remove_table`, `set_table_location`, `add_guideline`, `set_margins`, `set_orientation`, `set_paper_size`.

**A layout plan** (`plan`): the Vibey layout plan, camelCase, objects named by their designer name in `target`.

```json
{ "planVersion": 1, "operations": [
  { "action": "setFontSize", "target": "Title", "fontSizePt": 14 },
  { "action": "setBold", "target": "Title", "bold": true },
  { "action": "move", "target": "Title", "leftTwips": 720, "topTwips": 120 }
] }
```

Each action sets one thing and needs its own fields: `move` takes `leftTwips` and `topTwips`, `resize` takes `widthTwips` and `heightTwips`, `setFont` takes only `fontName`. The actions include `move`, `resize`, `setFont`, `setFontSize`, `setBold`, `setAlignment`, `setTextColor`, `setFillColor`, `setLineColor`, `addText`, `addLine`, `addBox`, `addField`, `removeObject`, `resizeSection`, `setSectionBackground` and `setMargins`. A plan the bundle cannot lower is refused before anything is applied, with one message per bad operation.

## Every edit is saved

A batch the bundle accepts is written over the opened file before the tool answers, so the file, the preview and what you just did always agree. There is no undo inside TCM: the reports live in a git repository, and git is the undo. Say what you changed, so the person can review the diff.

A batch is all or nothing. When the bundle refuses one operation, nothing in that batch is saved, the report is read again from disk, and the answer names the operation (`op #1: ...`, counting from 0) and why. Fix that one and send the batch again.

## What the bundle cannot do yet

Say "that is not supported yet" for these instead of trying; the bundle refuses them itself and TCM passes its message on.

- Text objects with embedded fields are read-only: `set_text` on one is refused, because rewriting it would drop the fields. The IR marks the ones it can rewrite with `"editable": true`.
- The insides of subreports: what a subreport shows cannot be edited from the report that holds it.
- Paper size or orientation on a report saved without a printer.
- Password-protected reports, and reports older than Crystal Reports XI.

## The database

`preview_report` renders from the data saved in the report, or only its static bands when it has none; it never touches a database. Changing stored procedures, views and functions, and a live-data preview, need the Dev login connection, Database Access and Crystal Reports switched on; see the preview_report_live tool.
"#;
