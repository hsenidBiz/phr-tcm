# Running in the tray, and beta releases - design

Date: 2026-09-27. Branch: `feat/tray-beta-channel`. Two independent features in
one spec; the plan implements Part A, then Part B.

## Why

- **Tray.** The AI bridge (the MCP tools an assistant uses) lives inside the
  app's process. Closing the window today quits the app, and the tools vanish
  with it. The owner wants closing to leave the app running in the Windows
  notification area (the taskbar's ^), so the tools stay available.
- **Beta releases.** Releases go out often. Most of them should reach only the
  people who ask for them, so everyone else updates less. Every install moves
  to stable releases; anyone can opt in to betas in Settings. A person on a
  beta who opts out keeps that build until a newer stable release ships.

## Owner decisions (2026-09-27)

- Tray extras: a first-close notice, a Settings switch for close-to-tray, and
  an opt-in Start with Windows.
- Beta versions carry a suffix: `1.26.0-beta.1`, `1.26.0-beta.2`, then the
  stable `1.26.0`.
- What's new shows every entry newer than the version last seen, betas
  included; beta entries are tagged "Beta".

---

## Part A - running in the tray

### Behaviour

- Closing the **main** window hides it; the process keeps running with
  everything it runs today (AI bridge, file watches, the assigned-work check,
  notifications). Other windows (the runner, dialogs) close exactly as now.
- A tray icon (the app icon, tooltip "Test Case Manager") is present while the
  app runs:
  - left click shows, unminimizes and focuses the main window;
  - right click opens a menu: **Open Test Case Manager**, **Quit**.
- Launching the app again (Start menu, shortcut) brings the hidden window
  back - the existing single-instance handler already calls
  `focus_main_window`, which shows the window.
- **Quit** exits the process. The existing exit clean-up
  (`close_autorun_on_exit` on `RunEvent::Exit`) still runs. **Restart to
  update** is unchanged: Velopack's apply ends the process itself.
- **First-close notice.** The first time the window hides, a Windows
  notification: "Test Case Manager is still running. Its tools stay available
  to your AI assistant. Right-click the tray icon to quit." Shown once per
  machine; remembered in the settings file (below). Sent through the
  notification plugin the app already uses.

### Settings - a new "Background" section

- **Keep running in the tray when closed** - on by default. Off: closing the
  main window quits the app, as today. The tray icon is present either way
  (it is also the way back to a window hidden by the switch being on when the
  person changes it).
- **Start with Windows** - off by default. On: the app is registered to start
  at sign-in with `--hidden`, and starts with no window, in the tray. Uses
  `tauri-plugin-autostart` (a new dependency; Windows registry Run key). The
  switch reads the plugin's own state (`is_enabled`), so it always shows the
  truth; turning it off removes the entry.
- A launch with `--hidden` does not show the main window; a later launch
  without it (the person opening the app) shows it through single-instance.
  The main window is created hidden (`visible: false` in `tauri.conf.json`)
  and shown by setup unless `--hidden` is present, so a hidden start never
  flashes a window.

### Where it lives

- `src-tauri/src/tray.rs` - tray icon and menu, the close handler
  (`WindowEvent::CloseRequested` on `main` → `prevent_close` + hide, when the
  switch is on), the first-close notice.
- `src-tauri/src/app_settings.rs` - one small JSON file in the app data dir,
  `app-settings.json`, read at startup and written by commands:
  `{ close_to_tray: bool (default true), close_notice_shown: bool (default
  false), beta_updates: bool (default false) }`. Missing or unreadable file =
  defaults; unknown fields ignored. Rust owns it because the close handler and
  the updater need it before (or without) any page.
- Commands (generated bindings): `get_app_settings`, `set_close_to_tray(bool)`,
  `get_autostart`, `set_autostart(bool)`; Part B adds `set_beta_updates(bool)`.
- `tauri` gains the `tray-icon` feature.
- Frontend: Settings "Background" section with the two switches; no other UI.

### Error handling

- Tray creation failing (it should not on Windows) is logged; closing then
  quits as today rather than hiding a window nobody can get back.
- A settings file write failure is logged and the command returns an error
  sentence (no path in it); the switch reverts.
- Autostart enable/disable failure: logged, error sentence, switch reverts.

### Tests

- Rust (`tests/suite/app_settings.rs`): defaults when missing / corrupt,
  round-trip, unknown fields ignored, the first-close marker set once.
- Rust: the close decision as a pure function (`close_action(label,
  close_to_tray, tray_ok) -> Hide | Close`) - main + on + tray → Hide; any
  other window → Close; switch off → Close; no tray → Close.
- Frontend: the Background section's switches call the commands and revert on
  error.
- Manual walk-through (no harness clicks a tray): close hides, tray click
  reopens, menu Quit exits, second launch shows the window, first-close notice
  once, switch off quits, Start with Windows adds/removes the entry and starts
  hidden.

---

## Part B - beta releases

### Versions

- A beta is `X.Y.Z-beta.N` (N ≥ 1); a stable is `X.Y.Z`. Semver ordering:
  `1.25.31 < 1.26.0-beta.1 < 1.26.0-beta.2 < 1.26.0 < 1.26.1-beta.1`.
- Every place that compares or validates versions learns the suffix:
  - `updater::parse_version` (Rust) - used by the failed-update-attempt check.
  - `compareVersions` (`src/lib/changelog.ts`) - today it splits on dots, so
    `1.26.0-beta.1` reads as `[1,26,0,1]` and counts as NEWER than `1.26.0`.
    It must follow semver prerelease ordering.
  - `scripts/release-v2.ps1` - accepts `^\d+\.\d+\.\d+(-beta\.\d+)?$`; the
    three-place version check and the changelog-entry check use the full
    string.
- **Risk to settle first:** Tauri must build a Windows exe for a suffixed
  version (the exe's file-version field is numeric). The plan's first Part B
  task is a throwaway `tauri build` at `1.26.0-beta.1` (not released); if it
  fails, the fix (e.g. Tauri's `bundle.windows` version override) is designed
  before anything else depends on the suffix. Velopack itself accepts semver 2
  prerelease versions.

### Publishing

- `release-v2.ps1 -Version 1.26.0-beta.1` publishes that release as a GitHub
  **prerelease** (`vpk upload github ... --pre`); a plain `X.Y.Z` publishes
  exactly as today. Same repository, same Velopack channel - no second channel
  (a separate beta channel would cut beta users off from stables unless every
  stable were published twice). `-AlsoLegacy` refuses a beta.

### Updating

- The updater's GitHub source gets `prerelease = beta_updates`:
  - stable (default): non-prerelease releases only;
  - beta: every release; Velopack picks the highest version, beta or stable.
- **Every existing install already passes `prerelease = false`**, so nothing
  already out there ever moves to a beta. New installs default to stable. This
  is the "all apps switch to the normal release" requirement.
- Opting out on a beta build: the next check reads stables only. Velopack
  offers an update only for a HIGHER version and never downgrades (its
  `AllowVersionDowngrade` stays off), so the app stays on its beta until a
  stable numbered above it ships.
- **Fallback when the API window has no stable.** Velopack's GitHub source
  reads only the 10 most recent releases. After more than 10 betas in a row, a
  stable-only check finds nothing there (`RemoteIsEmpty`). The updater treats
  an empty answer from the GitHub source as "try the next source" and falls
  through to the `latest/download` mirror, which GitHub points at the newest
  non-prerelease release - so a stable install never reads "up to date" just
  because the window was full of betas.
- Changing the switch runs a fresh update check at once.

### Settings - Updates section

- **Download beta builds** switch, off by default, stored as `beta_updates` in
  `app-settings.json` (Part A). The updater reads it at each check.
- The version line reads `Version 1.26.0-beta.1 (beta)` on a beta build.
- On a beta build with the switch off: "You're on a beta build. It stays until
  the next stable release."

### Changelog and What's new

- Beta builds get changelog entries like any release (`version:
  "1.26.0-beta.1"`), and the stable entry lists only what changed since the
  last beta.
- What's new shows every entry newer than the version last seen, up to the
  running one (unchanged logic, fixed comparison) - a stable user jumping
  `1.25.31 → 1.26.0` sees each beta's entry and the stable's.
- Entries whose version has a `-beta.` suffix carry a "Beta" tag in the
  Settings changelog and in What's new.

### Tests

- Rust: `parse_version` / ordering with and without the suffix; the source
  list for each setting (prerelease flag); the empty-answer fall-through; the
  failed-attempt check across a beta → stable move.
- Frontend: `compareVersions` ordering table (including
  `1.26.0-beta.2 < 1.26.0`); `entriesSince` across betas; the Beta tag; the
  switch calls the command and triggers a check; the beta-build notes.
- Release script: a PowerShell-free test of the version pattern and the upload
  arguments is not practical in this repo's suites; the plan adds a
  `-DryRun` switch to `release-v2.ps1` that prints the `vpk upload` arguments
  and the version checks without building or publishing, and the plan runs it
  for a beta and a stable version.

## Out of scope

- Applying a pending update automatically while the window is hidden.
- A separate beta repository or Velopack channel.
- Changing how stable releases are published.
