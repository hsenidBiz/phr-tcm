//! What an AI assistant reads before writing an action script.
//!
//! The assistant writing these scripts can usually see three things at
//! once: the test case (through the bridge), the app's own source (it is
//! a coding assistant, opened in that repo), and the database (through
//! the company's DB MCP server). That combination is what makes generated
//! scripts worth having - and it is also the trap this guide exists to
//! close. An assistant that reads an implementation and asserts what the
//! code currently does has written a mirror, not a test: it will pass
//! through the exact regression it was supposed to catch.
//!
//! So the guide is mostly about which source is allowed to decide what.

/// Every `kind` the runner's executor understands, in the order the guide
/// introduces them. Kept in step with `browser::actions::Action` by hand;
/// `tests/autorun_guide.rs` fails if this drifts from what serde emits.
pub const ACTION_KINDS: &[&str] = &[
    "navigate",
    "click",
    "fill",
    "wait_for",
    "check_text",
    "check_url",
    "expect_visible",
    "expect_hidden",
    "expect_text",
    "expect_contains_text",
    "expect_count",
    "expect_attribute",
];

/// The guide body. Static: it documents a format, not live org data, so
/// unlike the test-case writing guide it needs no Azure DevOps client and
/// works before anyone has signed in.
pub fn autorun_guide() -> String {
    r##"# Writing an Auto Run action script

An action script drives ONE test case through a real, visible browser
while a person watches. You write the actions; the person watching
decides the verdict. Nothing you write is ever sent to Azure DevOps.

## The actions

Each step of the test case becomes one entry with a `step_number` and a
list of `actions`, run in order:

- `{ "kind": "navigate", "url": "https://..." }` - waits for the page to load
- `{ "kind": "click", "selector": ... }`
- `{ "kind": "fill", "selector": ..., "value": "..." }` - also picks an
  option in a native list by its label, and sets a date, time, colour or
  range field directly
- `{ "kind": "wait_for", "selector": ..., "timeout_ms": 5000 }`
- `{ "kind": "expect_visible", "selector": ... }`
- `{ "kind": "expect_hidden", "selector": ... }` - gone counts as hidden
- `{ "kind": "expect_text", "selector": ..., "equals": "..." }`
- `{ "kind": "expect_contains_text", "selector": ..., "value": "..." }`
- `{ "kind": "expect_count", "selector": ..., "equals": 3 }`
- `{ "kind": "expect_attribute", "selector": ..., "name": "aria-checked", "equals": "true" }`
- `{ "kind": "check_text", "value": "..." }`  - is this text anywhere on the page, right now?
- `{ "kind": "check_url", "contains": "..." }` - is this in the address, right now?

There is nothing else. An action of any other kind is rejected.

`click` and `fill` wait up to 15 seconds for their element to be usable:
the only match, visible, inside the visible part of the page, not
moving, enabled, and not covered by something else. Then they use the
real mouse and keyboard. If the wait runs out, the outcome says which of
those was the problem. You do not need a `wait_for` in front of them.

One thing `fill` does not do: typing is delivered as a text commit, not
keystroke by keystroke, so a page that reacts to keydown (type-ahead
search, some autocompletes) will not see keys. The field still gets the
input events it would from a person. Only clearing a field sends a real
Backspace.

Every `expect_` action looks again until it holds, for up to 10 seconds
(add `"timeout_ms"` to change that), and a failure says what it actually
saw. Prefer them to `check_text`, which looks once and cannot tell you
where on the page the words were. Text is compared with runs of
whitespace collapsed, and case matters.

Never add a fixed pause. There is no action for one, on purpose.

## Selectors

A `selector` says which element. The best form is a locator, because it
says it the way the test case does - by what the control IS and what it
is CALLED:

- `{ "role": "button", "name": "Add Method" }`
- `{ "role": "textbox", "name": "Method Name" }`
- `{ "role": "dialog", "name": "Add Rating Method" }`
- `{ "text": "Step 1 of 6" }` - the deepest element showing those words
- `{ "css": "#save" }` - for a real id or data-testid

`role` is the ARIA role (button, link, textbox, checkbox, combobox,
searchbox, dialog, heading, row, cell...) and `name` is the
accessible name: a button's words, a field's label. Both come from the browser's own
accessibility tree, so they match what a screen reader would announce.

`name` and `text` match loosely - contains, ignoring case and extra
spaces. Add `"exact": true` when a near miss exists ("Save" and "Save as
new").

A list narrows from left to right, each entry searching inside the one
before it:

    [ { "role": "dialog", "name": "Add Rating Method" },
      { "role": "button", "name": "Add Method" } ]

Only what a person could SEE is matched. Applications keep hidden copies
of dialogs and menus in the page; they are ignored unless an entry says
`"visible": false`.

A locator must end up meaning exactly one element. If it matches several,
the action fails and says how many: narrow it with a list, `"exact"`, or
`"nth"` (zero-based: `{ "css": "tbody tr", "nth": 0 }` is the first row).
`nth` picks from THAT entry's matches across everything the previous
entry matched, taken in page order, not from the matches inside one of
them. Only `expect_count` and `expect_hidden` are happy with many.

A misspelt field is rejected, not ignored. Each entry takes exactly one
of `role`, `text` or `css`.

A plain string still works and means what it always has: a CSS selector
(first match), or `text=Some Words` (last match). It has no visibility
filter, so prefer a locator in anything new.

Prefer, in this order: `role` with `name`, then a `data-testid` or `id`
through `css`, then `text`. Words are the most readable and the most
fragile - they break when the wording changes, which is exactly when a
human would notice anyway.

If you can read the application's source, USE IT to find selectors. That
is what source access is for: the real label of a field or id of a button
beats a guess every time. Read the component, take it, move on.

## Three things that make a source-derived selector wrong

Reading the source is right, but the id you find is not always the id
that exists at runtime. Check for these before you trust one:

1. **Component libraries that wrap their real control.** A tag like
   `<x-button id="save-host">` is often a HOST: the real `<button>` is
   injected as a child at init, commonly with an id like
   `save-host-button`, and text inputs likewise become `...-input`.
   Clicking the host does nothing. Find the library's init code and see
   what id it actually gives the control.
2. **Ids built from data.** `group-header-gg-4711` is stable for one
   record on one machine and wrong everywhere else. Match on the visible
   text instead.
3. **Markup that does not exist yet.** Grids and cards rendered from an
   AJAX response, or cloned from a `<template>` when a modal opens, are
   absent at page load. `click`, `fill` and every `expect_` action wait
   for them; `check_text` does not, so follow a navigation with an
   `expect_visible` on something inside the rendered result.

Content inside an `<iframe>` cannot be reached at all: the actions run in
the top document only. If a step depends on one, say so and leave it for
the person to do by hand.

## Where each fact is allowed to come from

This is the important part, and the one that goes wrong quietly.

- **The application's source: locators and navigation ONLY.** How to
  reach a screen and how to address a control. Never what the correct
  outcome is.
- **The test case's expected result: every assertion.** A human wrote it
  without reading the code, which is the whole point. Every `expect_` and
  `check_` action comes from there and nowhere else.
- **The wiki specification: the tiebreaker.** When a case is vague, the
  spec outranks both the case and the code. Use `search_wiki` and
  `get_wiki_page`.
- **The database: verifying effects.** Useful precisely because it does
  not go through the UI you just read. Out of scope for the script
  itself, but worth checking by hand when a case is about data.

If you find yourself writing an assertion because "that is what the code
does", stop. You are about to automate the bug.

## Steps you cannot automate

Some steps are not machine-checkable - "the layout looks correct", "the
report reads sensibly". Do not invent an expectation that only appears to
cover them. Leave the step with its navigation actions and no check, and
say so in your reply. The person is watching; an honest gap is worth more
than a green tick that means nothing.

## A worked example

For a case whose step 1 is "Sign in as a manager" (expected: "The
dashboard is shown") and step 2 is "Open the objectives group" (expected:
"The group is listed"):

[
  {
    "step_number": 1,
    "actions": [
      { "kind": "navigate", "url": "https://app.example/login" },
      { "kind": "wait_for", "selector": "#username", "timeout_ms": 5000 },
      { "kind": "fill", "selector": "#username", "value": "manager@example" },
      { "kind": "fill", "selector": "#password", "value": "REPLACE_ME" },
      { "kind": "click", "selector": { "role": "button", "name": "Sign in" } },
      { "kind": "expect_visible", "selector": { "role": "heading", "name": "Dashboard" } }
    ]
  },
  {
    "step_number": 2,
    "actions": [
      { "kind": "click", "selector": "text=Objectives" },
      { "kind": "expect_visible", "selector": { "role": "heading", "name": "Objectives" } }
    ]
  }
]

## Saving it

`save_autorun_script` does NOT take the steps array on its own - it takes
a LIST of scripts, one entry per case, so a whole PBI can be saved in one
call. Every field below is required; there are no defaults, including
`wait_for`'s `timeout_ms` - leave it out and the save is rejected, not
defaulted to something reasonable.

For the case above (id 501, say):

{
  "scripts": [
    {
      "case_id": 501,
      "title": "Sign in as a manager",
      "steps": [
        {
          "step_number": 1,
          "actions": [
            { "kind": "navigate", "url": "https://app.example/login" },
            { "kind": "wait_for", "selector": "#username", "timeout_ms": 5000 },
            { "kind": "fill", "selector": "#username", "value": "manager@example" },
            { "kind": "fill", "selector": "#password", "value": "REPLACE_ME" },
            { "kind": "click", "selector": { "role": "button", "name": "Sign in" } },
            { "kind": "expect_visible", "selector": { "role": "heading", "name": "Dashboard" } }
          ]
        },
        {
          "step_number": 2,
          "actions": [
            { "kind": "click", "selector": "text=Objectives" },
            { "kind": "expect_visible", "selector": { "role": "heading", "name": "Objectives" } }
          ]
        }
      ]
    }
  ]
}

One case is a bundle of one - it still goes through `scripts` as a
one-entry list, not the array of steps by itself.

A case id must be a real, positive Azure DevOps work item id, cannot
repeat within the same call, and its `steps` cannot be empty - a script
with nothing to run does not earn a "Script ready" badge.

The app does NOT necessarily pick this up right away. The Auto Run screen
disables refetch-on-window-focus (alt-tabbing back to the app refetches
nothing), so if it is already open when you save, its "Script ready"
badges will not update until something in the app explicitly asks it to -
switching PBI, reopening the screen, or an edit made from its own script
editor. If the person watching says the badge has not changed, tell them
to navigate away from Auto Run and back rather than just switching
windows.

## When a script is already failing

The runner reports each action's outcome in plain words, e.g.
"waited 15000ms: button "Save" is covered by div.modal-backdrop" or
"expected text "Saved" but saw "Saving..."". That names the problem
directly - read the source again for the right locator and save a
corrected script. A failed action also keeps a screenshot the person can
open. Do not weaken a check to make a run go green.
"##
    .to_string()
}
