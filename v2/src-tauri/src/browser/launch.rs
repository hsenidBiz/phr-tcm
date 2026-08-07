//! Where the browser lives and how it is started.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Every place the Edge installers put msedge.exe, 64-bit first. Same
/// shape as `ai_tools::claude_cli_candidates` and for the same reason:
/// PATH is not trustworthy enough to be the only answer.
pub fn edge_candidates(program_files: &str, program_files_x86: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from(program_files).join(r"Microsoft\Edge\Application\msedge.exe"),
        PathBuf::from(program_files_x86).join(r"Microsoft\Edge\Application\msedge.exe"),
    ]
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
    let candidates = edge_candidates(
        &env_or("ProgramFiles", r"C:\Program Files"),
        &env_or("ProgramFiles(x86)", r"C:\Program Files (x86)"),
    );
    let exe = candidates
        .iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "Microsoft Edge was not found in either Program Files".to_string())?;

    let port = free_port()?;
    let profile_dir = std::env::temp_dir().join(format!("tcm-autorun-{port}"));
    std::fs::create_dir_all(&profile_dir).map_err(|e| e.to_string())?;

    let child = Command::new(exe)
        .args(launch_args(port, &profile_dir))
        .spawn()
        .map_err(|e| format!("could not start Edge: {e}"))?;

    Ok(LaunchedBrowser { child, port, profile_dir })
}
