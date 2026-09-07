# Test Case Manager — presentation brief

**Purpose of this document.** Source material for building a slide deck. It
is organised by the five areas the app targets, and every claim below is a
real, shipped feature — nothing aspirational.

**What the product is, in one line.** A Windows desktop app that lets a
tester write, upload, manage, review and run Azure DevOps test cases
without leaving one window — signing in as themselves, with no
administrator setup.

**Audience note for the deck.** The people in the room are testers and
their managers. The pitch is not "we built software"; it is "the parts of
this job that used to be a browser tab, a spreadsheet and a lot of
clicking are now one screen each."

---

## The problem it solves

Authoring test cases in Azure DevOps' web UI is slow and per-case. There
is no bulk path: creating forty cases means forty forms, and updating them
means finding each one again. Teams end up drafting in spreadsheets, then
copying them in by hand — which is where cases get lost, duplicated, or
quietly diverge from the spec.

This app makes the batch the unit of work: draft many, review them
together, write them in one pass, and manage them as a set afterwards.

---

## 1. Test Case Writing

**Authoring, by hand.** Title, tags, automation status, module,
preconditions, and ordered steps in a numbered action / expected grid.
Module is a dropdown fed by the organisation's own picklist; tags
autocomplete from the tags the project already uses, so a team's
vocabulary stays consistent instead of drifting.

**Authoring, with an AI assistant.** This is the differentiator and worth
its own slide. The app runs a local bridge that connects to the coding
assistants already on the tester's machine, exposing fifteen tools —
including `begin_test_case_writing`, `check_spec_coverage`,
`validate_cases`, `optimize_cases`, `transform_cases`, `search_wiki` and
`get_writing_guide`.

The starting tool hands the assistant a checklist to put to the tester in
chat: what the file is called, which spec documents are authoritative,
whether to check a PBI for duplicates, tags and module, what is out of
scope. Those answers are validated against real paths and values and
written to a plan file **for the tester to approve before a single case
exists**.

The safety property is the headline: **none of those tools can write to
Azure DevOps.** They read, reshape the assistant's own draft, or save
local files. The assistant proposes; the human still presses the button.

**Coverage checking.** `check_spec_coverage` reports which parts of a
specification have no test case yet — so the gap is found before the
review, not during it.

*Suggested slide:* the checklist-before-drafting flow, ending on "the
assistant drafts, the tester approves, and nothing reaches Azure DevOps
until a person says so."

---

## 2. Test Case Uploading

**One batch, not forty forms.** Cases queue up — typed by hand, imported
from a JSON file, or handed over by an assistant — and are written in a
single pass.

**JSON round-trip.** The import format is the same file the app exports,
which makes it AI-editable and human-diffable. A case carrying an id
**updates that exact work item**; a case without one is created. Title
matching is deliberately never treated as an update — only a duplicate
warning — because guessing wrong there means editing the wrong case.

**Files stay live.** An imported file is watched: edit it outside the app
and the changes flow into the queue automatically, with a report of what
moved.

**A review gate that shows the change, not just the count.** Before
anything is written, every queued item gets a git-style diff preview:
creates carry a NEW badge, updates show field old → new with **word-level
diffs** inside each step (added words highlighted, removed struck), and
no-op updates are flagged so they are not written at all.

**Writing is two-stage.** The confirm button spells out *create X ·
update Y*; then a final warning spotlights the target PBI — an animated
border runs around the chip in the context bar — before anything is sent.
A red Cancel stops the batch mid-run. The reason for the second stage is
worth saying out loud: **removing a created case needs delete permission
and only reaches the recycle bin**, so the target gets one last check.

**Afterwards.** A results panel opens with the outcome — *"9 test cases
uploaded — 3 created, 6 updated"* — and each case listed with its new work
item number. Anything that failed keeps a red outline in the queue, so the
one row still needing attention is obvious once everything that worked has
been cleared away.

**Board visibility, handled.** Creating and linking a case is not enough
for it to count on the board. The app finds or creates the PBI's
requirement-based test suite so the cases actually surface where the team
looks.

*Suggested slide:* the diff preview. It photographs well and it is the
thing no spreadsheet can do.

---

## 3. Test Case Management

**Find and select.** Pull a PBI's linked cases; live search by title, id
or tag with a matched / total count; click, ctrl/⌘+click to toggle,
shift+click for a range.

**Bulk edit.** Change status, module, tags (add or replace) and
preconditions across a selection in one action. Titles and steps are
deliberately never touched in bulk — those are the fields where a
mass-edit mistake is expensive.

**Smart grouping.** Cases group by shared title prefixes, so a suite of
forty "Goal Assessment — …" cases collapses into something scannable.
Expanded and collapsed groups are remembered between sessions.

**The suite browser.** The full plan → multi-level folder tree, with a
search that prunes the tree and auto-expands matches. From any folder:
view it as an HTML report, generate an execution report, hand its cases to
Edit, or jump a requirement suite straight to Edit or Run. Folder actions
roll up every descendant.

**Blank fields never wipe data.** An update skips fields left blank, so a
partial import cannot silently clear content that was already there.

---

## 4. Test Case Review

**A read-only tab for reviewing.** The same grouped list, search and
selection, but focused on reading rather than editing.

**Comments that stay local.** Per-case comments are saved on the machine,
scoped per organisation — **nothing is written to Azure DevOps**. A chip
marks commented cases. This matters for a review pass where you want
notes without publishing half-formed opinions to the work item.

**Review in the browser, annotate in place.** One click builds a shareable
HTML report of exactly the chosen cases and opens it. The report carries a
comment box under each case that **autosaves back into the app while it is
open** — so a walkthrough with a colleague can be annotated without
switching windows.

**Running the tests.** Select rows and open a compact always-on-top runner:
step-by-step, marking Passed / Failed / Blocked / Not Applicable per step
and overall, with screenshot capture (region snip, paste, or attach), a
Pin toggle, and File a Bug straight from a failure. Results submit as a
new test run under the existing plan.

**History and failures.** Every case shows its last five outcomes as
coloured dots. Rows expand in place to show the steps and, for failures,
the latest failure detail — the result comment and any linked bugs — so a
fix can be checked against the exact failure without opening the runner.

**Execution reports.** Highlight the cases to report on and one click
builds a shareable HTML summary: pass rate, outcome bar, failures first,
and failure details with comments and linked bugs. Whole-suite and
per-folder reports are available too.

---

## 5. Bonus — Work / Task Management

**A board, not just a list.** To Do / In Progress / Done for your or a
team's work items, with drag-and-drop between columns and a Hide Done
toggle.

**Moves are verified against what Azure DevOps actually saved.** If a
process rule blocks a transition — a required field, a missing date — the
card rolls back and the rule's own message is shown. The board never
displays a state the server rejected. This is the detail to dwell on: it
is the difference between a board that looks right and one that is right.

**A real work-item editor.** Opening a card gives an Azure-DevOps-style
full-window modal: State / Assigned / Activity up top, description and
discussion on the left, Planning and Classification on the right. Save
writes only the fields that were touched.

**Every process tab comes from the organisation's own layout.** A Bug
shows its RCA and Preventive Measures pages, laid out in the same section
columns as DevOps, with text, HTML and picklist fields all editable — not
a hardcoded approximation of one team's process.

**Rich text works properly.** Markdown preview by default, GFM tables, and
attached images fetched with the user's credentials and displayed inline.

**Pull requests too.** The PRs raised by or assigned to the user, with
review conversations, linked work items, and the build that ran for each.

---

## Cross-cutting: why it is safe to hand to a team

This deserves a slide of its own, because it is what makes the app
adoptable in an organisation that is careful about its work items.

- **It cannot destroy work.** Every file in the Rust client is scanned at
  build time and the build fails if it issues a DELETE. There is exactly
  one carved-out exception — moving a test case to the project's recycle
  bin, from which Azure DevOps can restore it — and that file is held to a
  *tighter* rule: the parameter that would erase an item permanently is
  asserted to appear nowhere in it, comments included. Plans, suites, runs,
  attachments, comments, board items and pull requests are never removed.
- **The sign-in token never leaves the Rust core.** It lives in memory for
  the session only — never returned to the UI layer, never written to
  disk, never logged. A test fails the build if a token-shaped field ever
  appears in the interface between the two.
- **No admin setup.** Interactive Microsoft sign-in through the system
  browser using the well-known Azure CLI public client. No Personal Access
  Token to mint and paste, no app registration for anyone to approve. The
  token refreshes silently; a mid-batch expiry prompts re-auth and resumes
  the item it was on.
- **Reads are free; writes are gated.** A review and confirm step precedes
  every create or update, and a rate limiter keeps writes within budget so
  a large batch cannot trip the organisation's throttling.

---

## Cross-cutting: the app itself

Worth one light slide near the end.

- Six full themes — Light, Slate, Midnight, Graphite, Ocean and OLED
  (true black) — plus System, with accent presets layered on top.
- A Ctrl+K command palette and keyboard shortcuts throughout.
- A guided tour that walks a new user through every tab on made-up data
  and leaves nothing behind.
- Installs and updates itself; no toolchain, no manual download.

---

## Suggested deck arc

1. **The problem** — forty cases, forty forms, and a spreadsheet in the middle.
2. **Writing** — authoring, then the AI bridge; land on "the assistant drafts, the human approves."
3. **Uploading** — the diff preview and the two-stage write.
4. **Managing** — search, bulk edit, grouping, the suite tree.
5. **Reviewing** — local comments, the annotated HTML report, the runner, execution reports.
6. **Bonus: the board** — and the "verified against what DevOps saved" detail.
7. **Why it is safe** — no destructive calls, token never leaves the core, no admin setup.
8. **Close** — one window, one sign-in, from spec to run.

## Tone guidance for the slide agent

Prefer the concrete over the adjectival. "Word-level diffs before writing"
lands; "powerful editing capabilities" does not. Where a safety property
is mentioned, give the mechanism — the build fails, the token never
crosses the boundary — because the mechanism is the reassurance. Avoid
claiming performance numbers, user counts or time savings: none have been
measured, and this document deliberately contains none.
