//! Opening the "How To Use" guide (docs-site/, built to `src-tauri/help/`).
//! The site is not in the exe: a release publishes it beside the installer
//! and `guide.rs` downloads it on demand. A development build opens the
//! repository's copy directly, so `npm run docs:build` is all it takes to
//! see a guide change.

use std::collections::BTreeSet;

use crate::modules::Module;

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

/// The guide's address: `index` as a file URL, with
/// `?modules=<names>` naming the modules that are on (their snake_case
/// keys, comma-separated, in tree order), and no query while none is. The
/// guide hides every module's pages it is not told about, so unknown is
/// hidden.
pub fn guide_address(index: &std::path::Path, on: &BTreeSet<Module>) -> Option<String> {
    let mut url = tauri::Url::from_file_path(index).ok()?;
    let names = Module::ALL.into_iter().filter(|m| on.contains(m)).map(Module::key).collect::<Vec<_>>().join(",");
    url.set_query((!names.is_empty()).then(|| format!("modules={names}")).as_deref());
    Some(url.to_string())
}

/// A page that sends the browser on to `address`. A file path handed to
/// the browser cannot carry a query, so the guide is opened through this.
pub fn launcher_html(address: &str) -> String {
    let quoted = serde_json::to_string(address).unwrap_or_else(|_| "\"\"".to_string());
    let attr = address.replace('&', "&amp;").replace('"', "&quot;");
    format!(
        "<!doctype html>
<meta charset=\"utf-8\">
<title>How To Use</title>
<meta http-equiv=\"refresh\" content=\"0; url={attr}\">
<script>location.replace({quoted});</script>
"
    )
}

/// Where the launcher page is written: the temp folder, never beside the
/// guide (whose folder is fingerprinted and published as it is).
fn launcher_path() -> std::path::PathBuf {
    std::env::temp_dir().join("tcm-how-to-use.html")
}

/// How the guide is opened: its index itself while no module is on (no
/// query needed: the guide hides every module's pages without one), or
/// through a launcher page carrying `?modules=...` while any is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideOpening {
    Index,
    Launcher(String),
}

/// `GuideOpening` for this index and these modules. A path that is no
/// file URL opens the index itself.
pub fn guide_opening(index: &std::path::Path, on: &BTreeSet<Module>) -> GuideOpening {
    if on.is_empty() {
        return GuideOpening::Index;
    }
    match guide_address(index, on) {
        Some(address) => GuideOpening::Launcher(launcher_html(&address)),
        None => GuideOpening::Index,
    }
}

/// Open the guide at `index`, telling it which modules are on (as the app
/// decides what it shows: `modules::current`). Falls back to opening
/// `index` itself, which shows the guide with every module's pages hidden,
/// if the launcher cannot be written.
pub fn open_guide(index: &std::path::Path) -> Result<(), String> {
    let GuideOpening::Launcher(page) = guide_opening(index, &crate::modules::current()) else {
        return open_index(index);
    };
    let launcher = launcher_path();
    match std::fs::write(&launcher, page) {
        Ok(()) => open_index(&launcher),
        Err(e) => {
            crate::applog::warn(format!("could not write the help launcher: {e}"));
            open_index(index)
        }
    }
}
