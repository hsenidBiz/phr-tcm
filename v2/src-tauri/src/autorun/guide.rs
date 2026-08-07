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

- `{ "kind": "navigate", "url": "https://..." }`
- `{ "kind": "click", "selector": "..." }`
- `{ "kind": "fill", "selector": "...", "value": "..." }`
- `{ "kind": "wait_for", "selector": "...", "timeout_ms": 5000 }`
- `{ "kind": "check_text", "value": "..." }`  - is this text on the page?
- `{ "kind": "check_url", "contains": "..." }` - is this in the address?

There is nothing else. An action of any other kind is rejected.

## Selectors

A selector is either a CSS selector, or `text=Some Words` to find the
element whose visible text contains those words.

Prefer, in this order: a `data-testid`, then an `id`, then `text=`.
`text=` is the most readable and the most fragile - it breaks when the
wording changes, which is exactly when a human would notice anyway.

If you can read the application's source, USE IT to find selectors. That
is what source access is for: the real id of a button beats a guess every
time. Read the component, take the id, move on.

## Where each fact is allowed to come from

This is the important part, and the one that goes wrong quietly.

- **The application's source: locators and navigation ONLY.** How to
  reach a screen and how to address a control. Never what the correct
  outcome is.
- **The test case's expected result: every assertion.** A human wrote it
  without reading the code, which is the whole point. `check_text` and
  `check_url` come from there and nowhere else.
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
report reads sensibly". Do not invent a `check_text` that only appears to
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
      { "kind": "click", "selector": "text=Sign in" },
      { "kind": "check_text", "value": "Dashboard" }
    ]
  },
  {
    "step_number": 2,
    "actions": [
      { "kind": "click", "selector": "text=Objectives" },
      { "kind": "check_text", "value": "Objectives" }
    ]
  }
]

## Saving it

Call `save_autorun_script` with the case id and that array. The app picks
it up immediately - the case's badge turns "Script ready" and a Run
button appears beside it.

## When a script is already failing

The runner reports each action's outcome in plain words, e.g.
"not found: #nope". That names the broken selector directly - read the
source again for the right one and save a corrected script. Do not weaken
a check to make a run go green.
"##
    .to_string()
}
