# Running in the tray, and beta releases - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Closing the main window leaves the app running in the Windows tray (so the AI tools stay available), and releases can be published as betas that only opted-in installs receive.

**Architecture:** Part A adds a Rust-owned settings file (`app_settings.rs`), a tray module (`tray.rs`) that intercepts the main window's close, the Tauri autostart plugin, and a Settings "Background" section. Part B teaches every version comparison the `-beta.N` suffix, passes a `beta_updates` flag into the Velopack GitHub source's `prerelease` switch, adds a Settings switch and Beta tags, and teaches `release-v2.ps1` to publish a suffixed version as a GitHub prerelease.

**Tech Stack:** Tauri 2.11 (Rust), tauri-specta generated bindings, React 19 + TypeScript + vitest, Velopack 1.2.158, PowerShell 5.1 release scripts.

**Spec:** `docs/superpowers/specs/2026-09-27-tray-and-beta-channel-design.md`

## Global Constraints

- Every Rust test is a module under `src-tauri/tests/suite/` with a `mod` line in `suite/main.rs` - never `#[cfg(test)]` in `src/`, never a new file directly under `tests/`.
- `src/bindings.ts` is generated: after changing a command's signature or adding one, run `cd src-tauri && cargo test --test bindings`. Never hand-edit it.
- New commands are registered in `collect_commands![...]` in `src-tauri/src/lib.rs` `specta_builder()`.
- The Azure DevOps client stays GET / POST / PATCH only - no HTTP DELETE.
- User-facing errors name no URL or file path; log the raw error with `crate::applog` and return a sentence.
- Colours only from Tailwind tokens / CSS custom properties (`text-text`, `text-muted`, `bg-surface-2`, `border-border`, `text-warning` ...). `src/ui-consistency.test.ts` must stay green and must never be weakened.
- Icons in screens come from `src/lib/actionIcons.ts` (`Icon*` names).
- The machine is shared: run ONE build or test command at a time, sequentially.
- Commit with a Bash heredoc: `git commit -F - <<'EOF' ... EOF`, ending with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Stage only files this task changed. Confirm with `git log -1`.
- Settings defaults (spec): `close_to_tray` true, `close_notice_shown` false, `beta_updates` false; Start with Windows off.
- Beta versions match `^\d+\.\d+\.\d+-beta\.\d+$`; stable `^\d+\.\d+\.\d+$`. Ordering: `1.25.31 < 1.26.0-beta.1 < 1.26.0-beta.2 < 1.26.0-beta.10 < 1.26.0 < 1.26.1-beta.1`.
- Copy (verbatim): first-close notice title "Test Case Manager", body "Test Case Manager is still running. Its tools stay available to your AI assistant. Right-click the tray icon to quit."; tray tooltip "Test Case Manager"; menu items "Open Test Case Manager", "Quit"; switches "Keep running in the tray when closed", "Start with Windows", "Download beta builds"; beta note "You're on a beta build. It stays until the next stable release."
- Nothing is released, pushed or merged by any task.

## Review Focus

1. The runner (or any non-main) window closing while the main window is open or hidden - the runner closes, the main window neither hides nor shows, and the app does not quit. Pinned in Task 2 (`close_action` table).
2. The tray failing to build - closing the main window then quits as it did before, instead of hiding a window nobody can reopen. Pinned in Task 2 (`tray_ok = false` rows).
3. A settings file that is missing, empty, corrupt, or from a future version with extra fields - the app starts with the defaults (close to tray ON) and never errors. Pinned in Task 1.
4. `-beta.10` against `-beta.9`, and a dev build reporting "dev" - numeric prerelease ordering in both Rust and TypeScript, and "dev" still never opens What's new. Pinned in Tasks 6 and 8.
5. A beta install whose GitHub API is blocked - it falls back to the `latest/download` mirror (stables only), which is older than its beta, and must report "up to date", not offer a downgrade. Pinned in Task 7 (Velopack never downgrades with `AllowVersionDowngrade` off; the test pins that `managers()` never sets it).

---

## Part A - running in the tray

### Task 1: The Rust-owned settings file

**Files:**
- Create: `src-tauri/src/app_settings.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod app_settings;`, call `app_settings::init(dir.clone())` in setup next to `extras::init`, register two commands)
- Create: `src-tauri/src/commands/app_settings.rs` and add `pub mod app_settings;` to `src-tauri/src/commands/mod.rs`
- Test: `src-tauri/tests/suite/app_settings.rs` (+ `mod app_settings;` in `suite/main.rs`)

**Interfaces:**
- Produces:
  - `pub struct AppSettings { pub close_to_tray: bool, pub close_notice_shown: bool, pub beta_updates: bool }` - `Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type`; `Default` gives `true, false, false`.
  - `pub fn load(dir: &Path) -> AppSettings`, `pub fn save(dir: &Path, s: &AppSettings) -> Result<(), String>`
  - `pub fn init(dir: PathBuf)`, `pub fn current() -> AppSettings`, `pub fn update(f: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String>`
  - Commands: `get_app_settings() -> AppSettings`, `set_close_to_tray(on: bool) -> Result<AppSettings, String>`

- [ ] **Step 1: Write the failing tests** - `src-tauri/tests/suite/app_settings.rs`:

```rust
//! The app's own settings file (app-settings.json): read before any page
//! loads, so the close handler and the updater can use it. Anything it
//! cannot read is the defaults - never an error.

use v2_lib::app_settings::{load, save, AppSettings};

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn a_missing_file_is_the_defaults() {
    let d = dir();
    assert_eq!(
        load(d.path()),
        AppSettings { close_to_tray: true, close_notice_shown: false, beta_updates: false }
    );
}

#[test]
fn an_empty_or_corrupt_file_is_the_defaults() {
    let d = dir();
    for body in ["", "{", "not json", "[]", "null"] {
        std::fs::write(d.path().join("app-settings.json"), body).unwrap();
        assert_eq!(load(d.path()), AppSettings::default(), "body {body:?}");
    }
}

#[test]
fn missing_fields_take_their_defaults_and_unknown_fields_are_ignored() {
    let d = dir();
    std::fs::write(d.path().join("app-settings.json"), r#"{"beta_updates":true,"from_the_future":1}"#).unwrap();
    assert_eq!(
        load(d.path()),
        AppSettings { close_to_tray: true, close_notice_shown: false, beta_updates: true }
    );
}

#[test]
fn saved_settings_read_back() {
    let d = dir();
    let s = AppSettings { close_to_tray: false, close_notice_shown: true, beta_updates: true };
    save(d.path(), &s).unwrap();
    assert_eq!(load(d.path()), s);
}

#[test]
fn save_creates_the_folder() {
    let d = dir();
    let nested = d.path().join("a").join("b");
    save(&nested, &AppSettings::default()).unwrap();
    assert_eq!(load(&nested), AppSettings::default());
}
```

Check `tempfile` is already a dev-dependency (`grep -n tempfile src-tauri/Cargo.toml`); the suite's other modules use it. Add `mod app_settings;` to `src-tauri/tests/suite/main.rs` in alphabetical position.

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --test suite app_settings::`
Expected: compile error - `v2_lib::app_settings` not found.

- [ ] **Step 3: Implement** - `src-tauri/src/app_settings.rs`:

```rust
//! The app's own settings: whether closing the main window keeps it running
//! in the tray, whether the one-time notice about that has been shown, and
//! whether this install takes beta builds.
//!
//! Rust owns them, not the webview's storage, because they are needed
//! before any page loads (the close handler, the update check at launch)
//! and by code that has no page at all. One small file in the app data dir,
//! beside `extras.json` - and like that one, not `crate::cache`, which is
//! wiped when a different account signs in: these belong to the machine.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const FILE: &str = "app-settings.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AppSettings {
    /// Closing the main window hides it to the tray instead of quitting.
    pub close_to_tray: bool,
    /// The "still running in the tray" notice has been shown once.
    pub close_notice_shown: bool,
    /// Update checks include beta (prerelease) builds.
    pub beta_updates: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { close_to_tray: true, close_notice_shown: false, beta_updates: false }
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static CURRENT: Mutex<Option<AppSettings>> = Mutex::new(None);

/// What `dir` holds. Missing, unreadable or the wrong shape all read as the
/// defaults - never an error. Unknown fields are ignored and missing ones
/// take their default (`#[serde(default)]`).
pub fn load(dir: &Path) -> AppSettings {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
        .unwrap_or_default()
}

/// Write `s` into `dir` (created if missing), atomically.
pub fn save(dir: &Path, s: &AppSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let body = serde_json::to_string(s).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&dir.join(FILE), &body)
}

/// Called once from setup with the app data dir.
pub fn init(dir: PathBuf) {
    *CURRENT.lock().unwrap() = Some(load(&dir));
    let _ = DIR.set(dir);
}

/// The settings as this process last loaded or saved them. The defaults
/// before `init` (the `--mcp` proxy never runs setup).
pub fn current() -> AppSettings {
    CURRENT.lock().unwrap().clone().unwrap_or_default()
}

/// Change the settings and save them. On a failed save the in-memory copy
/// is left as it was, so the app never acts on a choice that was not kept.
pub fn update(f: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String> {
    let dir = DIR.get().ok_or_else(|| "settings are not available yet".to_string())?;
    let mut next = current();
    f(&mut next);
    save(dir, &next)?;
    *CURRENT.lock().unwrap() = Some(next.clone());
    Ok(next)
}
```

`src-tauri/src/commands/app_settings.rs`:

```rust
//! The app's own settings (`crate::app_settings`), for the Settings screen.

use crate::app_settings::{self, AppSettings};

/// The settings as the app is using them now.
#[tauri::command]
#[specta::specta]
pub fn get_app_settings() -> AppSettings {
    app_settings::current()
}

/// Whether closing the main window keeps the app running in the tray.
#[tauri::command]
#[specta::specta]
pub fn set_close_to_tray(on: bool) -> Result<AppSettings, String> {
    app_settings::update(|s| s.close_to_tray = on).map_err(|e| {
        crate::applog::warn(format!("saving the close-to-tray setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })
}
```

In `lib.rs`: add `pub mod app_settings;` (alphabetical among the `pub mod` lines), add `app_settings` to the `use commands::{...}` list in `specta_builder()`, add `app_settings::get_app_settings, app_settings::set_close_to_tray,` to `collect_commands!`, and in setup right after `extras::init(dir.clone());` add:

```rust
                // Close-to-tray and beta updates: read before any page (see app_settings.rs).
                crate::app_settings::init(dir.clone());
```

- [ ] **Step 4: Run tests and regenerate bindings**

Run: `cd src-tauri && cargo test --test suite app_settings::` - Expected: 5 passed.
Run: `cd src-tauri && cargo test --test bindings` - Expected: pass; `src/bindings.ts` now has `getAppSettings`, `setCloseToTray` and the `AppSettings` type.
Run: `npx tsc --noEmit` - Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app_settings.rs src-tauri/src/commands/app_settings.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src-tauri/tests/suite/app_settings.rs src-tauri/tests/suite/main.rs src/bindings.ts
git commit -F - <<'EOF'
feat(v2): the app keeps its own settings file, readable before any page loads

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2: The tray icon, hide-on-close, the first-close notice, and a hidden start

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/Cargo.toml` (`tauri = { version = "2", features = ["tray-icon"] }`)
- Modify: `src-tauri/src/lib.rs` (`pub mod tray;`, `.on_window_event(tray::on_window_event)`, tray build + show-unless-hidden in setup)
- Modify: `src-tauri/tauri.conf.json` (main window `"visible": false`)
- Test: `src-tauri/tests/suite/tray.rs` (+ `mod tray;`), update `src-tauri/tests/suite/startup_window.rs`

**Interfaces:**
- Consumes: `app_settings::{current, update}` (Task 1).
- Produces: `pub enum CloseAction { Hide, Close }`, `pub fn close_action(label: &str, close_to_tray: bool, tray_ok: bool) -> CloseAction`, `pub fn start_hidden<I: IntoIterator<Item = String>>(args: I) -> bool`, `pub fn build(app: &tauri::App) -> tauri::Result<()>`, `pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent)`, `pub fn show_main<R: tauri::Runtime>(app: &tauri::AppHandle<R>)`.

- [ ] **Step 1: Write the failing tests** - `src-tauri/tests/suite/tray.rs`:

```rust
//! Closing the main window hides it to the tray - only the main window,
//! only when the switch is on, and only when the tray actually exists
//! (otherwise nobody could get the window back).

use v2_lib::tray::{close_action, start_hidden, CloseAction};

#[test]
fn the_close_decision() {
    use CloseAction::*;
    // (window, close_to_tray, tray_ok) -> action
    let table = [
        ("main", true, true, Hide),
        ("main", false, true, Close),
        ("main", true, false, Close),
        ("main", false, false, Close),
        ("runner", true, true, Close),
        ("runner", false, true, Close),
        ("anything-else", true, true, Close),
    ];
    for (label, on, tray_ok, want) in table {
        assert_eq!(close_action(label, on, tray_ok), want, "{label} on={on} tray_ok={tray_ok}");
    }
}

#[test]
fn a_hidden_start_is_asked_for_with_the_hidden_argument_only() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(start_hidden(args(&["v2.exe", "--hidden"])));
    assert!(!start_hidden(args(&["v2.exe"])));
    assert!(!start_hidden(args(&["v2.exe", "--hidden-thing"])));
    assert!(!start_hidden(args(&["v2.exe", "hidden"])));
}
```

Update `src-tauri/tests/suite/startup_window.rs`: read the whole file first. Keep every test about the background colour. Replace the assertion that the main window is `"visible": true` with one that pins the new rule, and rewrite the module comment's first paragraph to say why:

```rust
//! The main window is shown by Rust in setup - immediately, long before the
//! page loads - painted the splash's colour until the page draws. The
//! config creates it hidden only so a start at sign-in (`--hidden`, Start
//! with Windows) can stay in the tray without a window flashing up.
//!
//! It must never go back to waiting for the FRONTEND to show it: that was
//! several seconds of nothing on screen on a slow machine (WebView2's cold
//! start plus the whole bundle), with the splash drawn into a window nobody
//! could see.
```

```rust
#[test]
fn the_main_window_is_created_hidden_and_shown_by_setup_not_by_the_page() {
    assert_eq!(main_window()["visible"], serde_json::json!(false));
    let lib = std::fs::read_to_string(manifest_dir().join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("tray::start_hidden(std::env::args())") && lib.contains("tray::show_main("),
        "setup shows the main window unless the launch asked to start hidden"
    );
    // The page never shows the window itself.
    let src = std::fs::read_to_string(manifest_dir().join("../src/main.tsx")).unwrap();
    assert!(!src.contains(".show()"), "main.tsx must not show the window");
}
```

(Keep the existing name of any helper in that file; `manifest_dir()` and `main_window()` exist there already.)

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --test suite tray:: startup_window::`
Expected: compile error - `v2_lib::tray` not found.

- [ ] **Step 3: Implement** - add the feature to `src-tauri/Cargo.toml`: change the line `tauri = { version = "2", features = [] }` to `tauri = { version = "2", features = ["tray-icon"] }`.

`src-tauri/src/tray.rs`:

```rust
//! The tray icon, and closing the main window to it.
//!
//! The AI bridge lives in this process, so quitting takes the assistant's
//! tools away. Closing the main window therefore hides it (when the
//! Settings switch is on), and the icon in the notification area brings it
//! back or quits for real. Other windows (the runner) close as they always
//! did.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, Runtime};

/// Set once the tray icon exists. Without it a hidden window could never
/// be brought back, so closing falls back to quitting.
static TRAY_OK: AtomicBool = AtomicBool::new(false);

const NOTICE_TITLE: &str = "Test Case Manager";
const NOTICE_BODY: &str =
    "Test Case Manager is still running. Its tools stay available to your AI assistant. Right-click the tray icon to quit.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    Hide,
    Close,
}

/// What closing the window `label` should do. Pure, so the table is tested.
pub fn close_action(label: &str, close_to_tray: bool, tray_ok: bool) -> CloseAction {
    if label == "main" && close_to_tray && tray_ok {
        CloseAction::Hide
    } else {
        CloseAction::Close
    }
}

/// Whether this launch asked to start in the tray with no window - the
/// argument Start with Windows registers.
pub fn start_hidden<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter().any(|a| a == "--hidden")
}

/// Show, unminimize and focus the main window.
pub fn show_main<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Build the tray icon and its menu. Called once from setup; a failure is
/// logged by the caller and leaves `TRAY_OK` false.
pub fn build(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Test Case Manager", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = MenuBuilder::new(app).item(&open).separator().item(&quit).build()?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Test Case Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => {
                crate::applog::info("quit from the tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    TRAY_OK.store(true, Ordering::SeqCst);
    Ok(())
}

/// The builder's window-event hook: hides the main window instead of
/// closing it, when `close_action` says so, and shows the one-time notice.
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    let tauri::WindowEvent::CloseRequested { api, .. } = event else { return };
    let settings = crate::app_settings::current();
    if close_action(window.label(), settings.close_to_tray, TRAY_OK.load(Ordering::SeqCst)) != CloseAction::Hide {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    if !settings.close_notice_shown {
        notify_still_running(window.app_handle());
        if let Err(e) = crate::app_settings::update(|s| s.close_notice_shown = true) {
            crate::applog::warn(format!("could not remember the tray notice was shown: {e}"));
        }
    }
}

fn notify_still_running<R: Runtime>(app: &tauri::AppHandle<R>) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(NOTICE_TITLE).body(NOTICE_BODY).show() {
        crate::applog::warn(format!("the tray notice could not be shown: {e}"));
    }
}
```

If `show_menu_on_left_click` does not exist in the locked tauri version, the older name is `menu_on_left_click(false)`; use whichever compiles (check with `grep -rn "fn show_menu_on_left_click\|fn menu_on_left_click" ~/.cargo/registry/src/*/tauri-2.11.5/src/tray/`).

`src-tauri/src/lib.rs`:
- add `pub mod tray;` among the modules;
- replace the body of `focus_main_window` with a call to `tray::show_main(app);` (keep the function and its callers);
- after `.invoke_handler(builder.invoke_handler())` add `.on_window_event(tray::on_window_event)`;
- at the end of `setup`, before `Ok(())`, add:

```rust
            // The tray icon, then the main window: created hidden by the
            // config, shown here at once unless this launch is a start at
            // sign-in (`--hidden`) - see tests/suite/startup_window.rs.
            if let Err(e) = tray::build(app) {
                applog::warn(format!("the tray icon could not be created - closing the window will quit: {e}"));
            }
            if !tray::start_hidden(std::env::args()) {
                tray::show_main(app.handle());
            } else {
                applog::info("started hidden in the tray");
            }
```

`src-tauri/tauri.conf.json`: in `app.windows[0]` change `"visible": true` to `"visible": false`.

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --test suite tray:: startup_window:: app_settings::` - Expected: all pass.
Run: `cd src-tauri && cargo test --test bindings` - Expected: pass (no binding change).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/tray.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json src-tauri/tests/suite/tray.rs src-tauri/tests/suite/startup_window.rs src-tauri/tests/suite/main.rs
git commit -F - <<'EOF'
feat(v2): closing the main window keeps the app running in the tray

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 3: Start with Windows

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `tauri-plugin-autostart = "2"` under `[dependencies]`, next to the other `tauri-plugin-*` lines)
- Modify: `src-tauri/src/lib.rs` (register the plugin; register two commands)
- Modify: `src-tauri/src/commands/app_settings.rs` (two commands)
- Test: `src-tauri/tests/suite/app_settings.rs` (argument wiring)

**Interfaces:**
- Produces: `pub const AUTOSTART_ARGS: &[&str] = &["--hidden"];` in `tray.rs`; commands `get_autostart(app: AppHandle) -> bool` and `set_autostart(app: AppHandle, on: bool) -> Result<bool, String>`.

- [ ] **Step 1: Write the failing test** - append to `src-tauri/tests/suite/app_settings.rs`:

```rust
/// Start with Windows registers the app with the argument that makes it
/// start in the tray - the same one `start_hidden` looks for.
#[test]
fn start_with_windows_asks_for_a_hidden_start() {
    use v2_lib::tray::{start_hidden, AUTOSTART_ARGS};
    let launch = std::iter::once("v2.exe".to_string()).chain(AUTOSTART_ARGS.iter().map(|a| a.to_string()));
    assert!(start_hidden(launch));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --test suite app_settings::` - Expected: compile error, `AUTOSTART_ARGS` not found.

- [ ] **Step 3: Implement**

In `tray.rs` add:

```rust
/// What Start with Windows launches the app with: straight to the tray.
pub const AUTOSTART_ARGS: &[&str] = &["--hidden"];
```

In `lib.rs`, after `.plugin(tauri_plugin_notification::init())`:

```rust
        // Start with Windows (Settings, off by default): a Run key entry
        // that launches the exe with --hidden, so it starts in the tray.
        // The exe path is Velopack's `current\` folder, which stays the
        // same across updates.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(tray::AUTOSTART_ARGS.to_vec()),
        ))
```

In `commands/app_settings.rs` add:

```rust
/// Whether the app is registered to start at sign-in. Read from the
/// registry each time, so the switch always shows the truth.
#[tauri::command]
#[specta::specta]
pub fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// Register or unregister the start at sign-in. Answers the state after.
#[tauri::command]
#[specta::specta]
pub fn set_autostart(app: tauri::AppHandle, on: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let done = if on { launcher.enable() } else { launcher.disable() };
    done.map_err(|e| {
        crate::applog::warn(format!("changing Start with Windows failed: {e}"));
        "Start with Windows could not be changed. Settings → Logs has the details.".to_string()
    })?;
    Ok(launcher.is_enabled().unwrap_or(on))
}
```

Register `app_settings::get_autostart, app_settings::set_autostart,` in `collect_commands!`.

- [ ] **Step 4: Run tests and regenerate bindings**

Run: `cd src-tauri && cargo test --test suite app_settings::` - Expected: 6 passed.
Run: `cd src-tauri && cargo test --test bindings` - Expected: pass; bindings gain `getAutostart`, `setAutostart`.
Run: `npx tsc --noEmit` - Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/src/tray.rs src-tauri/src/commands/app_settings.rs src-tauri/tests/suite/app_settings.rs src/bindings.ts
git commit -F - <<'EOF'
feat(v2): Start with Windows launches the app straight to the tray

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 4: Settings - the Background section

**Files:**
- Create: `src/components/BackgroundSettings.tsx`
- Modify: `src/screens/Settings.tsx` (render `<BackgroundSettings />` directly after the "AI tools" `<section>`)
- Modify: `src/dev/demo.ts` (sample-data answers for the four new commands, so dev and capture mode never raise "Session expired": find the object where `checkUpdate` is answered and add `getAppSettings: () => ok({ close_to_tray: true, close_notice_shown: true, beta_updates: false })`, `setCloseToTray: (on: boolean) => ok({ close_to_tray: on, close_notice_shown: true, beta_updates: false })`, `getAutostart: () => ok(false)`, `setAutostart: (on: boolean) => ok(on)` - match that file's own helper names and shapes for plain and Result commands)
- Test: `src/components/BackgroundSettings.test.tsx`

**Interfaces:**
- Consumes: `commands.getAppSettings()`, `commands.setCloseToTray(on)` (Result), `commands.getAutostart()`, `commands.setAutostart(on)` (Result) from `src/bindings.ts`.

- [ ] **Step 1: Write the failing test** - `src/components/BackgroundSettings.test.tsx`:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { expect, test } from "vitest";
import BackgroundSettings from "./BackgroundSettings";

function renderIt() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <BackgroundSettings />
    </QueryClientProvider>,
  );
}

test("shows the saved choices and changes them", async () => {
  const calls: string[] = [];
  let tray = true;
  let autostart = false;
  mockIPC((cmd, args) => {
    const a = args as { on?: boolean };
    if (cmd === "get_app_settings") return { close_to_tray: tray, close_notice_shown: false, beta_updates: false };
    if (cmd === "set_close_to_tray") {
      calls.push(`tray:${a.on}`);
      tray = !!a.on;
      return { close_to_tray: tray, close_notice_shown: false, beta_updates: false };
    }
    if (cmd === "get_autostart") return autostart;
    if (cmd === "set_autostart") {
      calls.push(`autostart:${a.on}`);
      autostart = !!a.on;
      return autostart;
    }
  });
  renderIt();
  const traySwitch = await screen.findByRole("switch", { name: "Keep running in the tray when closed" });
  const startSwitch = screen.getByRole("switch", { name: "Start with Windows" });
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "true"));
  expect(startSwitch).toHaveAttribute("aria-checked", "false");

  fireEvent.click(traySwitch);
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "false"));
  fireEvent.click(startSwitch);
  await waitFor(() => expect(startSwitch).toHaveAttribute("aria-checked", "true"));
  expect(calls).toEqual(["tray:false", "autostart:true"]);
});

test("a change that fails puts the switch back", async () => {
  mockIPC((cmd) => {
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: false, beta_updates: false };
    if (cmd === "set_close_to_tray") throw "The setting could not be saved. Settings → Logs has the details.";
    if (cmd === "get_autostart") return false;
    if (cmd === "set_autostart") throw "Start with Windows could not be changed. Settings → Logs has the details.";
  });
  renderIt();
  const traySwitch = await screen.findByRole("switch", { name: "Keep running in the tray when closed" });
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(traySwitch);
  await waitFor(() => expect(traySwitch).toHaveAttribute("aria-checked", "true"));
  const startSwitch = screen.getByRole("switch", { name: "Start with Windows" });
  fireEvent.click(startSwitch);
  await waitFor(() => expect(startSwitch).toHaveAttribute("aria-checked", "false"));
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run src/components/BackgroundSettings.test.tsx` - Expected: FAIL, module not found.

- [ ] **Step 3: Implement** - `src/components/BackgroundSettings.tsx`:

```tsx
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AppSettings } from "../bindings";
import { toast } from "../lib/toast";
import { Switch } from "./ui/switch";

/** Settings' Background section: whether closing the window keeps the app
 *  running in the tray (so the AI tools stay available), and whether it
 *  starts in the tray at sign-in. Both are kept by Rust. */
export default function BackgroundSettings() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const autostart = useQuery({ queryKey: ["autostart"], queryFn: () => commands.getAutostart() });

  const setTray = async (on: boolean) => {
    const before = settings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, close_to_tray: on });
    try {
      const r = await commands.setCloseToTray(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["app-settings"], r.data);
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };

  const setAutostart = async (on: boolean) => {
    const before = autostart.data ?? false;
    qc.setQueryData(["autostart"], on);
    try {
      const r = await commands.setAutostart(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["autostart"], r.data);
    } catch (e) {
      qc.setQueryData(["autostart"], before);
      toast.error(String(e));
    }
  };

  return (
    <section className="space-y-3">
      <h2 className="text-sm font-semibold text-text">Background</h2>
      <p className="text-sm text-muted">
        Closing the window can leave the app running in the notification area, so the tools your
        AI assistant uses stay available. Right-click its icon there to quit.
      </p>
      <label className="flex items-center gap-2 text-sm text-text">
        <Switch
          checked={settings.data?.close_to_tray ?? true}
          disabled={!settings.data}
          onCheckedChange={(on) => void setTray(on)}
          ariaLabel="Keep running in the tray when closed"
        />
        Keep running in the tray when closed
      </label>
      <label className="flex items-center gap-2 text-sm text-text">
        <Switch
          checked={autostart.data ?? false}
          disabled={autostart.data === undefined}
          onCheckedChange={(on) => void setAutostart(on)}
          ariaLabel="Start with Windows"
        />
        Start with Windows
      </label>
    </section>
  );
}
```

Check the toast import path used by `Settings.tsx` (`grep -n "toast" src/screens/Settings.tsx | head -3`) and use the same one. In `Settings.tsx`, import it (`import BackgroundSettings from "../components/BackgroundSettings";`) and render `<BackgroundSettings />` right after the closing `</section>` of the "AI tools" section.

- [ ] **Step 4: Run tests**

Run: `npx vitest run src/components/BackgroundSettings.test.tsx src/screens/Settings.test.tsx src/ui-consistency.test.ts src/dev` - Expected: all pass. (If a Settings test pins the left column's headings, add "Background" after "AI tools" there - that order is intended.)
Run: `npx tsc --noEmit` - Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/components/BackgroundSettings.tsx src/components/BackgroundSettings.test.tsx src/screens/Settings.tsx src/dev/demo.ts
git commit -F - <<'EOF'
feat(v2): Settings has a Background section - keep running in the tray, Start with Windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

## Part B - beta releases

### Task 5: Confirm a suffixed version builds (spike, nothing committed)

**Files:** none committed. Temporarily edits `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`; restores both.

- [ ] **Step 1:** Record `git status --short` (must be clean apart from untracked files).
- [ ] **Step 2:** Set the version to `1.26.0-beta.1` in `src-tauri/tauri.conf.json` (`"version"`) and in `src-tauri/Cargo.toml` (`[package] version`).
- [ ] **Step 3:** Run: `npm run tauri build -- --no-bundle` (long; one command, wait for it). Expected: `src-tauri/target/release/v2.exe` is built. If it fails, copy the error into the report and STOP - do not work around it.
- [ ] **Step 4:** Run: `vpk upload github --help` and record whether a `--pre` (prerelease) option exists and its exact spelling.
- [ ] **Step 5:** Restore: `git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock`; confirm `git status --short` matches Step 1.
- [ ] **Step 6:** Report: build OK or the error; the prerelease flag's exact spelling. No commit.

---

### Task 6: Version ordering in Rust knows the beta suffix

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `semver = "1"` - already in the lock through velopack)
- Modify: `src-tauri/src/updater/mod.rs` (`parse_version`, `failed_attempt`; add `is_beta`)
- Test: `src-tauri/tests/suite/updater.rs`

**Interfaces:**
- Produces: `pub fn parse_version(v: &str) -> Option<semver::Version>`, `pub fn is_beta(v: &str) -> bool`.

- [ ] **Step 1: Write the failing tests** - append to `src-tauri/tests/suite/updater.rs`:

```rust
#[test]
fn versions_order_with_the_beta_suffix() {
    use v2_lib::updater::parse_version;
    let order = ["1.25.31", "1.26.0-beta.1", "1.26.0-beta.2", "1.26.0-beta.10", "1.26.0", "1.26.1-beta.1"];
    let parsed: Vec<_> = order.iter().map(|v| parse_version(v).unwrap_or_else(|| panic!("{v} parses"))).collect();
    for pair in parsed.windows(2) {
        assert!(pair[0] < pair[1], "{} < {}", pair[0], pair[1]);
    }
    assert!(parse_version("dev").is_none());
    assert!(parse_version("1.26").is_none());
}

#[test]
fn a_beta_is_a_version_with_the_beta_suffix() {
    use v2_lib::updater::is_beta;
    assert!(is_beta("1.26.0-beta.1"));
    assert!(!is_beta("1.26.0"));
    assert!(!is_beta("dev"));
}

/// A failed "Restart to update" from a beta to the stable above it is still
/// recognised, and one that landed is not.
#[test]
fn a_failed_update_is_recognised_across_a_beta() {
    use v2_lib::updater::{failed_attempt, note_attempt};
    let d = tempfile::tempdir().unwrap();
    note_attempt(d.path(), "1.26.0");
    assert_eq!(failed_attempt(d.path(), "1.26.0-beta.3"), Some("1.26.0".into()));
    assert_eq!(failed_attempt(d.path(), "1.26.0"), None);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --test suite updater::` - Expected: compile errors (`is_beta` missing, `parse_version` private or returns a tuple).

- [ ] **Step 3: Implement** - in `updater/mod.rs` replace `parse_version` and its doc comment with:

```rust
/// A version as Velopack orders it: semver, so `1.26.0-beta.2` sits
/// between `1.26.0-beta.1` and `1.26.0`. Anything else is None - a marker
/// this cannot read is not worth alarming anyone over.
pub fn parse_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim()).ok()
}

/// Whether `v` is a beta build (`X.Y.Z-beta.N`).
pub fn is_beta(v: &str) -> bool {
    parse_version(v).is_some_and(|p| p.pre.as_str().starts_with("beta."))
}
```

`failed_attempt` already compares `(Some(t), Some(r)) if t > r` - it now compares `semver::Version`s and needs no other change. Add `semver = "1"` to `[dependencies]` in `Cargo.toml`.

- [ ] **Step 4: Run tests** - `cd src-tauri && cargo test --test suite updater::` - Expected: all pass (the existing `failed_attempt` tests too).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/updater/mod.rs src-tauri/tests/suite/updater.rs
git commit -F - <<'EOF'
feat(v2): the updater orders beta versions below the stable they lead to

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 7: The update check follows the beta setting

**Files:**
- Modify: `src-tauri/src/updater/mod.rs` (`sources`, `managers`, `check`, `download_and_apply` take `beta: bool`; extract `attempt_from`)
- Modify: `src-tauri/src/commands/misc.rs` (`check_update`, `apply_update` pass `app_settings::current().beta_updates`)
- Modify: `src-tauri/src/commands/app_settings.rs` (`set_beta_updates`), `src-tauri/src/lib.rs` (register it)
- Test: `src-tauri/tests/suite/updater.rs`

**Interfaces:**
- Consumes: `app_settings::{current, update}` (Task 1).
- Produces: `pub fn source_plan(beta: bool) -> Vec<(&'static str, bool)>` (name, includes prereleases), `pub fn sources(beta: bool)`, `pub fn check(state: &UpdateState, beta: bool) -> UpdateStatus`, `pub fn download_and_apply(state, data_dir, beta: bool, on_progress)`, `pub fn attempt_from(name: &str, r: Result<UpdateCheck, velopack::Error>) -> Attempt`; command `set_beta_updates(on: bool) -> Result<AppSettings, String>`.

- [ ] **Step 1: Write the failing tests** - append to `src-tauri/tests/suite/updater.rs`:

```rust
/// Stable installs read stable releases only; the beta switch adds
/// prereleases on the GitHub source. The `latest/download` mirror never
/// includes them - GitHub points it at the newest non-prerelease.
#[test]
fn the_sources_follow_the_beta_setting() {
    use v2_lib::updater::source_plan;
    assert_eq!(source_plan(false), vec![("github api", false), ("latest/download", false)]);
    assert_eq!(source_plan(true), vec![("github api", true), ("latest/download", false)]);
}

/// A source whose answer names no release (e.g. the GitHub API's last ten
/// releases were all betas, on a stable install) is not "up to date": the
/// check moves on to the next source.
#[test]
fn an_empty_answer_moves_on_to_the_next_source() {
    use v2_lib::updater::{attempt_from, Attempt};
    use velopack::UpdateCheck;
    assert!(matches!(attempt_from("github api", Ok(UpdateCheck::RemoteIsEmpty)), Attempt::Failed(_)));
    assert!(matches!(attempt_from("github api", Ok(UpdateCheck::NoUpdateAvailable)), Attempt::UpToDate));
}

/// Opting out of betas on a beta build must never offer the older stable:
/// the managers never allow a downgrade.
#[test]
fn no_update_manager_allows_a_downgrade() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/updater/mod.rs")).unwrap();
    assert!(!src.contains("AllowVersionDowngrade: true"), "downgrades stay off");
    assert!(src.contains("UpdateManager::new_boxed(src, None, None)"), "managers use Velopack's default options");
}
```

If `velopack::UpdateCheck::NoUpdateAvailable` is named differently in 1.2.158, use the variant `check_for_updates` returns for "nothing newer" (see `velopack-1.2.158/src/manager.rs`).

- [ ] **Step 2: Run to verify they fail** - `cd src-tauri && cargo test --test suite updater::` - Expected: compile errors.

- [ ] **Step 3: Implement** - in `updater/mod.rs`:

```rust
/// Each source's name and whether it reads prereleases (betas), in the
/// order they are tried. Pure, so the beta switch's effect is tested.
pub fn source_plan(beta: bool) -> Vec<(&'static str, bool)> {
    vec![("github api", beta), ("latest/download", false)]
}
```

Change `sources()` to `sources(beta: bool)`, building the GitHub source as `Box::new(sources::GithubSource::new(REPO_URL, None, beta))` (keep every existing comment; add one line: "`beta` adds prereleases - the Settings switch; stable installs never see a beta."). Change `managers()` to `managers(beta: bool)` calling `sources(beta)`. Change `check(state)` to `check(state: &UpdateState, beta: bool)` calling `managers(beta)`, and move its match into:

```rust
/// What one source's answer means. `RemoteIsEmpty` is a failed attempt, not
/// "up to date", so the next source is asked - see `check`.
pub fn attempt_from(name: &str, r: Result<UpdateCheck, velopack::Error>) -> Attempt {
    match r {
        Ok(UpdateCheck::UpdateAvailable(info)) => Attempt::Available(info),
        Ok(UpdateCheck::RemoteIsEmpty) => Attempt::Failed(FEED_EMPTY.into()),
        Ok(_) => Attempt::UpToDate,
        Err(e) => {
            crate::applog::warn(format!("update check failed via {name}: {e}"));
            Attempt::Failed(FEED_UNREACHABLE.into())
        }
    }
}
```

(keep the existing comments about `RemoteIsEmpty` on that arm), with `check`'s loop becoming `let a = attempt_from(name, um.check_for_updates());`. Change `download_and_apply(state, data_dir, on_progress)` to `download_and_apply(state, data_dir, beta: bool, on_progress)` calling `managers(beta)` - the re-check before downloading must read the same releases the banner came from.

In `commands/misc.rs`: `updater::check(&state)` becomes `updater::check(&state, crate::app_settings::current().beta_updates)`, and the `download_and_apply` call passes `crate::app_settings::current().beta_updates` as the new third argument.

In `commands/app_settings.rs`:

```rust
/// Whether update checks include beta builds. Turning it off on a beta
/// build keeps that build until a newer stable one ships - the updater
/// never downgrades.
#[tauri::command]
#[specta::specta]
pub fn set_beta_updates(on: bool) -> Result<AppSettings, String> {
    app_settings::update(|s| s.beta_updates = on).map_err(|e| {
        crate::applog::warn(format!("saving the beta updates setting failed: {e}"));
        "The setting could not be saved. Settings → Logs has the details.".to_string()
    })
}
```

Register `app_settings::set_beta_updates,` in `collect_commands!`. Fix any other caller of the changed functions the compiler reports (`grep -rn "updater::check\|download_and_apply\|updater::sources" src-tauri`).

- [ ] **Step 4: Run tests and regenerate bindings**

Run: `cd src-tauri && cargo test --test suite updater::` - Expected: all pass.
Run: `cd src-tauri && cargo test --test bindings` - Expected: pass; bindings gain `setBetaUpdates`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/mod.rs src-tauri/src/commands/misc.rs src-tauri/src/commands/app_settings.rs src-tauri/src/lib.rs src-tauri/tests/suite/updater.rs src/bindings.ts
git commit -F - <<'EOF'
feat(v2): update checks include beta builds only when this install opts in

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 8: The changelog knows betas - ordering and the Beta tag

**Files:**
- Modify: `src/lib/changelog.ts` (`compareVersions`, add `isBetaVersion`)
- Create: `src/components/ChangelogVersionTitle.tsx` (the "Version X [Beta] date" heading shared by the modal and Settings)
- Modify: `src/components/ChangelogModal.tsx`, `src/screens/Settings.tsx` (`ChangelogVersion` uses the shared heading)
- Test: `src/lib/changelog.test.ts`, `src/components/ChangelogModal.test.tsx` (create if absent)

**Interfaces:**
- Produces: `compareVersions(a, b): -1 | 0 | 1` (semver-aware), `isBetaVersion(v: string): boolean`, `<ChangelogVersionTitle entry={entry} />`.

- [ ] **Step 1: Write the failing tests** - append to `src/lib/changelog.test.ts` (import `isBetaVersion` alongside the existing imports):

```ts
test("compareVersions orders betas below the stable they lead to", () => {
  const order = ["1.25.31", "1.26.0-beta.1", "1.26.0-beta.2", "1.26.0-beta.10", "1.26.0", "1.26.1-beta.1"];
  for (let i = 0; i < order.length - 1; i++) {
    expect(compareVersions(order[i], order[i + 1]), `${order[i]} < ${order[i + 1]}`).toBe(-1);
    expect(compareVersions(order[i + 1], order[i])).toBe(1);
  }
  expect(compareVersions("1.26.0-beta.2", "1.26.0-beta.2")).toBe(0);
});

test("a dev build still compares equal-ish, so What's new stays shut", () => {
  expect(compareVersions("dev", "dev")).toBe(0);
  expect(compareVersions("dev", "0.0.0")).toBe(0);
});

test("isBetaVersion", () => {
  expect(isBetaVersion("1.26.0-beta.1")).toBe(true);
  expect(isBetaVersion("1.26.0")).toBe(false);
  expect(isBetaVersion("dev")).toBe(false);
});
```

`src/components/ChangelogModal.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import ChangelogModal from "./ChangelogModal";

test("beta entries carry a Beta tag, stable ones do not", () => {
  render(
    <ChangelogModal
      entries={[
        { version: "1.26.0", date: "2026-10-05", items: ["Stable."] },
        { version: "1.26.0-beta.1", date: "2026-10-01", items: ["Beta."] },
      ]}
      onClose={() => {}}
    />,
  );
  const headings = screen.getAllByRole("heading", { level: 3 });
  expect(headings.map((h) => h.textContent)).toEqual(["Version 1.26.0 2026-10-05", "Version 1.26.0-beta.1 Beta 2026-10-01"]);
});
```

- [ ] **Step 2: Run to verify they fail** - `npx vitest run src/lib/changelog.test.ts src/components/ChangelogModal.test.tsx` - Expected: FAIL (`isBetaVersion` missing; beta ordering wrong).

- [ ] **Step 3: Implement** - in `changelog.ts` replace `compareVersions` (and its comment) with:

```ts
/** Semver compare: -1 / 0 / 1 for a < b / a == b / a > b. A prerelease
 * (`1.26.0-beta.2`) sorts below its release (`1.26.0`), and its numeric
 * parts compare as numbers (`beta.10` > `beta.9`). Anything that is not
 * X.Y.Z (e.g. "dev") reads as 0.0.0, which keeps What's new shut in dev. */
export function compareVersions(a: string, b: string): number {
  const parse = (v: string) => {
    const m = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(v.trim());
    if (!m) return { core: [0, 0, 0], pre: [] as string[] };
    return { core: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split(".") : [] };
  };
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < 3; i++) {
    if (pa.core[i] !== pb.core[i]) return pa.core[i] < pb.core[i] ? -1 : 1;
  }
  // No prerelease outranks any prerelease of the same X.Y.Z.
  if (!pa.pre.length || !pb.pre.length) return pa.pre.length === pb.pre.length ? 0 : pa.pre.length ? -1 : 1;
  for (let i = 0; i < Math.max(pa.pre.length, pb.pre.length); i++) {
    const x = pa.pre[i];
    const y = pb.pre[i];
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const nx = /^\d+$/.test(x) ? Number(x) : null;
    const ny = /^\d+$/.test(y) ? Number(y) : null;
    if (nx !== null && ny !== null) {
      if (nx !== ny) return nx < ny ? -1 : 1;
    } else if (nx !== null || ny !== null) {
      return nx !== null ? -1 : 1;
    } else if (x !== y) {
      return x < y ? -1 : 1;
    }
  }
  return 0;
}

/** Whether `v` is a beta build's version (`X.Y.Z-beta.N`). */
export function isBetaVersion(v: string): boolean {
  return /^\d+\.\d+\.\d+-beta\.\d+$/.test(v.trim());
}
```

`src/components/ChangelogVersionTitle.tsx`:

```tsx
import { isBetaVersion, type ChangelogEntry } from "../lib/changelog";

/** "Version 1.26.0-beta.1 [Beta] 2026-10-01" - the heading of one version's
 *  notes, in What's new and in Settings' changelog. */
export default function ChangelogVersionTitle({ entry }: { entry: ChangelogEntry }) {
  return (
    <h3 className="text-xs font-semibold text-text">
      Version {entry.version}
      {isBetaVersion(entry.version) && (
        <span className="ml-2 rounded-full bg-warning/15 px-1.5 py-0.5 text-[10px] font-medium text-warning"> Beta</span>
      )}
      <span className="ml-2 font-normal text-faint"> {entry.date}</span>
    </h3>
  );
}
```

(The leading spaces inside the spans make `textContent` read "Version 1.26.0-beta.1 Beta 2026-10-01"; if the rendered text differs by whitespace, adjust the test's expected strings to the rendered form rather than adding markup.)

Replace the `<h3>...</h3>` blocks in `ChangelogModal.tsx` and in `Settings.tsx`'s `ChangelogVersion` with `<ChangelogVersionTitle entry={e} />` / `<ChangelogVersionTitle entry={entry} />`.

- [ ] **Step 4: Run tests** - `npx vitest run src/lib/changelog.test.ts src/components/ChangelogModal.test.tsx src/screens/Settings.test.tsx src/ui-consistency.test.ts` - Expected: all pass. `npx tsc --noEmit` - no errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/changelog.ts src/lib/changelog.test.ts src/components/ChangelogVersionTitle.tsx src/components/ChangelogModal.tsx src/components/ChangelogModal.test.tsx src/screens/Settings.tsx
git commit -F - <<'EOF'
feat(v2): What's new and the changelog order betas correctly and tag them Beta

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 9: Settings - Download beta builds

**Files:**
- Modify: `src/screens/Settings.tsx` (Updates section)
- Modify: `src/dev/demo.ts` (`setBetaUpdates: (on: boolean) => ok({ close_to_tray: true, close_notice_shown: true, beta_updates: on })`, same helper style as Task 4)
- Test: `src/screens/Settings.test.tsx`

**Interfaces:**
- Consumes: `commands.getAppSettings()`, `commands.setBetaUpdates(on)` (Result), `isBetaVersion` (Task 8), the existing `check` mutation in `Settings.tsx`.

- [ ] **Step 1: Write the failing tests** - append to `src/screens/Settings.test.tsx`, following the file's existing `renderSettings(qc)` helper and `mockIPC` style (read the top of the file first; the app version comes from `getVersion()`, which `mockIPC` answers for the command `plugin:app|version`):

```tsx
test("Download beta builds is off by default and turning it on checks for updates", async () => {
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:app|version") return "1.26.0";
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: true, beta_updates: false };
    if (cmd === "set_beta_updates") {
      calls.push(`beta:${(args as { on: boolean }).on}`);
      return { close_to_tray: true, close_notice_shown: true, beta_updates: (args as { on: boolean }).on };
    }
    if (cmd === "check_update") {
      calls.push("check");
      return { available: null, blocked: null, failed_attempt: null };
    }
    return undefined;
  });
  renderSettings(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  const beta = await screen.findByRole("switch", { name: "Download beta builds" });
  await waitFor(() => expect(beta).toHaveAttribute("aria-checked", "false"));
  expect(screen.queryByText(/You're on a beta build/)).not.toBeInTheDocument();
  fireEvent.click(beta);
  await waitFor(() => expect(calls).toEqual(["beta:true", "check"]));
});

test("a beta build says so, and says it stays when betas are off", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:app|version") return "1.26.0-beta.2";
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: true, beta_updates: false };
    return undefined;
  });
  renderSettings(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByText(/Version 1\.26\.0-beta\.2 \(beta\)/)).toBeInTheDocument();
  expect(screen.getByText("You're on a beta build. It stays until the next stable release.")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run to verify they fail** - `npx vitest run src/screens/Settings.test.tsx` - Expected: FAIL, no such switch.

- [ ] **Step 3: Implement** - in `Settings.tsx`, beside the existing `version` query and `check` mutation:

```tsx
  const appSettings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });
  const running = version.data ?? "";
  const onBeta = isBetaVersion(running);
  const setBeta = async (on: boolean) => {
    const before = appSettings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, beta_updates: on });
    try {
      const r = await commands.setBetaUpdates(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["app-settings"], r.data);
      check.mutate();
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };
```

and replace the Updates section's version paragraph with:

```tsx
        <p className="text-sm text-muted">
          Version {version.data ?? "-"}
          {onBeta ? " (beta)" : ""} - updates install automatically from the releases feed.
        </p>
        <label className="flex items-center gap-2 text-sm text-text">
          <Switch
            checked={appSettings.data?.beta_updates ?? false}
            disabled={!appSettings.data}
            onCheckedChange={(on) => void setBeta(on)}
            ariaLabel="Download beta builds"
          />
          Download beta builds
        </label>
        {onBeta && appSettings.data && !appSettings.data.beta_updates && (
          <p className="text-xs text-muted">You're on a beta build. It stays until the next stable release.</p>
        )}
```

Imports: `isBetaVersion` from `../lib/changelog`, `type AppSettings` from `../bindings` (and `useQueryClient`/`qc` already exist in `Settings.tsx` - reuse them; use the same `toast` import the file already has).

- [ ] **Step 4: Run tests** - `npx vitest run src/screens/Settings.test.tsx src/ui-consistency.test.ts src/dev` - Expected: all pass. `npx tsc --noEmit` - no errors.

- [ ] **Step 5: Commit**

```bash
git add src/screens/Settings.tsx src/screens/Settings.test.tsx src/dev/demo.ts
git commit -F - <<'EOF'
feat(v2): Settings offers Download beta builds, and a beta build says it is one

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 10: The release script publishes a suffixed version as a beta

**Files:**
- Modify: `scripts/release-v2.ps1`

**Interfaces:**
- Consumes: Task 5's findings - the exact spelling of vpk's prerelease option (expected `--pre`) and whether `--no-bundle` builds the exe at a suffixed version.

- [ ] **Step 1: Implement** - in `release-v2.ps1` (pure ASCII; PowerShell 5.1):
  - Add `[switch]$DryRun` to `param(...)`.
  - Replace the version check with:

```powershell
if ($Version -notmatch '^\d+\.\d+\.\d+(-beta\.\d+)?$') { throw "Version must be X.Y.Z or X.Y.Z-beta.N, got '$Version'" }
$IsBeta = $Version -match '-beta\.\d+$'
if ($IsBeta -and $AlsoLegacy) { throw "-AlsoLegacy does not take a beta: the old feed has no beta readers." }
```

  - Leave the three version checks (tauri.conf.json, Cargo.toml, changelog) as they are - they already compare the full string.
  - Build the upload arguments once, after the version checks:

```powershell
# A beta is published as a GitHub PRERELEASE on the same feed: installs
# read prereleases only when Settings > Download beta builds is on, so
# nothing on the stable line ever moves to a beta.
$uploadArgs = @("upload", "github", "--repoUrl", $repoUrl, "--publish", "--releaseName", "v$Version", "--tag", "v$Version")
if ($IsBeta) { $uploadArgs += "--pre" }
# Velopack packs only target\release\v2.exe. A beta skips Tauri's own
# installers (the MSI one refuses a non-numeric prerelease label).
$buildArgs = @("run", "tauri", "build")
if ($IsBeta) { $buildArgs += @("--", "--no-bundle") }
if ($DryRun) {
    Write-Host "Dry run: $Version (beta: $IsBeta)"
    Write-Host "  npm $($buildArgs -join ' ')"
    Write-Host "  vpk $($uploadArgs -join ' ') --token *** --outputDir Releases"
    return
}
```

    (Use the prerelease flag spelling Task 5 recorded if it is not `--pre`.)
  - Replace `npm run tauri build` with `& npm @buildArgs`.
  - Replace the phr-tcm `vpk upload github ...` line with `& vpk @uploadArgs --token $token --outputDir (Join-Path $v2 "Releases")` (keep the `$LASTEXITCODE` check after it).
  - The `-AlsoLegacy` upload stays as it is (it can no longer run for a beta).

- [ ] **Step 2: Dry-run both kinds** (no build, no push, no publish; the version checks run against the committed files, so they are expected to refuse a version the files do not carry - that refusal is part of the check):

Run: `powershell -NoProfile -File scripts/release-v2.ps1 -Version 1.26.0-beta.1 -DryRun`
Expected: the tauri.conf.json refusal ("tauri.conf.json says '1.25.30' ...") - proving the checks run before anything else.
Run: `powershell -NoProfile -File scripts/release-v2.ps1 -Version 1.26.0-beta -DryRun`
Expected: "Version must be X.Y.Z or X.Y.Z-beta.N".
Run: `powershell -NoProfile -File scripts/release-v2.ps1 -Version 1.26.0-beta.1 -AlsoLegacy -DryRun`
Expected: "-AlsoLegacy does not take a beta".
Then temporarily set `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` to `1.26.0-beta.1` and add a throwaway `version: "1.26.0-beta.1"` changelog entry, run `powershell -NoProfile -File scripts/release-v2.ps1 -Version 1.26.0-beta.1 -DryRun`, and expect the printed `npm run tauri build -- --no-bundle` and `vpk upload github ... --pre`. Restore with `git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml src/lib/changelog.ts`. Paste the printed lines into the report.

- [ ] **Step 3: Commit**

```bash
git add scripts/release-v2.ps1
git commit -F - <<'EOF'
feat(v2): release-v2.ps1 publishes an X.Y.Z-beta.N version as a GitHub prerelease

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 11: Documentation - the help site, README and CLAUDE.md

**Files:**
- Modify: `docs-site/src/content/settings.ts` (new controls), `README.md` (the tray, betas), `CLAUDE.md` (the release rule for betas)
- Shots: `docs-site/shots/**`, `docs-site/shots/positions.json`, `src-tauri/help/**` (re-captured, rebuilt)

- [ ] **Step 1:** In `docs-site/src/content/settings.ts` add a group `{ id: "background", title: "Running in the background", summary: "Keep the app running in the tray when its window is closed, and start it with Windows." }` after `help-and-options`, and these controls (match the file's style):
  - `id: "close-to-tray"`, shot `MORE`, group `background`, `locate: { role: "switch", name: "Keep running in the tray when closed" }`, name "Keep running in the tray when closed", does: "On by default. Closing the window leaves the app running in the notification area (the ^ on the taskbar), so your AI assistant's tools stay available. Click the icon to open the window again; right-click it and choose Quit to close the app. Off: closing the window closes the app."
  - `id: "start-with-windows"`, shot `MORE`, group `background`, `locate: { role: "switch", name: "Start with Windows" }`, name "Start with Windows", does: "Starts the app in the notification area when you sign in to Windows, without opening its window."
  - `id: "beta-builds"`, shot `MAIN` (or `MORE` if the dry run finds it off-screen in `MAIN`), group `backup-updates`, `locate: { role: "switch", name: "Download beta builds" }`, name "Download beta builds", does: "Off by default. On: the app also installs beta builds, which bring new features sooner. Turn it off and a beta build stays until the next stable release arrives."
  - Change the `MORE` shot's `scrollTo` to `{ role: "switch", name: "Start with Windows" }` so both new switches are on screen, and update its `alt` to "Settings scrolled down to How To Use, the interface tour, AI tools and Background".
- [ ] **Step 2:** `README.md`: in the features/updates part, add that closing the window keeps the app in the tray (Start with Windows optional) and that beta builds are opt-in from Settings; in the release notes for maintainers, that `release-v2.ps1 -Version X.Y.Z-beta.N` publishes a GitHub prerelease. `CLAUDE.md` "Releases" bullet: add one sentence - "A version `X.Y.Z-beta.N` is published as a GitHub prerelease that only installs with Download beta builds on receive; its changelog entry uses the same version string."
- [ ] **Step 3:** Screenshots (the coordinator may do this step itself): check no `tauri dev` is running (port 1420 free, no `target\debug\v2.exe` process); start `npm run docs:dev` in the background; wait for `http://127.0.0.1:9333/json/version`; run `npm run docs:shots -- --dry-run`, then `npm run docs:shots`; stop docs:dev; `npm run docs:build`; `npx vitest run docs-site scripts` - Expected: all pass.
- [ ] **Step 4: Commit**

```bash
git add docs-site README.md CLAUDE.md src-tauri/help
git commit -F - <<'EOF'
docs(v2): the guide, README and release notes cover the tray and beta builds

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

## Final verification (coordinator)

- `cd src-tauri && cargo test --tests` - all pass.
- `npx tsc --noEmit`, `npx vitest run` - all pass.
- Manual walk-through in `npm run tauri dev` (owner's machine): close hides to the tray; tray left-click reopens; right-click Quit exits; a second launch shows the hidden window; the first-close notice appears once; switch off → closing quits; Start with Windows on → a Run entry exists (`reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) with `--hidden`, off → removed; the runner window closes normally.
