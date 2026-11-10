# Assessment items A1.6 to A1.9 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish backlog item A1 (docs/backlog.html): move the webview cache out of localStorage, do the smaller speed items, add arrow keys and skeleton loaders to the case lists, and stop an upload that would silently lose formatting.

**Architecture:** The webview cache keeps its synchronous API (`cacheRead`/`cacheWrite`/`persistentQuery`) over an in-memory map that `initCache()` loads from IndexedDB before the first render. The Rust side gets bounded logs and caches. Lists share one keyboard hook (`useRowKeys`) and one loading component (`ListSkeleton`). `submit_queue` refuses, before any write, when it cannot read the current steps of its updates; the screen offers "Upload anyway".

**Tech Stack:** Tauri 2, Rust (tokio, serde), React 19 + TypeScript, TanStack Query, vitest + jsdom, fake-indexeddb (dev only), tauri-specta bindings.

**Spec:** `docs/backlog.html`, item A1 (A1.6 to A1.9).

## Global Constraints

- Every Rust test is in `src-tauri/tests/suite/`, one module per file, with a `mod` line in `suite/main.rs`.
- `src/bindings.ts` is generated: `cd src-tauri && cargo test --test bindings`. Never hand-edit it.
- A tauri-specta command takes at most 10 arguments (the `AppHandle` counts).
- No colour literals in UI code (`src/ui-consistency.test.ts`). Button icons come from `src/lib/actionIcons.ts`.
- One cache per side: only `src/lib/cache.ts` touches cache storage (`src/lib/cache.test.ts` "one cache").
- User-facing text: short active sentences, no em dashes, no URLs.
- Run one test suite at a time on this machine.
- 22 Rust suite tests fail on Linux with or without these changes (Windows paths, browsers, Credential Manager). They are not regressions.

## Review Focus

1. A restored old backup puts `tcm-v2-cache:*` keys back into localStorage after launch: a read of such a key must still find it (Task 1, test "an earlier version's localStorage entries move over").
2. IndexedDB slow or unavailable at launch: the window must still open within 1.5 s and the cache must work for the session (Task 1, `main.tsx` race; covered by jsdom having no IndexedDB in the screen tests).
3. A write made while the cache is still loading must not be overwritten by the older disk copy (Task 1, test "a write made while the cache loads").
4. Keys typed in a field inside a row (a note, an editor) must not move the row focus (Task 4, test "keys typed in a field inside a row stay with the field").
5. "Upload anyway" must send exactly one more submit, with permission, and a later normal submit must not carry it (Task 3, `allowPlainNext` reset; test "an upload stopped for unreadable steps offers Upload anyway").

---

### Task 1: Webview cache in IndexedDB, and a warning when a queue save fails (A1.6)

**Files:**
- Modify: `src/lib/cache.ts`, `src/main.tsx`, `src/test-setup.ts`, `src/dev/DevPanel.tsx`, `src/hooks/useQueue.ts`, `package.json` (dev dep `fake-indexeddb` 6.2.6, exact)
- Test: `src/lib/cache.test.ts`, `src/hooks/useQueue.test.ts`; update tests that read `tcm-v2-cache:*` from localStorage (`App`, `QueueSection`, `WorkItemDrawer`, `plansAll`, `Suites`, `PrPanel`, `WorkBoard`)

**Interfaces:**
- Produces in `src/lib/cache.ts`: `initCache(): Promise<void>`, `cacheStoredKeys(): string[]`, `clearCache(): void`, test hooks `resetCacheForTests()`, `flushCacheForTests(): Promise<void>`, `cacheWriteAtForTests<T>(key, data, at)`. Database `tcm-v2-cache`, store `entries`, key = cache key without prefix.
- Produces in `src/hooks/useQueue.ts`: `saveDraftQueue(org, pbiId, queue): boolean`.

- [x] **Step 1: Tests** in `cache.test.ts` (with `import "fake-indexeddb/auto"`): "an entry survives a restart", "a removed entry stays removed after a restart", "another account's wipe reaches the database", "an earlier version's localStorage entries move over and leave localStorage", "a write made while the cache loads is not overwritten by the disk", "a write touches no localStorage at all". `useQueue.test.ts`: "a queue that cannot be saved warns once, not on every change".
- [x] **Step 2: Implement** the memory map, batched IndexedDB flush (50 ms), legacy migration at `initCache` plus lazy migration on a miss, caps 150 / 120 plan trees in memory. `main.tsx`: `await Promise.race([initCache(), 1500 ms])` before render. `test-setup.ts` afterEach: `resetCacheForTests()`. DevPanel "Clear app storage" also calls `clearCache()`. Queue save failure: one `toast.warning` per failure run, text "The queue could not be saved on this computer. It stays only until you close the app, so upload it before you close.", `duration: Infinity`.
- [ ] **Step 3: Verify** `npx tsc --noEmit -p .` clean and `npx vitest run` passes (a single flaky App.test failure that passes on re-run is the known flake).
- [ ] **Step 4: Commit** `perf(v2): webview cache in IndexedDB, queue save failures warn (A1.6)`.

### Task 2: Rust smaller items (A1.7)

**Files:**
- Modify: `src-tauri/src/applog.rs`, `src-tauri/src/cache/keys.rs`, `src-tauri/src/cache/mod.rs`, `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/suite/applog.rs`, `src-tauri/tests/suite/cache.rs`

**Interfaces:**
- Produces: `applog::MAX_FILE_BYTES = 25 MiB`, `applog::set_file_cap(bytes: u64)`; `keys::ASSIGNED_SEEN_PREFIX = "assigned-seen"`, `keys::DURABLE_MAX_AGE_MS = 60 days`, `keys::SESSION_MAX_AGE = 24 h`.

- [x] **Step 1: Tests** `applog::a_full_log_file_stops_growing_and_says_why` (cap 400 bytes, 50 lines: one file, under 700 bytes, exactly one "reached its size limit" line, all 50 lines in the memory tail); `cache::entries_past_every_shelf_life_are_removed_but_assignment_baselines_stay`.
- [x] **Step 2: Implement** the log file handle kept open per day (reopen on date change, reset on `init`), the size cap; `evict_stale` on load and on every `put_at` (assignment baselines kept); session values older than 24 h dropped on `session_put`; `SetupSteps` timings appended to the "started" log line (events and database logins, npm logs folder and app log, cache and settings, Auto Run data). Inline images were done in A1.3 (24 MB per answer).
- [ ] **Step 3: Verify** `cd src-tauri && cargo test --test suite -- applog:: cache::` passes.
- [ ] **Step 4: Commit** `perf(v2): bounded log file and caches, startup step timings (A1.7)`.

### Task 3: Stop an upload that would lose formatting (A1.9)

**Files:**
- Modify: `src-tauri/src/commands/queue.rs`, `src/components/QueueSection.tsx`, `src/bindings.ts` (regenerated)
- Create: `src-tauri/tests/suite/submit_plain_steps.rs` (+ `mod` line)
- Test: `src/components/QueueSection.test.tsx`

**Interfaces:**
- Produces: `commands::queue::SubmitOptions { order_hint: Vec<OrderHint>, allow_plain_steps: bool }` as the last `submit_queue` argument (replaces `order_hint`); `commands::queue::PLAIN_STEPS_REFUSAL: &str`; TS `export const PLAIN_STEPS_REFUSAL` in `QueueSection.tsx`, same text: "The current steps of the cases this upload updates could not be read, so nothing was sent. If you upload anyway, their steps are written as plain text and any formatting or pictures in them are lost."

- [x] **Step 1: Tests** Rust `the_plain_steps_sentence_is_word_for_word_the_one_the_screen_knows`; TS "an upload stopped for unreadable steps offers Upload anyway, which sends with permission" (first call `allow_plain_steps: false`, toast action "Upload anyway", second call `true`) and "the plain-steps sentence is word for word the one Rust answers".
- [x] **Step 2: Implement** in `submit_queue`: with update rows and `!allow_plain_steps`, a failed token or a failed baseline read returns `Err(PLAIN_STEPS_REFUSAL)` before any write. Screen: `onError` on that exact message shows `toast.warning` with action "Upload anyway", which sets a one-shot ref and calls `submit.mutate` with the same `preExisting`.
- [ ] **Step 3: Verify** `cargo test --test bindings`, `cargo test --test suite -- submit_plain_steps::`, `npx vitest run src/components/QueueSection.test.tsx`.
- [ ] **Step 4: Commit** `feat(v2): an upload that cannot read its baseline stops and asks (A1.9)`.

### Task 4: Arrow keys and skeleton loaders (A1.8)

**Files:**
- Create: `src/lib/useRowKeys.ts`, `src/components/ui/list-skeleton.tsx`
- Modify: `src/screens/ViewCases/index.tsx`, `src/screens/ExistingCases/index.tsx`, `src/screens/RunPanel/index.tsx`, `src/screens/RunPanel/RunRows.tsx`, `src/screens/Suites.tsx`, `src/screens/ManageCases/index.tsx`, `src/screens/AutoRun/index.tsx`
- Test: `src/lib/useRowKeys.test.tsx`, plus one keyboard test each in `ViewCases.test.tsx` and `RunPanel.test.tsx`

**Interfaces:**
- Produces: `useRowKeys<K>({ order: readonly K[]; onSelect: (key: K, keys: RowSelectKeys) => void }) => { listProps: { "data-row-list": ""; onKeyDown }, tabKey: K | null, onRowFocus: (key: K) => void }`. Each row: `data-row-key={key}`, `tabIndex={key === tabKey ? 0 : -1}`, `onFocus` → `onRowFocus(key)` when the row itself took focus. Space/Enter select as Ctrl-click; Shift+arrow selects as Shift-click; Home/End go to the ends.
- Produces: `ListSkeleton({ label?, rows = 5, rowClassName = "h-8", className?, style? })`: `role="status"` with the label in an `sr-only` span, rows `aria-hidden`.
- Navigation order is the rows on screen: rows in a folded group are left out unless held open.

- [ ] **Step 1: Write failing tests** in `useRowKeys.test.tsx`: "Down and Up move focus to the next and previous row", "Shift+Down selects as a Shift-click would", "Space selects the focused row as a Ctrl-click would", "only one row is in the Tab order", "keys typed in a field inside a row stay with the field". `ViewCases.test.tsx`: "arrow keys move through the cases and Space selects one". `RunPanel.test.tsx`: "arrow keys move through the rows and Space selects one".
- [ ] **Step 2: Run** `npx vitest run src/lib/useRowKeys.test.tsx` → fails for the missing behaviour or wiring.
- [ ] **Step 3: Implement** the wiring: View and Update pass the screen's `handleCardClick` through `rowLatest`; Run passes `handleRowClick` through `rowClickRef`. Memo rows take `tabbable: boolean` and the stable `onRowFocus`. Skeletons replace "Loading test points" (Run Tests, 6 rows; Search Suites folder, 3 rows, indented) and "Loading test cases…" (Auto Run, 5 rows); Search Suites and Suite Management show a 6-row decorative skeleton (no label) under the existing first-scan progress bar.
- [ ] **Step 4: Verify** the new tests pass, then `npx vitest run src/ui-consistency.test.ts src/a11y.test.tsx src/screens` and `npx tsc --noEmit -p .`.
- [ ] **Step 5: Commit** `feat(v2): arrow keys in the case lists, skeleton loaders (A1.8)`.

### Task 5: Close out

- [ ] Mark A1 built in `docs/backlog.html` (what shipped, the hand checks owed: big suite scroll in the real app, IndexedDB migration on an installed app, Upload anyway against a real org).
- [ ] Full gate: `npx vitest run`, `npx tsc --noEmit -p .`, `npm run build`, `cd src-tauri && cargo test --tests`.
- [ ] Commit and push to `claude/software-assessment-optimization-e3zpfi`.
