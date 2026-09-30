# How To Use on Demand Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the app without the 31 MB How To Use site; publish the site beside each release and let Settings download it on demand, keep it across updates, and offer Update Guide when it changed.

**Architecture:** A new `src-tauri/src/guide.rs` owns the guide on disk (fingerprint, status, safe unpack and install, adoption of an old copy) behind a `GuideSource` trait for the two downloads; commands expose status, download (with progress events) and open. `help.rs` stops embedding the site. The release script fingerprints, zips and uploads the site; Settings shows Download / How To Use / Update Guide.

**Tech Stack:** Rust (tauri 2, tauri-specta, reqwest 0.13, sha2 0.11, zip 8.6 - already in Cargo.lock), React 19 + TypeScript, vitest, PowerShell 5.1 release script, Node (a small fingerprint script).

**Spec:** `docs/superpowers/specs/2026-09-30-guide-on-demand-design.md`

## Global Constraints

- Download addresses: exactly `https://github.com/hsenidBiz/phr-tcm/releases/download/v<app version>/how-to-use.json` and `.../how-to-use.zip` (`<app version>` = `env!("CARGO_PKG_VERSION")`, betas included). Nothing else is fetched.
- The json fetch: at most once per app run, 10 s timeout. The zip: refused above 200 MB (by `size` in the json and by bytes received).
- `how-to-use.json` = `{ "fingerprint": "<64 lowercase hex>", "sha256": "<64 lowercase hex>", "size": <bytes> }`.
- Fingerprint rule (both implementations, pinned by tests to the same value): list every file under the site root; path relative to the root with `/` separators; sort by that path (byte order); for each, the text `<path>\n<sha256 hex of its bytes>\n`; the fingerprint is the SHA-256 hex of the concatenation.
- On disk, under the app local data dir: `help/<fingerprint>/` (index.html at its root) and `help/installed.json` = `{ "fingerprint": "<hex>" | null, "at": "<applog stamp>" }`. A folder being filled is `help/.incoming-<n>/`.
- User-facing sentences, verbatim, naming no URL or path:
  - `Download How to Use from Settings first.`
  - `Could not download How to Use. Check your connection and try again - Settings, Logs has the details.`
  - `The downloaded guide was damaged, so it was not kept. Try again.`
  - `How to Use is not available for this version.`
- Raw errors go to `applog` only. Never `reqwest::Client::new()` (tests/suite/ado_network.rs scans for it) - build with `reqwest::Client::builder()` and deadlines.
- Development builds (`cfg!(debug_assertions)`): status is always `Ready`; open uses `concat!(env!("CARGO_MANIFEST_DIR"), "/help/index.html")`; nothing is downloaded.
- Rust tests only in `src-tauri/tests/suite/` (new file gets a `mod` line in `suite/main.rs`). `src/bindings.ts` regenerated with `cd src-tauri && cargo test --test bindings`, never hand-edited.
- No hardcoded colours; icons from `src/lib/actionIcons.ts`; `src/ui-consistency.test.ts` unchanged.
- One build/test command at a time. Commits via Bash heredoc ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never push, never release.

## Review Focus

1. **A download interrupted halfway** (network drops at 60%): the guide on disk (if any) still opens, and no `.incoming-*` folder or temp file is left behind. Pinned in Task 2 (install cleanup) and Task 3 (download error path).
2. **A zip whose entries use backslashes** (Windows PowerShell's `ZipFile` on some versions): unpacks correctly into subfolders, and `..\x` is still refused. Pinned in Task 2.
3. **The app run offline from the start**: Settings shows How To Use (or Download, if nothing is on disk) with no error toast and no Update Guide. Pinned in Tasks 1 and 5.
4. **Two clicks on Download in quick succession**: one download; the button is disabled while it runs, and Rust refuses a second concurrent download with the download-failed sentence. Pinned in Tasks 3 and 5.
5. **Release run with an unchanged guide**: the fingerprint equals the previous release's, so an installed guide shows no Update Guide. Pinned by the fingerprint tests in Tasks 1 and 4 (same rule, same fixed value).

---

### Task 1: Fingerprint, installed record, status and adoption

**Files:**
- Create: `src-tauri/src/guide.rs`; Modify: `src-tauri/src/lib.rs` (`pub mod guide;`)
- Create: `src-tauri/tests/suite/guide.rs`; Modify: `src-tauri/tests/suite/main.rs` (`mod guide;`)

**Interfaces (produces, all in `crate::guide`):**
- `pub fn fingerprint(root: &Path) -> std::io::Result<String>` - the Global Constraints rule.
- `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct Installed { pub fingerprint: Option<String>, pub folder: String, pub at: String }` - `folder` is the folder's name under `help/` (the fingerprint for a downloaded guide; the old version number for an adopted one). This adds `folder` to the spec's on-disk record.
- `#[derive(Deserialize, Clone, PartialEq)] pub struct Published { pub fingerprint: String, pub sha256: String, pub size: u64 }`
- `#[derive(Serialize, specta::Type, Clone, Copy, PartialEq, Debug)] pub enum GuideState { NotDownloaded, Ready, UpdateAvailable }`
- `#[derive(Serialize, specta::Type)] pub struct GuideStatus { pub state: GuideState, pub size: Option<u32> }` - bytes; `u32` because specta refuses `u64` and the cap is 200 MB.
- `pub fn read_installed(help_root: &Path) -> Option<(Installed, PathBuf)>` - the record and its folder, only when the folder has `index.html`.
- `pub fn adopt_legacy(help_root: &Path) -> Option<Installed>` - when `installed.json` is missing, the newest `help/<x.y.z[-beta.n]>/` that has `index.html` (a beta sorts below its release; numeric parts compare as numbers - reuse an existing version compare in the crate if there is one) is recorded as `Installed { fingerprint: None, folder: <that name>, at: now }` and written to `installed.json`.
- `pub fn state_for(installed: Option<&Installed>, published: Option<&Published>) -> GuideState` - None installed -> NotDownloaded; published None -> Ready; fingerprint equal -> Ready; else (different or null) -> UpdateAvailable.

- [ ] **Step 1: Failing tests** in `suite/guide.rs`: `the_fingerprint_follows_the_rule` (temp folder with `index.html` = `"<html>"`, `img/a.jpg` = bytes `[1,2,3]`: assert equals the hex computed in the test by writing out the rule by hand with `sha2`; and a second assert against the literal pinned hex printed once and pasted - the same literal Task 4's JS test uses); `the_fingerprint_ignores_order_and_times` (create files in reverse order, touch mtimes: same value); `state_follows_what_is_on_disk_and_published` (the five cases from spec §9: none -> NotDownloaded; equal -> Ready; different -> UpdateAvailable; published None -> Ready with and without a guide; null installed + published -> UpdateAvailable; null installed + None -> Ready); `an_old_guide_is_adopted_newest_first` (`help/2.0.4/index.html`, `help/2.0.5-beta.2/index.html`, `help/2.0.5-beta.10/` without index: adopts `2.0.5-beta.2`, writes `installed.json` with `fingerprint: null`, `folder: "2.0.5-beta.2"`); `nothing_is_adopted_when_a_record_exists`; `read_installed_needs_the_folder_and_its_index`.
- [ ] **Step 2:** `cd src-tauri && cargo test --test suite guide::` - expect compile failure (module missing).
- [ ] **Step 3:** Implement `guide.rs` with the interfaces above. Serde on `Installed` writes `fingerprint`, `folder`, `at`.
- [ ] **Step 4:** Run the same - PASS.
- [ ] **Step 5:** Commit - "feat(v2): the guide on disk - its fingerprint, its record, and whether it is current"

### Task 2: Safe unpack and install

**Files:** Modify `src-tauri/src/guide.rs`, `src-tauri/Cargo.toml` (`zip = { version = "8.6", default-features = false, features = ["deflate"] }` - match the version in Cargo.lock; confirm the feature name compiles), `suite/guide.rs`.

**Interfaces:**
- `pub const MAX_ZIP_BYTES: u64 = 200 * 1024 * 1024;`
- `pub const DAMAGED: &str` / `NOT_DOWNLOADED: &str` / `DOWNLOAD_FAILED: &str` / `NOT_PUBLISHED: &str` - the four sentences, verbatim.
- `pub fn unpack(zip_path: &Path, into: &Path) -> Result<(), String>` - every entry name split on both `/` and `\`; refuse (before writing anything - scan all names first) an empty name, an absolute path, a drive letter (`C:`), any `..` component; directories created; files written. `Err` carries the reason for the log.
- `pub fn install(help_root: &Path, zip_path: &Path, published: &Published) -> Result<PathBuf, String>` - size check, sha256 check, unpack into `help/.incoming-<n>` (n from a process-wide counter + pid), require `index.html`, rename to `help/<fingerprint>/` (if that folder exists, remove it first), write `installed.json` (`fingerprint: Some`, `folder: fingerprint`), remove every other entry under `help/` except `installed.json` and the new folder; returns the new `index.html`. Any failure removes the incoming folder and returns `Err(DAMAGED.into())` for size/checksum/unpack/index problems (raw reason logged).

- [ ] **Step 1: Failing tests** (build zips in the test with the `zip` crate's writer): `a_good_zip_is_installed_and_the_old_guide_removed`; `a_wrong_checksum_keeps_nothing_and_the_old_guide_stays` (old guide folder + record untouched, no `.incoming-*` left - Review Focus 1); `a_wrong_size_is_refused`; `entries_escaping_the_folder_are_refused` (`../x`, `..\x`, `/abs`, `C:\x`, `a/../../x`: each Err, nothing written outside); `backslash_entries_unpack_into_folders` (`img\light\a.jpg` lands at `img/light/a.jpg` - Review Focus 2); `a_zip_without_an_index_is_refused`.
- [ ] **Step 2:** run `cargo test --test suite guide::` - FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5:** Commit - "feat(v2): a downloaded guide is checked and unpacked safely before it replaces the old one"

### Task 3: Fetching, commands and opening; the site leaves the exe

**Files:** Modify `src-tauri/src/guide.rs`, `src-tauri/src/help.rs` (drop `include_dir!` and the embedding/write code; keep `OPEN_ERROR` if still used, otherwise remove), `src-tauri/Cargo.toml` (drop `include_dir` if nothing else uses it), `src-tauri/src/commands/misc.rs` (`open_help`), a new `src-tauri/src/commands/guide.rs` (+ `commands/mod.rs`), `src-tauri/src/events.rs` (`GuideProgress`), `src-tauri/src/lib.rs` (register commands + event), `src/bindings.ts` (regenerated); rewrite `src-tauri/tests/suite/help.rs` (its embedding tests go; keep any that still apply) and extend `suite/guide.rs`.

**Interfaces:**
- `pub fn guide_url(version: &str, file: &str) -> String` -> `https://github.com/hsenidBiz/phr-tcm/releases/download/v{version}/{file}`.
- `pub trait GuideSource { fn published(&self) -> impl Future<Output = Result<Option<Published>, String>>; fn download(&self, to: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> impl Future<Output = Result<(), String>>; }` - `Ok(None)` = 404 (not published for this version).
- `pub struct GithubGuide { pub version: String }` implementing it with a client from `reqwest::Client::builder().connect_timeout(10 s)` (json: `.timeout(10 s)`; zip: no total timeout, but a 60 s per-chunk read via `tokio::time::timeout` around each `chunk()`), streaming the zip to the temp file and stopping past `MAX_ZIP_BYTES`.
- `pub async fn status<S: GuideSource>(help_root: &Path, source: &S) -> GuideStatus` - adopt legacy first, then `state_for(read_installed, published)`. The published json is fetched once per app run and kept in a process-wide `std::sync::Mutex<Option<Option<Published>>>`; an error is logged and kept as `None`, so offline never nags (Review Focus 3). `size` from the published json. `pub fn forget_published()` clears it - for tests (they take a lock from `suite/serial.rs`) and after a successful download.
- `pub async fn download<S: GuideSource>(help_root: &Path, source: &S, on_progress: impl FnMut(u64,u64)) -> Result<PathBuf, String>` - one at a time (a static `AtomicBool` claim; a second call gets `DOWNLOAD_FAILED` - Review Focus 4); `Ok(None)` published -> `NOT_PUBLISHED`; transport error -> `DOWNLOAD_FAILED` (raw logged); temp file in `std::env::temp_dir()` removed on every path; then `install`.
- Commands (tauri + specta, ungated): `guide_status(app) -> GuideStatus`; `guide_download(app) -> Result<(), String>` (emits `GuideProgress { received: u32, total: u32 }` as bytes; opens the guide after installing, like the old button did); `open_help(app)` now opens `help/<installed folder>/index.html`, or `Err(NOT_DOWNLOADED)` when nothing is installed; in a debug build it opens the repository copy (Global Constraints). The help root is `app_local_data_dir()/help`, as today.

- [ ] **Step 1: Failing tests** with a `FakeGuide` (answers a scripted `published` and writes a scripted zip, calling `on_progress`): `the_status_asks_the_release_once`; `offline_is_ready_never_update` (published Err -> Ready with a guide, NotDownloaded without; no error surfaced); `a_download_installs_and_reports_progress`; `a_failed_download_leaves_the_old_guide_and_no_temp_file` (Review Focus 1); `not_published_says_so` (Ok(None) -> NOT_PUBLISHED); `a_second_download_while_one_runs_is_refused` (Review Focus 4 - hold the first with a oneshot); `the_urls_point_only_at_the_release` (`guide_url("2.0.6-beta.1","how-to-use.json")` exact string). In `suite/help.rs`: remove tests of the embedding; keep the file only if something still applies (else delete it and its `mod` line).
- [ ] **Step 2:** `cargo test --test suite guide::` - FAIL.
- [ ] **Step 3:** Implement; drop the embedding; regenerate bindings (`cargo test --test bindings`).
- [ ] **Step 4:** `cargo test --test suite guide::`, then `cargo test --tests` (full, once) - PASS; `npx tsc --noEmit` clean (the existing Settings button still calls `commands.openHelp()`).
- [ ] **Step 5:** Commit - "feat(v2): the guide is downloaded from the app's own release instead of shipping inside it"

### Task 4: The release publishes the guide

**Files:** Create `scripts/guide-fingerprint.mjs` (exports `fingerprint(dir)`; run directly, prints the hex for `process.argv[2]`), `scripts/guide-fingerprint.test.mjs` (vitest; if the vitest config does not collect `scripts/*.test.mjs`, put it at `src/lib/guideFingerprint.test.ts` importing the script); modify `scripts/release-v2.ps1`.

**Interfaces / behaviour:**
- `fingerprint(dir)` - the Global Constraints rule with Node's `crypto`.
- Release script, after the Velopack upload to phr-tcm succeeds: `$fp = node scripts/guide-fingerprint.mjs src-tauri/help`; create `Releases/how-to-use.zip` from `src-tauri/help` with `[System.IO.Compression.ZipFile]::CreateFromDirectory` (Optimal, no base directory); `Get-FileHash -Algorithm SHA256` and length -> write `Releases/how-to-use.json` (lowercase hex, UTF-8 without BOM - PS 5.1: `[System.IO.File]::WriteAllText(..., (New-Object System.Text.UTF8Encoding $false))`); `gh release upload "v$Version" Releases/how-to-use.zip Releases/how-to-use.json --repo hsenidBiz/phr-tcm --clobber`; `$LASTEXITCODE -ne 0` -> throw ("the guide upload failed - re-run just: gh release upload ..."). `-DryRun` prints the three steps and the fingerprint without uploading. Not uploaded to the legacy feed.

- [ ] **Step 1: Failing test** `the_fingerprint_matches_the_apps` - the same two-file fixture as Task 1, asserting the SAME pinned literal hex (Review Focus 5).
- [ ] **Step 2:** run it - FAIL (module missing).
- [ ] **Step 3:** Implement the script and the release-script section.
- [ ] **Step 4:** run the test - PASS; run `& .\scripts\release-v2.ps1 -Version 9.9.9-beta.1 -DryRun` in PowerShell and confirm it prints the guide steps and a 64-hex fingerprint (the dry run must not require a changelog entry for this check - if it does, report the dry-run output up to that refusal and note it).
- [ ] **Step 5:** Commit - "feat(v2): each release publishes How To Use beside the app"

### Task 5: Settings - Download, How To Use, Update Guide

**Files:** Modify `src/screens/Settings.tsx`, `src/screens/Settings.test.tsx`.

**Behaviour:**
- On mount, `commands.guideStatus()`; an error or no answer counts as `Ready` (so the existing tests, which do not mock it, see How To Use - and offline shows no error, Review Focus 3).
- `NotDownloaded`: one button `Download How to Use` + ` (N MB)` when `size` is known (whole MB, rounded up), icon from `actionIcons.ts` (a download icon - add `IconDownload` there if none exists, named for what it does).
- `Ready`: `How To Use`, as today.
- `UpdateAvailable`: `How To Use` and `Update Guide`.
- Download/Update: `commands.guideDownload()`; while running the button reads `Downloading...` and both buttons are disabled; progress from `events.guideProgress` shown as `12 of 31 MB` beside it; on error a toast with the returned sentence; on success the status is asked again.
- `How To Use` click keeps today's flow; an error toast shows the returned sentence (it is now `Download How to Use from Settings first.` when nothing is installed).

- [ ] **Step 1: Failing tests:** `a_guide_not_yet_downloaded_offers_to_download_it_with_its_size` (mock `guide_status` -> `{state:"NotDownloaded", size: 32505856}` -> button `Download How to Use (32 MB)`); `downloading_shows_progress_then_the_guide_is_ready` (mock `guide_download` pending; emit `guide-progress` {received: 12582912, total: 32505856} -> `12 of 32 MB`, button disabled; resolve -> status re-asked -> `How To Use`); `a_changed_guide_offers_update_guide`; `a_failed_download_says_why` (toast with the sentence); `offline_or_unanswered_status_shows_how_to_use`.
- [ ] **Step 2:** `npx vitest run src/screens/Settings.test.tsx` - FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS; `npx vitest run src/ui-consistency.test.ts src/a11y.test.tsx`; `npx tsc --noEmit`.
- [ ] **Step 5:** Commit - "feat(v2): Settings downloads How To Use on demand and offers Update Guide"

### Task 6: The guide's own text, and the whole branch

**Files:** Modify `docs-site/src/content/settings.ts` (the How To Use control's text: downloaded the first time you open it, then kept; Update Guide appears when a newer guide is published); run `npm run docs:build` (regenerates `src-tauri/help/`; the screenshot does not change).

- [ ] **Step 1:** Edit the text; `npm run docs:build`; `npx vitest run docs-site` (the guard tests: built page fresh, no hidden terms).
- [ ] **Step 2:** `cd src-tauri && cargo test --tests`; `npm test`; `npx tsc --noEmit`; `npm run build` - all pass.
- [ ] **Step 3:** Commit - "docs(v2): How To Use says it is downloaded on demand"
