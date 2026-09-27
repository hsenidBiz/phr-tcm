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

/// What Start with Windows launches the app with: straight to the tray.
pub const AUTOSTART_ARGS: &[&str] = &["--hidden"];

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
