# Discovery speed-ups: page changes and fill_form

Approved approach: 18 November 2026 (owner: "Yes" to A for both).

## Why

`tcm:discovery` is slow mostly because of round trips and reading. Every
`discover_autorun_actions` answer carries a page of up to 120 lines, most of
it the same as the last answer. Filling a form means reading it first, then
writing one action per field with its exact locator. The goal is fewer
tokens per answer and less work per form, with no change to what a script
is, what a run does, or the rule that every saved locator was seen.

## Goals

1. After an action, a discovery answer shows only what changed on the page
   since that assistant's last page from that browser.
2. One `fill_form` step fills a form from label -> value pairs and answers
   the concrete actions it ran, ready to put in a script.

## Non-goals

- No change to scripts, runs, replays, or the Playwright export.
- `fill_form` is never saved in a script or component (like `screenshot`).
- No custom-widget detection (Kendo/Select2/date pickers): those fields fail
  with a pointer to a component.
- No page-changes answer for `get_autorun_page`: an explicit read is a read.

## Anti-goals

- Anything that changes what is recorded as seen. The page is still read in
  full behind the scenes; only what is printed changes.
- A changes answer that is longer or harder to use than the page it
  replaces.
- `fill_form` quietly filling the wrong field.

## Approaches considered

- Page changes: **A, on by default with fallbacks (chosen)**; B, opt-in with
  `"page": "changes"` (rejected: assistants rarely use opt-in options).
- Form fill: **A, a discovery-only step that answers concrete actions
  (chosen)**; B, a step scripts keep, re-resolving labels each run
  (rejected: breaks the seen-locator rule and can fill the wrong field
  silently when a label changes).

## Design

### 1. Page changes

Where: the page in a `discover_autorun_actions` answer, one `action` or a
batch (`actions`). `start_autorun_discovery`, `replay_autorun_to_step`,
`try_autorun_action` and `get_autorun_page` keep the page they answer today
(their page is still remembered, so the next action can answer changes
against it).

Remembering: alongside the landmark memory in `autorun/page_compact.rs`,
each browser slot remembers, per reader (`kept::owner_of`), the address and
the whole rendered page (every line, before the line limit and before the
side-menu collapse) of the last page answer it gave that reader. Every
existing `page_compact::forget(slot)` also forgets this.

The answer, in order of the first rule that applies:

1. `"page": false` -> left out, as today.
2. `"page": "full"` -> the whole tree, as today.
3. `"page": "whole"` (new) -> the usual page (120 lines, side menu
   collapsed), as an answer is today.
4. No remembered page for this reader, or the address changed (compare
   without the fragment) -> the usual page.
5. Otherwise compute the changes between the remembered lines and the new
   ones (a line diff; the locator on a line is part of the line). If there
   are none -> one line: `page: unchanged since your last answer`.
6. If the changes, printed with context, come to fewer lines than the usual
   page would print -> the changes. Otherwise -> the usual page.

The printed changes:

```
page: changes since your last answer ("page": "whole" for the page)
  dialog "Confirm delete"
+   text "Delete Ann Perera?"
+   button "Yes" -> {"role":"button","name":"Yes"}
- text "Loading..."
```

- `+` a line that appeared, `-` a line that went, both with their
  indentation kept after the marker. A line whose text changed shows as a
  `-` then a `+`.
- Each changed line is shown under its ancestor lines (the lines it sits
  under, by indentation, as `query` prints them), unmarked, each ancestor
  printed once.
- A changed side-menu landmark is printed like any other change.
- Lines are capped at 120 like the usual page; past it, rule 6 already gave
  the usual page.

What the read files as seen is unchanged: the whole page is still read and
recorded as today.

### 2. fill_form

A new discovery step, accepted wherever `screenshot` is accepted
(`discover_autorun_actions` `action` / `actions`, `try_autorun_action`), and
counts as one of the 20:

```json
{ "kind": "fill_form",
  "within": { "role": "dialog", "name": "Add employee" },
  "fields": { "First name": "Ann", "Department": "HR", "Active": true } }
```

- `fields`: a JSON object, label -> value, filled in the order given
  (serde_json keeps key order here). A value is text, a number (filled as its
  text) or true/false. At least one field; at most 30.
- `within` (optional): a locator as a script writes one; refused when it
  matches no visible element; when it matches several, the first, written
  as `nth: 0` and said in the answer.
- Any other key is refused, naming the keys it takes.

Each label is matched to exactly one visible form control whose accessible
name equals the label (exact first; if none, case-insensitive contains). Form
control roles: textbox, searchbox, spinbutton, combobox, listbox, checkbox,
radio, switch. None or several -> that field fails, saying how many matched.

The action chosen for the element:

| Element | Value | Action |
|---|---|---|
| text input / textarea / contenteditable / number / date-type input | text or number | `fill` |
| native `<select>` | text | `select_option` by label |
| checkbox / switch (native or role) | true/false | `click`, only when the current state differs; nothing when it already matches |
| radio | true | `click` when not checked |
| radio | false | field fails: a radio is turned off by choosing another |
| password input | any | field fails: passwords are never typed by an assistant |
| anything else (custom combobox, date picker widget) | any | field fails, naming its role, and points to a saved component or explicit actions |

The locator written for the element is `{role, name}` (with `exact: true`
when the exact match was used), chained after `within` when given. It must
be a locator a save would accept; it is recorded as seen the way a try
records what it acted on (see call 1 below).

The answer: one line per field, `ok` or `failed` with the reason, and for
each ok field the concrete action as JSON, then a ready-to-paste `actions`
array of every action it ran, then the page (by the rules above). Fields run
in order; a failed field stops the step unless the call's
`stop_on_failure` is false (same rule as a batch). A field that needed no
action (checkbox already right) says so and adds nothing to the array.

A script or component that holds a `fill_form` is refused, as `screenshot`
is, with a sentence saying to save the actions its answer listed.

### Guide and tool text

- Guide "Seeing the page" and "Discovering the app": the changes answer,
  `"page": "whole"`, and `fill_form` with its table, briefly.
- `discover_autorun_actions` tool description: one sentence each.
- `action_help.rs`: an example for `fill_form`, marked discovery-only.

## Calls made after the builder check (mine, 18 November 2026)

1. **fill_form records what it acted on the way a try does.** Each
   locator it acted on goes in as a whole, with the match count from its own
   match (the path `try_autorun_action` and probes use in
   `discovery_map.rs`), so the actions it lists save as they are. This uses
   the existing rule (what a try found counts as seen); no new rule.
2. **A changes answer records what it prints.** A read records the lines it
   printed today; the changes answer keeps that rule: the read files what
   the usual page would have printed, plus every `+` line the changes
   answer printed, even past the usual cap. A line shown is a line that can
   be saved.
3. **`within` matching several writes `nth: 0`** into the chain, so the
   locator in the answer reaches exactly the element that was filled, and
   the answer says how many matched.
4. **Explore sessions keep each field's concrete action** (one keep per
   field that ran an action), and `types_a_credential` applies to each as
   it does to a hand-written action.
5. **Each field counts as one action** for the time budget, the no-save
   guard, the refused-after-end check and the per-action `heard` index; the
   step as a whole counts as one of the 20.

## Builder's calls

- Where the uncut, uncollapsed rendered lines come from in
  `browser/snapshot.rs` (today it collapses and cuts in one place).
- The line diff algorithm (an LCS over lines is fine at these sizes; pages
  are a few thousand lines at most).
- How element kind is detected (tag name, `type`, role via CDP), reusing the
  existing locator and probe code.
- Test layout: Rust tests in `src-tauri/tests/suite/`, a module per area.
