//! Where the browser lives and how it is started.

use super::tree::{self, Ends, Tree};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;

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
/// today's result, and nothing that hides the window BY ITSELF - a
/// supervised run never does; an unattended run adds `background_args`
/// unless the person asked to watch (see `commands::autorun_replay`).
///
/// The three `--disable-*` switches keep the APPLICATION UNDER TEST running
/// at full speed when its window is minimised or another window covers it.
/// Chromium otherwise throttles that page's timers to once a minute and
/// stops painting it, so the app's own debounces, toasts and animations
/// crawl, and a run that was fine while somebody watched it fails the
/// moment the window is put in the background. They do not hide the window
/// and they do not change what the page does - only when it gets to do it.
///
/// `--disable-sync` keeps the throwaway profile throwaway. Edge signs a new
/// profile into the Windows account by itself and syncs it: the person's
/// extensions install, one opens a tab of its own, and that tab is shown
/// instead of the page under test. A page that is not shown draws no
/// frames, and every role lookup (`Accessibility.queryAXTree`, which
/// answers only after a frame) then goes unanswered.
///
/// `--force-device-scale-factor=1` keeps a high-DPI screen from scaling the
/// page, and the window size holds the page size in force (1280 x 720 unless
/// the person changed it) with the browser's own frame and toolbar
/// (`viewport`). The page area itself is fixed per tab
/// over CDP, which wins over the window.
pub fn launch_args(port: u16, profile_dir: &Path) -> Vec<String> {
    vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", profile_dir.display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-popup-blocking".to_string(),
        "--disable-background-timer-throttling".to_string(),
        "--disable-backgrounding-occluded-windows".to_string(),
        "--disable-renderer-backgrounding".to_string(),
        "--disable-sync".to_string(),
        format!("--disable-features={PASSWORD_FEATURES_OFF}"),
        "--force-device-scale-factor=1".to_string(),
        super::viewport::window_size_arg(),
        "about:blank".to_string(),
    ]
}

/// The browser features switched off in every Auto Run browser, as the
/// value of the ONE `--disable-features` switch Chromium honours (a second
/// switch replaces the first, so any other feature is folded in by
/// `args_with`). Chrome's breach check ("Change your password") opens a
/// bubble over the page after a sign-in with a weak password.
pub const PASSWORD_FEATURES_OFF: &str = "PasswordLeakDetection";

const PASSWORD_PREFS_TOP: [&str; 1] = ["credentials_enable_service"];
const PASSWORD_PREFS_PROFILE: [&str; 2] = ["password_manager_enabled", "password_manager_leak_detection"];

/// The profile's `Preferences` with the browser's password saving and
/// breach warning off ("Save password?" and "Change your password" are
/// bubbles over the page under test). `existing` is the file's text when
/// there is one: every other key it holds is kept; text that is not a JSON
/// object is replaced.
pub fn password_prefs(existing: Option<&str>) -> serde_json::Value {
    use serde_json::{json, Value};
    let mut prefs = match existing.and_then(|t| serde_json::from_str::<Value>(t).ok()) {
        Some(v @ Value::Object(_)) => v,
        _ => json!({}),
    };
    let root = prefs.as_object_mut().expect("an object");
    for key in PASSWORD_PREFS_TOP {
        root.insert(key.to_string(), json!(false));
    }
    if !root.get("profile").is_some_and(Value::is_object) {
        root.insert("profile".to_string(), json!({}));
    }
    let profile = root.get_mut("profile").and_then(Value::as_object_mut).expect("an object");
    for key in PASSWORD_PREFS_PROFILE {
        profile.insert(key.to_string(), json!(false));
    }
    prefs
}

/// Write `<profile_dir>/Default/Preferences` from `password_prefs`, keeping
/// what a profile that is reused already has in it.
pub fn write_password_prefs(profile_dir: &Path) -> std::io::Result<()> {
    let default = profile_dir.join("Default");
    std::fs::create_dir_all(&default)?;
    let file = default.join("Preferences");
    let existing = std::fs::read_to_string(&file).ok();
    let text = serde_json::to_string(&password_prefs(existing.as_deref())).map_err(std::io::Error::other)?;
    std::fs::write(&file, text)
}

/// The line logged when the prefs could not be written - the launch goes
/// on (the prompts are a nuisance, a refused launch stops every run). Names
/// the kind of failure, never a path.
pub fn prefs_failure_is_logged_only(e: &std::io::Error) -> String {
    format!("Auto Run could not switch off the browser's password prompts: {:?}", e.kind())
}

/// `launch_args` plus extra switches, kept in front of the start page
/// (Chromium treats everything after the first non-switch as a URL).
pub fn args_with(port: u16, profile_dir: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = launch_args(port, profile_dir);
    let start_page = args.pop();
    // Chromium honours only one `--disable-features`: an extra one is
    // folded into ours, not added beside it.
    const SWITCH: &str = "--disable-features=";
    for extra_switch in extra {
        match extra_switch.strip_prefix(SWITCH) {
            Some(more) => {
                if let Some(ours) = args.iter_mut().find(|a| a.starts_with(SWITCH)) {
                    ours.push(',');
                    ours.push_str(more);
                } else {
                    args.push(extra_switch.to_string());
                }
            }
            None => args.push(extra_switch.to_string()),
        }
    }
    args.extend(start_page);
    args
}

/// Close each browser left from an earlier run whose DevTools port does
/// not answer (hung: it holds its profile and a sign-in nobody can use).
/// One that answers, or whose port is not known, is left alone: it may be
/// another slot's. `end` closes one and says whether it is gone. Returns
/// the profile folder names closed.
pub fn close_hung(
    orphans: Vec<tree::Orphan>,
    answers: &dyn Fn(u16) -> bool,
    end: &mut dyn FnMut(&tree::Orphan) -> bool,
) -> Vec<String> {
    let mut closed = Vec::new();
    for o in orphans {
        let Some(port) = o.port else { continue };
        if answers(port) {
            continue;
        }
        if end(&o) {
            closed.push(o.profile.clone());
        }
    }
    closed
}

/// Does a browser's DevTools port answer `/json/version` within 1.5 s?
/// A plain blocking request: `launch_with` is not async.
pub fn devtools_answers(port: u16) -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let limit = Duration::from_millis(1500);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, limit) else { return false };
    let _ = stream.set_read_timeout(Some(limit));
    let _ = stream.set_write_timeout(Some(limit));
    let request = format!("GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).is_ok() && head.starts_with(b"HTTP/1.")
}

/// Extra switches for an unattended run nobody is watching: headless, in a
/// window the size every browser the app drives starts with
/// (`viewport::window_size_arg`), rather than whatever default a headless
/// window happens to start at. The page itself is fixed per tab by
/// `viewport::device_metrics_params`.
pub fn background_args() -> [String; 2] {
    ["--headless=new".to_string(), super::viewport::window_size_arg()]
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

/// A browser the app started: the process it spawned, the DevTools port,
/// the throwaway profile, and (on Windows) the job every process of the
/// browser runs in (`tree`). Dropping it ends the browser's own processes
/// and closes the job's last handle, which ends what is left of the tree
/// unless a program the person opened from the browser was seen in the
/// job (then that program is left running): a browser the app forgets can
/// no longer outlive it. `close` is the tidy way, which also removes the
/// profile.
pub struct LaunchedBrowser {
    pub child: Child,
    pub port: u16,
    pub profile_dir: PathBuf,
    tree: Arc<Tree>,
    pid_gone_noted: bool,
}

impl LaunchedBrowser {
    /// The pid the app spawned. Edge can replace that process with another
    /// one, so it says nothing on its own about whether the browser runs.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// How many of the browser's own processes (those carrying its
    /// profile) its job holds now. `None` off Windows.
    pub fn processes(&self) -> Option<u32> {
        self.tree.processes()
    }

    /// Does the browser's job still kill what is in it if the app goes?
    /// It stops once a program the person opened is seen in it.
    pub fn kills_on_close(&self) -> Option<bool> {
        self.tree.kills_on_close()
    }

    /// Put a running process in the browser's job, as a program the
    /// person opened from the browser would be. Only the live tests use
    /// this.
    #[cfg(windows)]
    #[doc(hidden)]
    pub fn put_in_job(&self, child: &std::process::Child) -> Result<(), String> {
        self.tree.put_in_job(child)
    }

    /// Is this process one of the browser's (in its job)?
    pub fn holds(&self, pid: u32) -> bool {
        self.tree.holds(pid)
    }

    /// Has the process the app spawned ended?
    pub fn pid_ended(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    /// Does any process of the browser still run? On Windows that is the
    /// job, whatever became of the spawned pid; elsewhere the spawned pid.
    pub fn running(&mut self) -> bool {
        match self.processes() {
            Some(n) => n > 0,
            None => !self.pid_ended(),
        }
    }

    /// True the first time it is asked, false after: so that the spawned
    /// pid going is logged once per browser, not once per case.
    pub fn note_pid_gone(&mut self) -> bool {
        !std::mem::replace(&mut self.pid_gone_noted, true)
    }

    /// End the browser's whole process tree, wait until it is gone, then
    /// remove its profile. A browser whose processes are still there after
    /// `tree::END_WITHIN` is handed back: the caller must not start another
    /// one in its place, and dropping it is the last resort (its job
    /// closes and Windows kills what is left).
    pub fn close(mut self) -> Result<(), LaunchedBrowser> {
        if self.end() {
            Ok(())
        } else {
            Err(self)
        }
    }

    /// `close` in place: end the tree, wait, remove the profile. True when
    /// no process is left. For a holder that cannot give the value up,
    /// such as a `Drop`.
    pub fn end(&mut self) -> bool {
        #[cfg(not(windows))]
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let gone = self.tree.end();
        let _ = self.child.try_wait();
        gone
    }
}

/// What a liveness check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Its processes run and DevTools answers.
    Alive,
    /// None of its processes is left.
    Gone,
    /// Its processes still run, but DevTools did not answer.
    NotAnswering,
}

/// Is a launched browser still usable? Its own processes still run (or,
/// off Windows where there is no job, the spawned pid still runs) AND its
/// DevTools port answered. The spawned pid ending on Windows does not
/// matter: Edge can hand its browser to another process of the same job.
pub fn liveness(processes: Option<u32>, pid_ended: bool, devtools_answered: bool) -> Liveness {
    let running = match processes {
        Some(n) => n > 0,
        None => !pid_ended,
    };
    match (running, devtools_answered) {
        (false, _) => Liveness::Gone,
        (true, false) => Liveness::NotAnswering,
        (true, true) => Liveness::Alive,
    }
}

/// `liveness` as a yes or no.
pub fn still_alive(processes: Option<u32>, pid_ended: bool, devtools_answered: bool) -> bool {
    liveness(processes, pid_ended, devtools_answered) == Liveness::Alive
}

/// The one line logged when a browser's spawned pid is found gone: enough
/// to tell a browser that really closed from one that moved to another
/// process. Names no host and no port.
pub fn pid_gone_line(pid: u32, processes: Option<u32>, devtools_answered: bool) -> String {
    let count = processes.map_or("an unknown number of".to_string(), |n| n.to_string());
    let answered = if devtools_answered { "answered" } else { "did not answer" };
    format!("unattended run: the browser's first process (pid {pid}) has ended; {count} of its processes run and DevTools {answered}")
}

/// A free DevTools port and a new, empty profile folder named for it. The
/// folder is made with `create_dir`, never reused: one that already
/// exists (a browser still holding it, or one an older version left)
/// means another port is picked, so two browsers never share a profile.
fn fresh_profile() -> Result<(u16, PathBuf), String> {
    let mut last = String::new();
    for _ in 0..8 {
        let port = free_port()?;
        let dir = long_temp_dir().join(format!("{}{port}", tree::PROFILE_PREFIX));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok((port, dir)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e.to_string(),
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(last)
}

/// The temp folder in its long form. A `TMP` set to an 8.3 short path
/// (`C:\Users\ADMINI~1\...`) would give the browser a profile path its
/// processes might hand on in the long form, and a process of the browser
/// would then not be told for its own (`tree::carries_profile`).
pub(crate) fn long_temp_dir() -> PathBuf {
    let temp = std::env::temp_dir();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let wide: Vec<u16> = temp.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut buf = vec![0u16; 1024];
        // SAFETY: `wide` is NUL-terminated and `buf` holds `buf.len()` u16s.
        let n = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetLongPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32)
        } as usize;
        if n > 0 && n < buf.len() {
            return PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
        }
    }
    temp
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
    launch_with(which, &[])
}

/// `--window-position=x,y` for a window that opens there, none when it asks
/// for no position (the main browser, and every launch before there were
/// extra ones).
pub fn window_position_switch(position: Option<(i32, i32)>) -> Option<String> {
    position.map(|(x, y)| format!("--window-position={x},{y}"))
}

/// `launch_in`, with the window opened at `position` (screen pixels from the
/// top left) when one is given: an extra assistant's browser is offset from
/// the main one so the windows are not stacked exactly. `None` is `launch_in`.
pub fn launch_in_at(which: Browser, position: Option<(i32, i32)>) -> Result<LaunchedBrowser, String> {
    let switch = window_position_switch(position);
    match switch.as_deref() {
        Some(switch) => launch_with(which, &[switch]),
        None => launch_with(which, &[]),
    }
}

static LAUNCHING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The same, with extra switches. A supervised run passes none but a
/// window position (`launch_in_at`): the
/// window is always visible. An unattended run passes `background_args`
/// unless the person asked to watch, so it can run headless; the live
/// tests pass `--headless=new` for the same reason.
pub fn launch_with(which: Browser, extra_args: &[&str]) -> Result<LaunchedBrowser, String> {
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

    // Held until the new browser is registered: another slot's browser
    // that is starting has no tree yet and does not answer yet, and must
    // not be taken for a hung one.
    let _starting = LAUNCHING.lock().unwrap_or_else(|e| e.into_inner());
    // A browser an earlier run left that no longer answers would keep its
    // profile and any sign-in; close it before a new one starts. Only Auto
    // Run's own (`tcm-autorun-<port>` under the temp folder), never one this
    // app holds, never the person's.
    for name in close_hung(tree::find_orphans(&long_temp_dir()), &devtools_answers, &mut tree::end_orphan) {
        crate::applog::warn(format!(
            "Auto Run closed a browser left from an earlier run that did not answer (profile {name})"
        ));
    }
    let (port, profile_dir) = fresh_profile()?;
    if let Err(e) = write_password_prefs(&profile_dir) {
        crate::applog::warn(prefs_failure_is_logged_only(&e));
    }
    let tree = match Tree::new(profile_dir.clone()) {
        Ok(t) => t,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&profile_dir);
            return Err(format!("could not start {}: {e}", which.label()));
        }
    };
    let mut command = Command::new(exe);
    command
        .args(args_with(port, &profile_dir, extra_args))
        // Never the app's own cwd: a browser that inherits the install's
        // `current\` pins it, and the next update cannot rename it. See
        // `leave_install_dir` in lib.rs for the update that taught us this.
        .current_dir(&profile_dir);
    // Started suspended, put in its job, and only then let run: Edge
    // starts its crashpad handler within milliseconds, and a process it
    // starts before the browser is in the job would be outside it.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    let spawned = command.spawn();
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&profile_dir);
            return Err(format!("could not start {}: {e}", which.label()));
        }
    };
    #[cfg(windows)]
    if let Err(e) = tree.adopt_suspended(&child) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&profile_dir);
        return Err(format!("could not start {}: {e}", which.label()));
    }

    let tree = Arc::new(tree);
    tree::register(&tree);
    Ok(LaunchedBrowser { child, port, profile_dir, tree, pid_gone_noted: false })
}
