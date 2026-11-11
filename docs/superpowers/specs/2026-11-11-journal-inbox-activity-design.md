# Upload journal, Waiting for you, and assistant activity (backlog A2 F6, F8, F9)

Date: 2026-11-11. Owner decisions are marked **(owner)**; everything else
marked **(my call)** is the designer's and may be corrected.

## Why

Three gaps from the 2026-11-11 software assessment:

- **F6.** If the app stops (crash, power, kill) between sending an upload's
  `$batch` and applying its result, nothing records that the upload went
  out. Azure DevOps may have created the cases while the queue still shows
  the rows as fresh creates; only the duplicate-title warning stands between
  the person and a duplicate this app can never delete (no DELETE, by
  invariant).
- **F8.** Things an assistant leaves for the person to decide are scattered:
  proposed accounts (Accounts dialog, Proposed tab), a proposed sign-in
  (the sign-in notice in Auto Run setup), and case divergences that need a
  ruling (a mark on a case card, which does not even refresh live). Nothing
  tells the person something is waiting.
- **F9.** What an assistant is doing is only visible on the Auto Run screen,
  and only while it is busy (the Assistants card). A sign-in trial shows
  nowhere. There is no record of what an assistant saved, proposed or tried.

The person usually drives the assistant from Claude Code while looking at
some other screen of the app, which is exactly when these are missed.

## Goals

1. An interrupted upload always comes back as the existing "Outcome
   unknown" hold, so Check is the only way forward.
2. One list, **Waiting for you**, of every assistant item awaiting the
   person's decision, with a way to open each where it is decided.
3. The person learns something new is waiting without being on Auto Run.
4. While any assistant works, every screen shows what it is doing, with Stop.
5. A timeline of what assistants saved, proposed, tried and changed.

## Non-goals

- Spreadsheet import/export (F1 declined **(owner)**: JSON stays the only format).
- New decisions in the list itself: each row opens the existing place
  (Accounts → Proposed, the sign-in notice, the case card). No second
  approve path (lean controls).
- OS toast notifications **(my call)**: the in-app bell plus a sidebar count
  is the notification. `tray.rs` stays as it is.
- Changing what assistants may do, or what Stop ends.
- Showing the existing Db/Api activity records in the timeline **(my call)**:
  they are detailed statement logs kept out of the UI on purpose.

## Anti-goals (a failure even if it works)

- The activity strip covering or swallowing clicks. It sits in the layout
  flow, pushing content down, never an overlay. jsdom cannot catch this;
  it needs a hand walk-through.
- Any argument, value, login, URL, path, SQL or request body in the timeline,
  the strip or the list. Labels, case ids, setup/area names the person
  already sees elsewhere are fine; secrets never.
- Naming a hidden feature anywhere (UI copy, changelog, guide, commits).
- A hold or journal ever being lifted by anything other than Check or Release.

## Constraints (from the project)

- No HTTP DELETE to Azure DevOps. F6 adds no network calls.
- Rust owns disk I/O and secrets; frontend talks through generated
  `bindings.ts` (regenerate, never hand-edit).
- Rust tests only in `src-tauri/tests/suite/`; process-wide state takes a
  `serial` lock.
- Colours from tokens; icons from `src/lib/actionIcons.ts`;
  `ui-consistency.test.ts` must keep passing unweakened.
- Auto Run and everything assistant-related appear only where Auto Run is
  available today (its module switched on); off, none of F8/F9 shows.
- Per-environment Auto Run data: the **active environment only** **(owner,
  earlier ruling)**.
- User-facing text names no URL and no path.
- Changelog entry is end-user-facing.

## Approaches

**F6.** One sensible way **(owner approved as proposed)**: a Rust-side
journal written before the first `$batch` send. Alternatives dropped: a
frontend localStorage marker written before calling `submitQueue` (the
webview can die mid-write, and Rust is the side that knows when sending
starts); reconciling every PBI on launch (network calls on every start,
and still nothing to say *which* rows were sent).

**F8/F9 scope.** Two options were put to the owner: everything across the
whole app (strip on every screen, bell + sidebar count) or only on the Auto
Run screen. **Across the whole app (owner).**

**F9 timeline source.** Record at the one choke point every assistant call
already passes (`ai_bridge::route_timed`), from a curated table of routes,
versus instrumenting each handler. The choke point **(my call)**: one place,
the caller's label is already there, and it can only ever see method, path
and status, so it cannot leak arguments by accident.

## Design

### F6: upload journal

- **Write.** In `commands/queue.rs` `submit_queue`, right after
  `upload_since` is computed and before the first `$batch` chunk is sent,
  Rust writes a journal file for `(org, project, pbi_id)` holding: `since`
  (the same value as `upload_since`), one title per **create** row being
  sent, each row's content signature in the same form the frontend's
  `holdSignature` uses (or the data needed to compute it, builder's choice,
  as long as a hold made from the journal marks the same rows a normal hold
  would), and the ids already linked to the PBI before the upload (the
  `ids` a hold carries). Updates are not journaled: a re-sent update is the
  same PATCH.
- **Location.** A small JSON file per PBI under the app's local data folder
  (`upload-journal/`), written atomically (temp + rename). File names carry
  no secret; org and PBI may be hashed or escaped, builder's choice.
- **Clear.** Rust never deletes it on its own after sending. The frontend
  clears it (new command) only **after** `applyOutcome` has saved any hold
  (or confirmed none is needed). So a crash anywhere between "send" and
  "hold saved" leaves the journal behind.
- **Recover.** When the queue for a PBI loads, the frontend asks for that
  PBI's journal. If one exists it becomes a hold through the existing
  `saveHold` path: every journaled title held as unknown (not ambiguous),
  `since`, `ids`, `sigs`. If a hold already exists for that PBI, the two are
  merged (union of titles/sigs, earliest `since`, union of ids). Then the
  journal is cleared. The banner reads as today's "Outcome unknown, check
  before uploading again"; Check (`reconcile_upload`) and Release work
  unchanged.
- **Not opened PBIs.** On sign-in, Rust lists journals for the signed-in
  org and the frontend raises one bell notification per PBI, kind
  `upload-interrupted`, "An upload to PBI {id} was interrupted", whose click
  opens that PBI (the journal then becomes a hold as above). Dedupe id
  includes the journal's `since`, so the same interruption is raised once.
- **Account switch.** Journals belong to the org they were written for; a
  different account never sees them, and they are not wiped (a wiped
  journal is a lost hold).

### F8: Waiting for you

- **Items** (active environment, current project) **(my call on the set)**:
  1. each proposed account awaiting approval;
  2. a proposed sign-in awaiting approval;
  3. each case divergence awaiting a ruling.
  Suspected defects are left out: they are findings shown on the case and
  in run results, not a decision the assistant is waiting on.
- **Rust.** One command returns the items: kind, a short title (account
  key, "Sign-in for {site label}", "Case {id}: {short reason}"), when it was
  proposed, and a target the frontend can open. Reads the same files the
  three existing places read; no new store.
- **Live.** Writing a divergence (report or ruling) now emits
  `AutorunDataChanged` with a new kind `Divergences`, so case cards and the
  list refresh like proposals already do. The list's query is invalidated by
  `Proposals`, `SignInProposal` and `Divergences`.
- **Where it shows.**
  - Top of the Auto Run screen: a **Waiting for you** section, hidden when
    empty. Each row: icon by kind, title, age, and **Open** taking the
    person to the existing place (Accounts dialog on the Proposed tab; the
    sign-in notice; the case card scrolled into view and highlighted).
  - Sidebar: Auto Run's entry shows the count as a small badge (same badge
    style as the bell's unread count).
  - Bell: a new notification kind `assistant-waiting`, raised once per new
    item (dedupe id = kind + item key), whose click opens Auto Run with the
    item in view. Deciding the item does not need the bell entry dismissed.
- **Copy** is plain and names the assistant only as "the assistant" or its
  label as the Assistants card shows it.

### F9: assistant activity

**Activity strip (every screen).**
- Mounted once in `App.tsx`, in the layout flow above `<main>`, below any
  existing top bar. Visible when any assistant's activity is not idle, or a
  sign-in trial is running; otherwise renders nothing (zero height).
- One line per working assistant, at most three lines then "and N more":
  label · what it is doing (the same wording the Assistants card uses:
  discovering {area}, mapping {modules}, exploring, replaying case {id} step
  N of M, working) · browser · account key.
- **Stop** on a line when that assistant's `can_stop` is true, doing exactly
  what the card's Stop does. **Open** goes to the Auto Run screen. End
  discovery and Release stay on the card only **(my call, lean)**.
- Fed by the existing `AutorunAgentsChanged` event and
  `commands.autoRunAgents()`; the strip and the card share one hook.
- **Sign-in trials.** A trial today claims the check browser with no
  account and no activity. It now reports itself so the strip shows
  "Trying a sign-in" for the environment (no site URL, no login). Builder
  chooses between a new `AgentActivity` variant and a separate trial field
  in the agents payload. Stop appears only if a trial can actually be
  cancelled safely today; otherwise none.

**Timeline.**
- New `activity_log` kind `Assistant` → `assistant-YYYY-MM-DD.jsonl`, kept
  30 days like the others.
- Recorded in `ai_bridge::route_timed` after the answer, for routes in a
  curated table only. Each record: time, the caller's label (as the card
  shows it), a fixed action sentence from the table, ok/refused (status
  < 400), and the case id when the route names one in the body's
  well-known field. Nothing else from the body or query, ever.
- Table, grouped:
  - **Saved**: script, component, area, setup (and setup removed), quirk,
    validation rule, test file, API template / fixture / flow, run order.
  - **Proposed**: accounts, sign-in, divergence reported, suspected defect
    marked/cleared, divergence ruled.
  - **Tried**: try an action, replay to step, run setup, check sign-in,
    discovery started/ended, prove/run API template.
  - **Changed data**: explore change recorded, explore creates undone.
  Read-only routes (pages, lists, guides, lookups) are never recorded.
- Shown on the Auto Run screen under Waiting for you as **Recent activity**:
  the last 7 days, newest first, grouped by day, each row time · assistant
  · sentence · case link when present; refused calls shown muted. A filter
  by assistant when more than one appears. Refreshes on a throttled event
  (at most once a second) emitted when a record is written.
- The existing "open activity folder" stays where it is.

### Docs, changelog, backlog

- Changelog: three end-user lines (interrupted uploads are held for a
  Check; Waiting for you with a count and bell notice; what the assistant
  is doing on every screen, with Stop, and its recent activity).
- How To Use guide: the Auto Run screen and queue hold pages mention the
  new parts; re-take affected shots per the docs-shots runbook.
- Backlog A2: F6, F8, F9 marked shipped with the version; F1 struck as
  declined (already edited).

## Left to the builder

- Exact Rust module names and command names; the journal file naming.
- Whether the signature is stored or recomputed (constraint above).
- Trial reporting shape (activity variant vs separate field).
- The exact route table entries (from the groups above) and their sentences.
- Component file layout under `src/components/` and `src/screens/AutoRun/`.
