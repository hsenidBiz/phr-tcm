//! Opening the "How To Use" guide (docs-site/, built to `src-tauri/help/`).
//! The site is not in the exe: a release publishes it beside the installer
//! and `guide.rs` downloads it on demand. A development build opens the
//! repository's copy directly, so `npm run docs:build` is all it takes to
//! see a guide change.

/// The repository's built site, opened by a development build. Its path is
/// fixed at compile time; nothing is copied or downloaded.
pub const DEV_INDEX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/help/index.html");

/// `open_help`'s failure message. Names no URL or path, per the app's rule
/// that user-facing errors never do (see `ado/transport.rs`).
pub const OPEN_ERROR: &str = "Could not open the help pages. Settings, Logs has the details.";

/// Hand `index` to the default browser - the same opener call every other
/// "open in browser" button in this app uses. The raw reason goes to the
/// log; the person gets `OPEN_ERROR`.
pub fn open_index(index: &std::path::Path) -> Result<(), String> {
    tauri_plugin_opener::open_path(index, None::<&str>).map_err(|e| {
        crate::applog::warn(format!("could not open the help site: {e}"));
        OPEN_ERROR.to_string()
    })
}
