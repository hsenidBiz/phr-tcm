//! The main window is visible from the moment the process creates it,
//! painted the splash's colour until the page draws.
//!
//! It used to start hidden and wait for the frontend to call `show()` once
//! React had mounted, to avoid a white flash. On a slow machine that wait -
//! WebView2's cold start plus loading and running the whole bundle - was
//! several seconds of a process in Task Manager and nothing on screen, and
//! the splash in index.html, made for exactly that wait, was drawn into a
//! window nobody could see. A background colour on the window and webview
//! avoids the flash without hiding anything.

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
fn the_main_window_is_shown_at_once() {
    assert_eq!(
        main_window()["visible"],
        serde_json::Value::Bool(true),
        "a hidden main window leaves a slow machine with nothing on screen until the whole bundle has run"
    );
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
