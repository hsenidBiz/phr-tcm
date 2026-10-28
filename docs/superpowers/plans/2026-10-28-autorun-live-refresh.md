# Auto Run live refresh Implementation Plan

**Goal:** What an assistant adds through the AI bridge (areas, components, the discovery map and its summary, quirks, test files, proposed accounts, fixtures) shows on the Auto Run screen at once, without a reload.

**Architecture:** The same pattern scripts already use (`autorun/store.rs` `on_scripts_changed` → `AutorunScriptsChanged` → `src/hooks/useAutoRunScriptsSync.ts`): each store's single write choke point tells process-wide listeners after a successful write; lib.rs turns that into one Tauri event; one webview hook mounted once in App invalidates the matching query keys. Fixtures reuse the existing `ApiTemplatesChanged` event. No polling.

**Why it is stale today:** refetch-on-focus is off (`src/main.tsx`), and the Setup panel's queries (`SetupPanel.tsx` `useAutoRunSetup`) are only read on mount; switching tabs remounts (`App.tsx` `key={section}`), which is the "reload" the owner sees.

## Global constraints
- Rust tests only in `src-tauri/tests/suite/<module>.rs` with a `mod` line in `suite/main.rs`. Run one module: `cd src-tauri && cargo test --test suite autorun_data_changed::`. Never set CARGO_TARGET_DIR. One build/test at a time.
- `src/bindings.ts` is generated: `cd src-tauri && cargo test --test bindings`. Never hand-edit.
- No DELETE to Azure DevOps (not touched here). No paths/URLs in user text (none added).
- Files keep their existing line endings (most are CRLF): edit with the Edit tool, never sed/heredoc rewrites.
- Commits via bash heredoc, ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never `git add -f`, never touch git stash.

## Task 1: Rust — the event and the notifications

**Files:** `src-tauri/src/events.rs`; new `src-tauri/src/autorun/data_changed.rs` (+ `pub mod data_changed;` in `autorun/mod.rs`); `src-tauri/src/lib.rs`; the stores below; new test `src-tauri/tests/suite/autorun_data_changed.rs` (+ `mod autorun_data_changed;` in `tests/suite/main.rs`); regenerated `src/bindings.ts`.

**Produces:**
- `events.rs` (follow the `ScanKind` pattern):
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
  #[serde(rename_all = "camelCase")]
  pub enum AutorunDataKind { Areas, Components, Map, MappingSummary, Quirks, TestFiles, Proposals }
  #[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type, tauri_specta::Event)]
  pub struct AutorunDataChanged { pub kinds: Vec<AutorunDataKind> }
  ```
- `autorun/data_changed.rs`: copy `store.rs`'s listener registry (~lines 46-80): `pub fn on_data_changed(f: impl Fn(&Path, &[AutorunDataKind]) + Send + Sync + 'static)` and `pub fn notify(at: &Path, kind: AutorunDataKind)`; copy listeners out of the lock before calling them. `at` is the store root (the test-files folder for test files) so tests can filter by tempdir.
- Call `notify` once after each successful write (never on a refused or no-op write):
  - `autorun/nav.rs` `save_nav` → `Areas`
  - `autorun/components.rs` `update_with` and `reset_components` → `Components`
  - `autorun/discovery_map.rs` `update` (only on the path that actually writes) and `reset_map` → `Map`
  - `autorun/mapping_summary.rs` `save_summary` → `MappingSummary`
  - `autorun/quirks.rs` `save_quirks` → `Quirks`
  - `environments.rs` `save_proposals` → `Proposals`
  - `test_files.rs` `commit` (on Ok) and `remove` → `TestFiles`
- `lib.rs`: add `events::AutorunDataChanged,` to `collect_events![]`; in `setup`, beside the scripts listener, register `autorun::data_changed::on_data_changed` emitting `events::AutorunDataChanged { kinds: kinds.to_vec() }`.

**Tests** (model on `tests/suite/autorun_scripts_changed.rs`: process-wide listener, filter by tempdir): one per store — nav put hears `[Areas]`; components remove/reset hear `[Components]`; a discovery-map write hears `[Map]` and a no-op update hears nothing; `reset_map` hears `[Map]`; a quirk record hears `[Quirks]`; `save_summary` hears `[MappingSummary]`; `save_proposals` hears `[Proposals]`; a test-file save and remove hear `[TestFiles]`, a refused save hears nothing; a source check that lib.rs registers both `events::AutorunDataChanged,` and `autorun::data_changed::on_data_changed(`.

Then regenerate bindings; event becomes `events.autorunDataChanged`, kinds `"areas" | "components" | "map" | "mappingSummary" | "quirks" | "testFiles" | "proposals"`.

Verify: `cargo test --test suite autorun_data_changed::` passes; `cargo test --test bindings` passes. Commit.

## Task 2: Webview — one hook

**Files:** new `src/hooks/useAutoRunDataSync.ts`, mounted once in `src/App.tsx` beside `useAutoRunScriptsSync()`; new `src/hooks/useAutoRunDataSync.test.tsx`; update the stale comment in `src/screens/AutoRun/AccountsDialog.tsx` (~line 60, "nothing tells this screen…").

**Produces:** `export const DATA_KIND_KEYS: Record<AutorunDataKind, readonly unknown[]>`:
- `areas` → `["autorun-nav"]`, `components` → `["autorun-components"]`, `map` → `["autorun-map"]`, `mappingSummary` → `["autorun-mapping-summary"]`, `quirks` → `["autorun-quirks"]`, `testFiles` → `["test-files"]`, `proposals` → `envKeys.proposals` (verify its exact value).
- Listens to `events.autorunDataChanged` and `events.apiTemplatesChanged` (the latter → `[FIXTURES_KEY]` from `screens/ApiTemplates/FixturesTab.tsx`). Collects kinds in a Set and flushes with a 300 ms trailing `setTimeout` (cleared on unmount) — map writes come once per page read. Invalidates by prefix (any org/project).

**Tests** (model on `src/hooks/useAutoRunScriptsSync.test.tsx`, `mockIPC(..., { shouldMockEvents: true })`): `{kinds:["areas"]}` reloads an `["autorun-nav","o","p"]` query and not an `["autorun-components","o","p"]` one; `api-templates-changed` reloads `["api-fixtures","o","p"]`; several events inside the window cause one reload; every kind has a `DATA_KIND_KEYS` entry.

Verify: `npx tsc --noEmit` clean; `npx vitest run src/hooks src/App.test.tsx src/ui-consistency.test.ts` passes. Commit.

## Review focus
- A write that fails or is refused must not announce a change.
- The discovery map writes on every page read: the debounce must keep that to one reload per burst.
- A listener panicking or the app not yet set up must never break a store write.
- Prefix invalidation must not reach unrelated keys (e.g. `["test-files"]` is shared with API Templates — that is intended).
