# UI polish + database credentials - design

Date: 2026-09-26. Owner-approved in conversation (items 1-11 of the owner's
list; credentials "per database" with Windows Credential Manager; action
buttons on the Import File pattern).

## Goal

Eleven owner requests: a calmer, more consistent UI (animations, sticky
action buttons in one place, less empty furniture) and a Company database
card that is just "which database, manage its login, may it write" - with
the login kept out of the webview.

## Global constraints

- No HTTP DELETE to Azure DevOps outside `ado/deletion.rs`.
- Rust tests live only in `src-tauri/tests/` (never `#[cfg(test)]` in `src/`).
- `src/bindings.ts` is generated (`cargo test --test bindings`); never hand-edit.
- Never weaken `src/ui-consistency.test.ts` or the a11y tests; colours come
  from theme tokens only.
- User-facing errors name no URL (`ado/transport.rs` sentences).
- Motion respects `prefers-reduced-motion` (index.css already switches the
  `t-*` animations off; new motion must join that).
- Nothing names Auto Run, the unlock, or other hidden features in visible
  copy, the changelog, commit messages or docs.
- Commits via Bash heredoc, trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- One build/test command at a time (shared machine). Cargo runs in SDD use
  `CARGO_TARGET_DIR=target/gate`.

---

## A. Company database: credentials per database (item 3)

### What the person sees

The AI Tools screen's Company database card (`src/screens/AiBridge.tsx`,
`data-tour="ai-db"`) shows only:

1. **Database** dropdown - the shipped databases (today: Dev read only, Dev
   dev login, QA read only) plus **Your own database**.
2. A one-line status under it: `Signs in as <user>` or `No login saved`.
3. **Manage credentials** button - opens the credentials modal for the
   selected database.
4. **Create, update and delete** switch - unchanged rule: enabled only when
   the selected database's user is the dev login (`isDevLoginConnection`
   rule, applied to the user name); shown off otherwise.
5. The PHR X registration block keeps its existing `showPhrx` gating and
   behaviour, but registers the selected database (by id, see below).
6. **Forget them** - also wipes every stored credential (below).

The raw connection-string fields, the "Edit as one string" toggle and the
host/port/database/user/password inputs leave the card.

### Credentials modal

Titled `Credentials for <label>` (accessible name set). Fields:

- Shipped database: Server and Database shown as read-only text; **User**
  and **Password** editable.
- Your own database: **Server**, **Port** (optional), **Database**, **User**,
  **Password**, **Trust the server certificate**.
- Password is never pre-filled. When one is saved its placeholder reads
  `Saved - leave blank to keep it`.

Buttons: **Test connection** (left), then **Reset to default** (shipped
database with a saved override only), **Cancel**, **Save** (right).

- **Test connection** tests what the form holds now (a blank password means
  the saved one). It runs `SELECT 1` through the existing sqlcmd path
  (`db::sqlcmd::run_sql` with `RealRunner`, guard included). Result shown
  inline under the fields: success `Connected to <database> on <server> as
  <user>.` in `text-success`; failure = the server's own reason (sqlcmd's
  message, password scrubbed via the existing `hide_password`) in
  `text-danger`. sqlcmd missing -> the existing "sqlcmd not found" sentence.
  While running the button reads `Testing`.
- **Save** stores and closes; a toast `Saved the login for <label>.`
- **Reset to default** removes the override so the shipped login applies.

### Where the secret lives

- Rust owns it. Each database id has at most one entry in **Windows
  Credential Manager** (generic credential, target `tcm-v2/db/<id>`, local
  machine persistence). The blob is the full connection string for that
  database (UTF-8). Behind a `SecretStore` trait with a Credential Manager
  implementation for the app and an in-memory one for tests; tests never
  touch the real store. Use the `windows-sys` crate (already in Cargo.lock)
  as a direct Windows-only dependency with the credentials feature, rather
  than adding a new crate.
- Shipped databases gain a stable `id` in `db_defaults.rs`
  (`dev-read`, `dev-login`, `qa-read`); Your own database is `own`.
  Resolution for an id: stored override if present, else the shipped
  string (shipped ids), else none (`own`).
- The webview never receives a password or a full connection string again.
  It gets a public view per database:
  `DbDatabase { id, label, shipped: bool, server, port: Option<u16>,
  database, user, trust_cert, has_password, customised }`.
- The webview keeps only non-secret choices in localStorage: the selected id
  (`tcm-v2-db-selected`), and the existing `tcm-v2-db-mcp` blob minus
  `connection_string` (exe path, db type, schema filter).

### Commands (replacing the connection-string plumbing)

- `db_databases() -> Vec<DbDatabase>` (replaces `db_server_presets`; the
  shipped connection strings stop crossing IPC).
- `save_db_credentials(id, form: DbCredentialsForm) -> Result<DbDatabase, String>`
  where `DbCredentialsForm { server, port, database, user, password:
  Option<String>, trust_cert }` (password `None` = keep the saved one;
  server/port/database/trust ignored for shipped ids).
- `test_db_connection(id, form: Option<DbCredentialsForm>) -> Result<String, String>`.
- `reset_db_credentials(id) -> Result<DbDatabase, String>` (shipped ids only).
- `forget_db_credentials() -> Result<(), String>` (every id).
- `import_legacy_db_connection(connection_string) -> Result<String, String>`:
  one-time migration, returns the id to select - an exact match to a shipped
  string selects that id and stores nothing; anything else is stored as `own`.
- `set_bridge_context(..., db_id: Option<String>, db_writes)` replaces
  `db_connection_string`; the bridge resolves the string from the store at
  the moment a database tool runs, so a saved login applies to the next call.
- `register_db_server` and `db_server_defaults`: `DbServerConfig` loses
  `connection_string` and gains `db_id`; Rust resolves it.

`DbCredentialsForm` carries a password INTO Rust only; no exported output
type may carry a `password` field - a test in `tests/` serialises
`DbDatabase` and asserts no key `password` and no stored secret substring.

### Migration

On app start (App-level, before the first bridge push), if
`tcm-v2-db-mcp` still holds a non-empty `connection_string`: call
`import_legacy_db_connection`, store the returned id as selected, and
rewrite the blob without `connection_string`. On failure leave the blob
untouched and log; the next start retries.

### Errors

Credential Manager failures surface as `Could not save the login in
Windows Credential Manager.` / `Could not read the saved login.` (raw error
to `applog`). No URLs are involved.

---

## B. One action-button standard (items 5 and 11)

Rule: **actions on the thing live bottom-right; view controls
(Collapse all) live bottom-left.**

- New shared component `src/components/ActionDock.tsx`, extracted from
  Import File's pattern (`QueueSection.tsx` ~1496 `useOnScreen("0px 0px
  -24px 0px")` + the `fixed bottom-6 right-6` portalled copy with
  `data-sticky-action`). It renders its children in place, right-aligned,
  and - once that in-place row is off screen - a floating copy bottom-right,
  portalled to `<body>` (AnimatedContent's GSAP transform breaks `fixed`
  otherwise), fading/sliding as today. Floating buttons get `tabIndex={-1}`
  (the in-place ones stay the keyboard path), as today. Optional `stackKey`
  so several docks on one screen stack upward instead of overlapping (the
  Suite Management `useFloatRank` behaviour).
- **Import File** (`QueueSection`): moves onto ActionDock, no visible change.
- **Suite Management** (`screens/ManageCases/SuiteCases.tsx`): the order
  actions (Apply order, Reset, Apply order from files) move to the right
  end of the toolbar and dock bottom-right with the suite name label as
  today; view controls (Group by title, A-Z groups, Expand/Collapse groups)
  stay left in the toolbar.
- **Update Test Cases** (`screens/ExistingCases/index.tsx`): the selection
  bar's actions (Bulk edit, Rename, Export JSON, Delete, Move to PBI,
  Clear) dock bottom-right while a selection exists and the bar is
  scrolled away; the `N selected` count travels with them.
- The bottom-left **Collapse all** pill stays as is.
- `src/ui-consistency.test.ts` gains a rule: no `fixed` element positioned
  `right-*`/`bottom-*` outside `ActionDock` (bottom-left pills and toasts
  keep their existing allowances).

---

## C. Smaller items

1. **Logs animation** (`src/screens/Settings.tsx` ~436-533): switching
   between Changelog and Logs plays an entrance on the newly shown panel -
   fade + 4px rise over the app's standard duration and `--motion-ease-*`
   curve, keyed on the panel so each switch replays it; none under reduced
   motion.
2. **Backup text** (`Settings.tsx` ~541): drop "Auto Run scripts" from the
   description list. Backup contents unchanged.
3. **Rename**: `How it works` -> `AI Tools Breakdown` (`AiBridge.tsx`
   ~1018, its test, and the `mcpTools.ts` comment).
4. **Smooth 120-case group opening** (`components/ui/collapse.tsx`,
   `.cv-row` in `index.css`). Symptom: the group grows, slows by row 40-50,
   then the rest appears at once. Leading hypothesis to confirm first:
   `.cv-row` rows use `content-visibility: auto`; while the fold clips its
   content the browser skips the rows below the clip edge, renders each as
   it is revealed (per-frame work that stalls the grow), and then renders
   everything left when the clip comes off. Contributing: the grow runs only
   to the visible span under a strong ease-out, so the last half crawls.
   Fix direction: during the grow, rows inside an entering panel render
   normally (e.g. `.t-collapse.is-entering .cv-row { content-visibility:
   visible }`) and the grow uses an even curve for tall spans; the result
   must look continuous from first row to last. Reduced motion unchanged.
   Verified by hand in the running app (jsdom does no layout).
5. **Recent JSON Imports** (`QueueSection.tsx` ~1560): with no recent
   imports the whole section renders nothing.
6. **Share box** (`screens/ImportFile.tsx` ~758-781): one row - the input
   (placeholder `Paste a share link from a teammate`) and **Import shared**;
   the one-time-use explanation moves to the input's `title`. The top
   border/padding stays so it still reads as its own part.
7. **Pull Requests picker** (`screens/PrPanel.tsx` ~902, `MultiSelect`):
   options are ordered checked-first - "Your Pull Requests" first if
   checked, then checked repos in repo-list order, then unchecked in
   repo-list order. Order is computed when the dropdown OPENS (not live
   while clicking, so an option does not jump from under the pointer).
8. **Images in comments** (item 10): a new Rust command
   `comment_images(org, sources: Vec<String>) -> Vec<InlineImage>` reuses
   the work item detail downloader (`collect_attachment_images` +
   `attachment_download_url` token-host guard), extended to find markdown
   image URLs (`![alt](url)`) as well as `src="..."`. Work item comments
   (`CommentsPanel.tsx`) and PR threads (`PrThreads.tsx`) swap URLs for the
   returned data URLs before rendering, like `WorkItemDrawer`'s
   `withInlineImages`. The images are held in memory for the open view only
   (react-query, not the disk cache - data URLs are large). An image that
   cannot be fetched renders as a small `Image unavailable` placeholder in
   `text-faint`, not a broken icon.

## Testing

- Rust (`src-tauri/tests/`): secret store round-trip with the memory store;
  resolution order (override > shipped > none); `DbDatabase` never carries a
  password; legacy import (shipped match vs own); bridge context resolves by
  id; `test_db_connection` with a fake Runner (success sentence, failure
  scrubbed of the password); comment image URL extraction for markdown and
  HTML, and the token-host guard refusing a foreign host.
- Vitest: credentials modal (blank password keeps, test result states,
  reset only when customised, accessible name); card shows only the three
  controls; migration runs once; ActionDock in-place vs floating; Suite
  Management actions on the right; Update Test Cases selection docking;
  Logs panel animation class and reduced motion; empty Recent imports
  renders nothing; share row; PR picker order on open; comment image swap
  and the unavailable placeholder; rename.
- Hand checks (owner): real Credential Manager save/test/forget; Test
  connection against Dev; 120-case group opening; floating buttons on all
  three screens; a real comment image.

## Out of scope

Changing what the backup contains; moving any other secret; redesigning
the PHR X block; the 26 unnamed Modal call sites.
