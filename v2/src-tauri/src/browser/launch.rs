//! Where the browser lives and how it is started.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Which browser the run is watched in. Both are Chromium, so both speak
/// the same DevTools Protocol and take the same switches - the only thing
/// that differs is where the installer put the exe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Browser {
    Edge,
    Chrome,
}

impl Browser {
    /// Map a stored preference. Anything unrecognised is Edge: this app is
    /// Windows-first and Edge is the one browser guaranteed to be present,
    /// so an unknown name should still open something rather than fail.
    pub fn from_name(name: &str) -> Browser {
        match name.trim().to_ascii_lowercase().as_str() {
            "chrome" => Browser::Chrome,
            _ => Browser::Edge,
        }
    }

    fn relative_exe(self) -> &'static str {
        match self {
            Browser::Edge => r"Microsoft\Edge\Application\msedge.exe",
            Browser::Chrome => r"Google\Chrome\Application\chrome.exe",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Browser::Edge => "Microsoft Edge",
            Browser::Chrome => "Google Chrome",
        }
    }
}

/// Every place this browser's installers put its exe, 64-bit first. Same
/// shape as `ai_tools::claude_cli_candidates` and for the same reason:
/// PATH is not trustworthy enough to be the only answer.
///
/// `local_app_data` matters for Chrome and only for Chrome: its consumer
/// installer run without elevation lands in the user's own profile, which
/// is exactly what happens on a locked-down work machine where the tester
/// cannot elevate. Edge is always machine-wide, so it never appears there
/// - listing the path for it anyway would just be a stat that always
/// fails.
pub fn browser_candidates(
    which: Browser,
    program_files: &str,
    program_files_x86: &str,
    local_app_data: &str,
) -> Vec<PathBuf> {
    let rel = which.relative_exe();
    let mut out = vec![
        PathBuf::from(program_files).join(rel),
        PathBuf::from(program_files_x86).join(rel),
    ];
    if which == Browser::Chrome {
        out.push(PathBuf::from(local_app_data).join(rel));
    }
    out
}

/// Edge specifically. Kept as its own name because it reads better at the
/// call sites that only ever meant Edge, and its test pins the paths.
pub fn edge_candidates(program_files: &str, program_files_x86: &str) -> Vec<PathBuf> {
    browser_candidates(Browser::Edge, program_files, program_files_x86, "")
}

/// The arguments the run needs: a debugging port to drive it through, a
/// throwaway profile so no cookie or extension from yesterday leaks into
/// today's result, and NOTHING that hides the window.
pub fn launch_args(port: u16, profile_dir: &Path) -> Vec<String> {
    vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", profile_dir.display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-popup-blocking".to_string(),
        "about:blank".to_string(),
    ]
}

/// Ask the OS for a port, then let it go: the browser binds it a moment
/// later. The gap is a race in theory and has never been one in
/// practice, and it beats guessing a fixed port that a stale browser
/// might still hold.
pub fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}

pub struct LaunchedBrowser {
    pub child: Child,
    pub port: u16,
    pub profile_dir: PathBuf,
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// Start Edge. Errors name the thing to fix rather than a code.
pub fn launch() -> Result<LaunchedBrowser, String> {
    launch_in(Browser::Edge)
}

/// Start the chosen browser. The error names the browser the person
/// asked for, so "not found" is actionable rather than a mystery.
pub fn launch_in(which: Browser) -> Result<LaunchedBrowser, String> {
    let candidates = browser_candidates(
        which,
        &env_or("ProgramFiles", r"C:\Program Files"),
        &env_or("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        &env_or("LOCALAPPDATA", ""),
    );
    let exe = candidates.iter().find(|p| p.is_file()).ok_or_else(|| {
        format!(
            "{} is not installed on this machine - pick the other browser in Auto Run",
            which.label()
        )
    })?;

    let port = free_port()?;
    let profile_dir = std::env::temp_dir().join(format!("tcm-autorun-{port}"));
    std::fs::create_dir_all(&profile_dir).map_err(|e| e.to_string())?;

    let child = Command::new(exe)
        .args(launch_args(port, &profile_dir))
        // Never the app's own cwd: a browser that inherits the install's
        // `current\` pins it, and the next update cannot rename it. See
        // `leave_install_dir` in lib.rs for the update that taught us this.
        .current_dir(&profile_dir)
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", which.label()))?;

    Ok(LaunchedBrowser { child, port, profile_dir })
}
