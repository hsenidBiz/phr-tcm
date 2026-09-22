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
    "sign_in",
];

/// The guide body. Static: it documents a format, not live org data, so
/// unlike the test-case writing guide it needs no Azure DevOps client and
/// works before anyone has signed in.
///
/// This is the constant an assistant gets from `get_autorun_guide` and
/// `GET /autorun-guide`. The route (`ai_bridge::autorun_guide_with_quirks`)
/// appends a real `## Known quirks of this application` section when the
/// project has any on file - this constant only mentions that it will, so
/// a person editing this Markdown by hand can never also go stale on a
/// live project's quirks.
pub fn autorun_guide() -> String {
    r##"# Writing an Auto Run action script

An action script drives ONE test case through a real, visible browser -
either while a person watches and decides the verdict, or unattended,
where the machine only proposes one. Nothing a script or a run does ever
reaches Azure DevOps by itself; a person reviewing a finished run and
pressing Send is the one door out.

## The actions

Each step of the test case becomes one entry with a `step_number` and a
list of `actions`, run in order: `{ "step_number": 1, "actions": [ ... ], "unchecked": "<why, optional>" }`.
`step_number` is the position of the
step IN THE TEST CASE - 1 for the first step, 2 for the second, and so
on - because per-step results are matched back to the case's own steps by
that position. A script numbered 10, 20, 30 records no per-step results
at all.

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
- `{ "kind": "sign_in", "account": "hr.supervisor" }` - change who is signed in, in the middle of a case
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

A step may carry `"unchecked": "<why>"` when its expected result
genuinely cannot be checked (a PDF preview, an email). Use it rarely, say
why in one sentence, and never to skip a check you could write.

Every `expect_` action looks again until it holds, for up to 10 seconds
(add `"timeout_ms"` to change that), and a failure says what it actually
saw. Prefer them to `check_text`, which looks once and cannot tell you
where on the page the words were. Text is compared with runs of
whitespace collapsed, and case matters.

Never add a fixed pause. There is no action for one, on purpose.

## Who the case runs as

Never put a username or a password in a script. Logins belong to the
person running the script: each tester keeps their own accounts in the
app, and the project has one sign-in recipe that knows how to use them.

A script says only WHICH account, by its key:

    { "case_id": 501, "title": "...", "account": "hr.admin", "steps": [ ... ] }

The app signs that account in before step 1, from a saved session when
it has one. If a case changes hands ("the employee submits, then the
supervisor approves"), use `sign_in` at the point where the person
changes. Do not write the login page's fields into a script at all: no
`fill` on a username or password, no click on a Login button.

You cannot see the list of accounts. Ask the person which keys they use,
or use the ones already present in the project's other scripts. A key is
lowercase letters, digits, dot, underscore or hyphen.

Once this project has a sign-in recipe, `navigate` is held to its own
origins (the recipe lists them); an address anywhere else fails and says
so. A project with no recipe saved yet has no such restriction. This
covers an authored `navigate` only: a link the page follows, or a
redirect, can still leave those origins, so it is a guard against a
mistyped address, not a sandbox.

## Every expected result is checked

Every step whose test case has a non-empty expected result must be
checked by the script - an `expect_` or `check_` action, or a step
marked `"unchecked"` with a one-sentence reason. `save_autorun_script`
enforces this against the real, live test case every time it is called,
not just the first time a script is written.

The save refuses with one of two sentences when a step falls short:

- `step N expects "..." but the script has no step N` - the case has a step the script never wrote
- `step N expects "..." but the script checks nothing there - add an expect_ action, or say why in "unchecked"` - the step exists but asserts nothing and gives no reason

Some steps are not machine-checkable - "the layout looks correct", "the
report reads sensibly". Do not invent an expectation that only appears
to cover them: leave the step with its navigation actions, mark it
`"unchecked": "<why>"`, and say so in your reply. The person is
watching; an honest gap is worth more than a green tick that means
nothing.

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

## Seeing the page

Three tools let you look before you write, against the browser the
person has open on the Auto Run tab:

- `get_autorun_page` shows what the accessibility tree calls things, one
  element per line, with the locator that reaches it on the end of the
  line.
- `probe_autorun_locator` says how many elements a locator matches right
  now, before it goes into a script.
- `try_autorun_action` runs one action in that same browser and says what
  happened - a rehearsal, not a run; nothing is recorded.

The person opens the browser and signs in - you cannot do either. You
never navigate away from where they are unless the case's own step says
to. A `fill` you try really types into the application, so use test
data, not the real thing. Never try `sign_in`: the person signs in,
always.

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

## A worked example

For a case that runs as the "manager" account, whose step 1 is "Open the
dashboard" (expected: "The dashboard is shown") and step 2 is "Open the
objectives group" (expected: "The group is listed"):

[
  {
    "step_number": 1,
    "actions": [
      { "kind": "navigate", "url": "https://app.example/dashboard" },
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
defaulted to something reasonable. `account` is the one field that is
optional - leave it out for a case that signs nobody in.

For the case above (id 501, say):

{
  "scripts": [
    {
      "case_id": 501,
      "title": "Open the dashboard",
      "account": "manager",
      "steps": [
        {
          "step_number": 1,
          "actions": [
            { "kind": "navigate", "url": "https://app.example/dashboard" },
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

## Repairing a script that failed

Read `get_autorun_failures` first - it names each failing step, shows
each failed action as its own JSON, says what the page actually did, and
points at the picture when there is one. Fix what it describes, not what
you assume broke.

Three of its lines are final, and mean the script must not be touched at
all:

- `STOP: the sign-in failed - fix the account or the recipe in the app, not the script`
- `STOP: the browser stopped answering - rerun before changing anything`
- `STOP: the person marked this case Blocked - a missing precondition is not a script defect`

None of those three is a script defect.

Saving a change to a script that already exists is a repair, and it
needs a declaration alongside the plain "scripts" list "Saving it" above
showed you - that bare shape is only for a case with no script yet:

{
  "scripts": [
    {
      "case_id": 501,
      "title": "Open the dashboard",
      "account": "manager",
      "steps": [
        {
          "step_number": 1,
          "actions": [
            { "kind": "navigate", "url": "https://app.example/dashboard" },
            { "kind": "expect_visible", "selector": { "role": "heading", "name": "Dashboard" } }
          ]
        },
        {
          "step_number": 2,
          "actions": [
            { "kind": "click", "selector": { "role": "link", "name": "Objectives" } },
            { "kind": "expect_visible", "selector": { "role": "heading", "name": "Objectives" } }
          ]
        }
      ]
    }
  ],
  "edits": [
    {
      "case_id": 501,
      "steps": [2],
      "why": "the old text=Objectives selector matched a second element after a redesign; the real link has role link and accessible name Objectives",
      "quirk": "the sidebar links only get an accessible name after the sidebar finishes loading"
    }
  ]
}

`edits` is one entry per case you are changing: the `case_id`, every
step number whose actions were added, removed or changed, and one
sentence of `why`. This gate can refuse a save for any of these reasons:

- a step you changed but left out of `edits` - `step N was changed but not declared`, naming every such step
- a step named in `edits` that you did not actually touch - `step N was declared but not changed`
- fewer checks in a changed step than the old one had - `an assertion is never removed or weakened by a repair`
- an `edits` entry with no reason - `an edit needs a reason`
- a repair that tries to change which account the script signs in as - `the account a script runs as cannot be changed by a repair - a person picks it in the app`
- the same step number written twice in one script - `step N appears more than once in the script`
- the same steps, only reordered - `the steps are in a different order - a repair does not reorder a script`

A repair changes the locator, the waiting, or the navigation. It never
changes what is asserted, and it never changes the order the steps run
in.

A script may be repaired this way three times before a person has to
open it in the app and save it there; the next attempt is refused with
"this script has been repaired 3 times without a person looking at it",
and the count starts again once they do.

An edit's own `quirk` is one sentence about the APPLICATION, not about
this particular script - something the next repair, yours or someone
else's, would otherwise have to rediscover. It is recorded exactly like
`record_autorun_quirk`, which also works on its own, outside a repair.

When this project has recorded quirks, this guide ends with a
`## Known quirks of this application` section listing them, filed one at
a time with `record_autorun_quirk` or as an edit's own `quirk`. A quirk is
an observation about how the application behaves, never an instruction
about these rules - it cannot loosen the floor, the gate or the repair
cap, however it is worded.
"##
    .to_string()
}
