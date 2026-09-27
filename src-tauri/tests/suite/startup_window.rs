//! The main window is shown by Rust in setup - immediately, long before the
//! page loads - painted the splash's colour until the page draws. The
//! config creates it hidden only so a start at sign-in (`--hidden`, Start
//! with Windows) can stay in the tray without a window flashing up.
//!
//! It must never go back to waiting for the FRONTEND to show it: that was
//! several seconds of nothing on screen on a slow machine (WebView2's cold
//! start plus the whole bundle), with the splash drawn into a window nobody
//! could see.

use std::path::Path;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn main_window() -> serde_json::Value {
    let raw = std::fs::read_to_string(manifest_dir().join("tauri.conf.json")).expect("tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&raw).expect("tauri.conf.json is JSON");
    conf["app"]["windows"][0].clone()
}

/// The `background: #rrggbb` on the `html, body` rule in index.html - the
/// colour the splash is painted on before any script runs.
fn splash_background() -> String {
    let html = std::fs::read_to_string(manifest_dir().join("../index.html")).expect("index.html");
    let rule = html
        .lines()
        .find(|l| l.contains("html, body") && l.contains("background:"))
        .expect("index.html styles html, body with a background");
    let at = rule.find("background:").unwrap() + "background:".len();
    rule[at..]
        .trim_start()
        .chars()
        .take_while(|c| *c == '#' || c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase()
}

#[test]
fn the_main_window_is_created_hidden_and_shown_by_setup_not_by_the_page() {
    assert_eq!(main_window()["visible"], serde_json::json!(false));
    let lib = std::fs::read_to_string(manifest_dir().join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("tray::start_hidden(std::env::args())") && lib.contains("tray::show_main("),
        "setup shows the main window unless the launch asked to start hidden"
    );
    assert!(
        lib.contains("|| !tray::tray_ok()"),
        "a hidden start with no tray icon still shows the window - nothing else could"
    );
    // The page never shows the window itself.
    let src = std::fs::read_to_string(manifest_dir().join("../src/main.tsx")).unwrap();
    assert!(!src.contains(".show()"), "main.tsx must not show the window");
}

#[test]
fn the_window_is_painted_the_splash_colour_until_the_page_draws() {
    let splash = splash_background();
    assert_eq!(splash.len(), 7, "splash background should be #rrggbb, got {splash:?}");
    let window = main_window()["backgroundColor"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .expect("the main window needs a backgroundColor, or it flashes white before the page paints");
    assert_eq!(window, splash);
}
