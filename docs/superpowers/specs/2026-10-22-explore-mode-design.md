# Explore mode: the assistant does a job in the app with no test case

Status: draft for the owner's review, 2026-10-22 (after the builder check and the owner's answers on deleting).

## Intent

The owner wants to run tests that have no test case behind them. They give the assistant a plain task, for example "set up a Draft cycle with three employees, publish it, check every employee got a task and the cycle shows Published". The assistant does it on its own:
- it sets up the preconditions;
- it does the thing;
- it checks what happened;
- it reports back.

Today the assistant can already drive the Auto Run browser without a case, but every tool frames the work as writing a script, and it cannot fire an API call without saving a proven template first (or at all, while its own browser holds the account). This design adds a mode for doing a job rather than scripting one.

**What the owner said:**
- The assistant does the checks itself.
- The result is a written summary in the chat, or a .md file. Nothing is kept in the app.
- What it creates or changes stays in the app, unless the person asks to remove it or undo it.
- Every API call that works may be saved as an API template for later: "there's no harm saving something that can be reused much later". A call that fails is not saved. When a saved template already does the same thing, it is reused, not duplicated (owner confirmed).
- The existing Auto Run limits apply: test environments set up in the app only, saved accounts only, and the must-not-save protection when the task asks for it. Nothing more was asked for (owner confirmed).
- **Undo and remove:** when the person asks in the chat, the assistant deletes what it created itself. It does not hand a Clean up list over for the person to confirm (owner, 2026-10-22, option a).
- **Deleting as part of a task:** when a task needs something deleted, the assistant tells the person what it is about to delete and waits for a yes in the chat before doing it (owner, 2026-10-22).

## Goals

1. **A plain-task command, `/tcm:explore <task>`.** It drives the Auto Run browser as a saved account, does the task, checks the outcome and ends with a summary (in the chat, and as a .md file).
2. **One-off API calls in the same signed-in browser session the assistant is using,** with no fresh sign-in, and without ending that session.
3. **Every API call that succeeds is saved as an API template,** or matched to an existing one, so it can be reused later.
4. **A change list:** every record the assistant created, and every value it changed with its value before. The summary shows it, it outlives the chat, and "undo" or "remove" works through it.
5. **The same tools work when the person simply asks in a chat,** without the command.

## Non-goals

- No test case, no script, no run report, no Past runs entry, and no area mapping is required.
- No new HTTP methods. API calls are GET or POST, as templates are today; nothing in the app under test is changed with PUT, PATCH or DELETE requests. (The app's own deletes go through POST templates, as Clean up does today.)
- A delete call is never saved as an API template. A saved delete template must only ever be proven on a record the app itself made (`PROVE_REFUSAL`), and that rule stays.
- No unattended or scheduled explore runs, and no several browsers at once. Several browsers is the separate several-agents project.
- No change to how Auto Run runs, repairs or writes scripts.

## Anti-goals (failure even if it works)

- **Losing track of what it changed.** A change the change list does not hold cannot be undone. The change list is the safety net, so recording a change is required, not optional.
- **Working outside the test environments set up in the app,** or as anything but a saved account, or showing a password.
- **Undo that guesses.** If a before-value is not known, or a record cannot be deleted by a proven template, the undo says so plainly. It never invents a value.
- **Templates saved half-working,** or a pile of near-duplicate templates.

## How it works

### 1. The session (reuses the discovery browser)
- `start_autorun_discovery` gains `explore: true`. It opens and signs in exactly as a discovery does, and `no_save` works as today. In explore mode:
  - **`area` is optional, and nothing is filed as seen.** Today an area-less read says nothing was recorded; in explore mode it is simply quiet. Explore mode is not mapping.
  - **The Writer role and `RULES_DISCOVERY` are not sent.** The new **explorer** role is sent instead, once per session (`roles.rs`, `roles/explorer.md`, the same mechanism as the other four roles). Its rules:
    - do the task asked, and nothing beyond it;
    - read a value before changing it, and record every change;
    - name created records with the environment's test prefix;
    - check outcomes on the page, in the requests, or with `db_query` when it is offered;
    - never bend a check to fit what the page shows (a mismatch is a finding for the summary);
    - stop and report when a step fails twice in the same way.
  - **Its answers end with a short `RULES_EXPLORE` block** (the 2-3 rules of the moment), like the other modes.
- Actions are `discover_autorun_actions`, unchanged. Reading uses `get_autorun_page`, `probe_autorun_locator`, `search_autorun_requests` and `get_autorun_request`. Ending uses `end_autorun_discovery`.

### 2. One-off API calls: new tool `send_api_request`
- **Arguments:**
  - one step in the template step format: `method` (GET or POST), `path`, `query`, `json` or `form`, `files` (Test file names), `expect`, `capture`;
  - an `effect`: `read`, `create`, `edit` or `delete`. It is required for a POST and defaults to `read` for a GET;
  - for `effect: delete`, `person_said_yes: true` is required. The explorer role says it may be set only after the person answered yes, in this chat, to a message naming what will be deleted. Without it the call is refused with: "Tell the person what will be deleted and wait for their yes in the chat first.";
  - optional naming for its saved template: `id`, `name`, `description`, `params` (which values are parameters).
- **It runs inside the page the session holds,** with the same in-page fetch and anti-forgery handling a template step uses (`api_templates/runner.rs` `FETCH_FN`). The token is read the way a template run reads it; the builder decides whether that is from the current page or from the template's anti-forgery page, in the same tab.
- **It needs an explore session** (or a discovery) of the assistant's own. Without one, it says to start one.
- **Gates:**
  - It is offered only while API Templates is on.
  - A POST needs the same API-writes switch that templates need.
  - A `no_save` session refuses POSTs, with the guard's sentence.
  - It never leaves the recipe's allowed sites.
- **The answer** carries the status, the captured values, the response (redacted as `get_autorun_request` redacts, and cut at the template runner's 64 KB) and the template it was saved as or matched (see 3).
- **It is logged** in the activity log like a template run (method and path, never bodies with secrets).

### 3. Saving calls as templates (owner rule)
- **A call that met its `expect`** (or answered 2xx with no `expect`) is saved as an API template, or matched to an existing one:
  - **A match** is a saved template with one step, the same method, the same path (placeholders aside), and the same body field names. A match is not saved again; the answer names it.
  - **Otherwise the app saves a new one-step template.** It uses the assistant's `id`, `name`, `description` and `params` when given. Otherwise it derives them: the id from method and path, made unique; the name from the path; no params.
  - **It is marked proven by this run.** This run was the same signed-in in-page fetch that `prove_api_template` performs, and the record says it was proven during an explore session, with its date.
- **A call that failed is not saved,** and neither is a delete call.
- **Successful GETs are saved too (owner: "all API calls can be saved").** `Effect` gains `Read`. It needs a serde-compatible addition to the stored type, its colour in the flow page and the API Templates tab, its place in the fixture rules (a read step is allowed in a fixture and creates nothing), and the guide. Matching an existing template (above) is what keeps this from piling up near-copies.
- **Several calls the assistant made in order** can be saved as one multi-step template with the existing `prove_api_template` (that runs in a fresh browser, so after the session ends). The design does not add a new multi-step path.
- **The person's API Templates tab shows these like any other template.** No new screen.

### 4. The change list
- **New tool `record_explore_change`.** The assistant calls it for every change, so the explorer role and the guide say to. Each change is one of:
  - **created:** a record it made, with `kind`, `id` and `name`;
  - **changed:** an existing record's field, with `record`, `field`, `before` and `after`;
  - **deleted:** a record it deleted after the person's yes. It is marked as one that cannot be undone, with what it held where the assistant read it first.
- **`send_api_request`** with `effect: create` and a captured id records a created entry by itself. With `effect: edit` it asks for the before-value. The assistant must record it with `record_explore_change` before or after the call, and the answer reminds it.
- **Storage:** one JSON file per explore session in the Auto Run store (`explore/<started-at>-<account>.json`), never holding a password or cookie. It outlives the chat and the app.
- **Only creates the app saw go onto the test-made list** (`test_made.rs` gains a second writer, kept `pub(crate)` and reached only from this path):
  - a `send_api_request` create whose response carried the captured id; or
  - a browser write in the session's captured requests whose response carries the id the assistant names, which the app checks before accepting.

  A `created` entry the app cannot match goes in the change file as *not verified*. It is shown in the summary, and undo can only remove it in the browser, never through a delete template. This keeps an older, real record from ever becoming deletable by mistake. The person's own Clean up also sees the verified ones, with its usual preview, prefix and age filters.
- **Reconciling:** when the session ends, and in `get_explore_changes`, the app compares the write requests it captured in the session (method and path) against the change list. It lists every write no change accounts for, under "Saves not on the change list", so the summary has to name them. It does not refuse to end the session.
- **New tool `get_explore_changes`:** lists the saved sessions (newest first), or one session's change list.

### 5. Undo and remove (only when the person asks)
- **The `/tcm:explore` command takes `undo <session>`,** or the person asks in a chat. The assistant reads the change list with `get_explore_changes`, then works backwards:
  - **changed fields** are set back to `before` in the browser, or with `send_api_request` where a template does that edit;
  - **created records** are removed with the proven delete template for their kind, through a new bridge route and tool, `undo_explore_creates {session, ids}`. It runs the existing Clean up for exactly those test-made ids, taken from that session's verified creates. No preview, age or prefix filter applies, because the person asked in the chat (owner's option a). It is still delete templates only, and only for ids on that session's verified list. The tripwire in `tests/suite/api_fixtures.rs` and the guide's sentence about Clean up are narrowed to say exactly that: the person's Clean up, or an undo of the assistant's own explore session. A test pins that any other id is refused. Clean up signs in as the delete template's account under its lease, so the route ends the assistant's own explore session first when it holds that account, and says so;
- **Anything it cannot undo is named in the answer:** no before-value, no delete template, or the record already changed again. Each entry it undid is marked undone in the session file.
- **"Remove" (delete what was created, leave the changed values) is the same, with created records only.**

### 6. The summary
- The command's last step writes a .md summary and shows it in the chat. It holds:
  - the task as given;
  - the account and environment;
  - what it did, step by step;
  - what it checked, and each check's result;
  - findings (anything that looked wrong, with the request or page evidence);
  - the change list;
  - the templates saved or reused;
  - how to undo.
- **File location (my call):** `tcm-explore/<date>-<short-task>.md` in the folder Claude was started in, and in the chat.

## Constraints (from the project)

- All Azure DevOps calls stay GET, POST or PATCH, with no DELETE anywhere. This design touches the app under test, not Azure DevOps.
- Secrets live in Rust only. No password, cookie or token goes into an answer, a log, a template or a change file.
- User-facing errors name no URL (`ado/transport.rs` style).
- Gating: Auto Run module for the session and the change tools; API Templates for `send_api_request`; the per-tool AI Bridge switches as usual.
- `src/bindings.ts` is generated. Rust tests live in `src-tauri/tests/suite/`.
- Hidden features are never named.

## Left to the builder

- **The anti-forgery token for `send_api_request`:** read it from the current page or from the template's token page in the same tab, whichever is reliable with what the runner does today.
- **The exact matching rule for "same body field names"** (nested JSON depth).
- **Wording** of the explorer role and the `RULES_EXPLORE` block (within the role caps of 45 lines and 4000 characters).
- **Whether `run_api_template` of a saved template may run inside the assistant's own explore session** instead of being refused while it holds the account. This is desirable, so saved templates can be reused mid-task. The builder judges whether it fits the runner cleanly; if not, it stays refused, and `send_api_request` covers one-step reuse.

## Calls I made (for the owner to check)

0. These four came from the builder check:
   - only creates the app saw can be deleted by undo;
   - unrecorded saves are listed, but do not block ending the session;
   - GETs are saved as a new "read" kind;
   - deletes inside a task need `person_said_yes`, and are never saved as templates.

1. The command is `/tcm:explore`, and the tools also work in an ordinary chat.
2. Explore reuses the discovery browser (`start_autorun_discovery` with `explore: true`) instead of a new browser tool.
3. A call saved from an explore session counts as proven by that run.
4. Created records also go on the test-made list, so the app's Clean up sees them.
5. The summary is saved in a `tcm-explore` folder, as well as shown in the chat.
6. The assistant may check results with the database query tool when Database Access is on.
