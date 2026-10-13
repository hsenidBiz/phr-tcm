//! Running one command in the clone: `npx playwright …` or `npm run users …`,
//! and nothing else.
//!
//! - **What runs.** Only `npx` and `npm`, found on PATH, and only as:
//!   - `npx playwright …`, with `--no` so npx never downloads a package the
//!     repo does not have. A config, tsconfig or reporter file Playwright
//!     would load must be inside the clone (`confine_playwright_args`).
//!   - `npm run users -- …`, exactly: nothing between `users` and `--`,
//!     where npm would read an option as its own config (a script shell, a
//!     prefix, node options). The app adds `--silent` itself, so npm prints
//!     no `> node … <args>` banner that would carry the arguments into the
//!     output.
//!
//!   The clone must be the repo and have its `node_modules`.
//! - **No shell of the app's own.** On Windows `npx` and `npm` are `.cmd`
//!   shims, which only `cmd.exe` can run, and `cmd.exe` would read `&`,
//!   `^`, `%` and `"` in an argument. So a shim is never run: the
//!   `node.exe` beside it runs npm's own `npx-cli.js` or `npm-cli.js`
//!   directly, which is what the shim does, and every argument reaches
//!   Node as it was given. Where that layout is not found, nothing is
//!   started. npx then starts Playwright without a shell. `npm run` is
//!   different: npm runs the `users` script through `cmd.exe` itself, with
//!   its own escaping of the arguments after `--`.
//! - **One job.** Each run starts suspended inside a Windows job of its own
//!   (`browser::tree`'s job) and every process it starts lands in the same
//!   job. Stop, and the app exiting (`stop_all`, from
//!   `close_autorun_browsers`), end the whole job; so does the run's own
//!   process ending, so nothing it left behind outlives it.
//! - **One at a time.** A second start is refused with a sentence naming
//!   the running one.
//! - **Output.** stdout and stderr stream line by line, ANSI escapes
//!   stripped and each line cut at `LINE_CAP`, as `RunEvent::Output`;
//!   `RunEvent::Done` comes after the last line, and always comes: the
//!   output is waited for at most `OUTPUT_GRACE` once the run has ended.
//!   The arguments are never logged: they can hold a password.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A run's id, unique for the life of the app.
pub type RunId = u32;

/// What a run reports, in order: its output lines, then `Done` once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// One line of stdout or stderr, ANSI escapes stripped.
    Output { run: RunId, line: String },
    /// The run ended. `code` is its exit code; `None` when it was stopped,
    /// or ended without one.
    Done { run: RunId, code: Option<i32> },
}

/// Where a run's events go: the app emits them to the webview, tests
/// collect them.
pub type Sink = Arc<dyn Fn(RunEvent) + Send + Sync>;

/// The refusal for any program or command other than the two allowed.
pub const ONLY_NPX_AND_NPM: &str = "Only npx playwright and npm run users can be run in the repo.";
/// The refusal for a clone without `node_modules`.
pub const NO_NODE_MODULES: &str =
    "The clone has no node_modules folder, so nothing was started. Run npm install in the clone, then try again.";
/// The refusal for a Playwright config, tsconfig or reporter file outside
/// the clone, or a short option form that could hide one.
pub const OUTSIDE_THE_CLONE: &str =
    "A Playwright config, tsconfig or reporter file must be a file inside the clone, so nothing was started.";
/// The longest output line passed on; the rest of a longer one is cut.
pub const LINE_CAP: usize = 64 * 1024;
/// What ends a line cut at `LINE_CAP`.
pub const CUT_MARK: &str = " [line cut: over 64 KiB]";
/// How long the output is waited for once the run has ended.
pub const OUTPUT_GRACE: Duration = Duration::from_secs(5);
/// The refusal when npx or npm cannot be found.
pub const NODE_NOT_FOUND: &str =
    "Node.js (npx and npm) was not found on PATH, so nothing was started. Install Node.js, then restart the app.";
/// The refusal when the run cannot be started.
pub const COULD_NOT_START: &str = "The command could not be started. Settings → Logs has the details.";

/// The programs a repo command can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Npx,
    Npm,
}

impl Program {
    /// `npx` or `npm`, exactly; anything else is refused.
    pub fn parse(name: &str) -> Result<Program, String> {
        match name {
            "npx" => Ok(Program::Npx),
            "npm" => Ok(Program::Npm),
            _ => Err(ONLY_NPX_AND_NPM.to_string()),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Program::Npx => "npx",
            Program::Npm => "npm",
        }
    }

    /// npm's own script the shim runs, under `node_modules/npm/bin`.
    fn cli(self) -> &'static str {
        match self {
            Program::Npx => "npx-cli.js",
            Program::Npm => "npm-cli.js",
        }
    }
}

/// How a program is started: an executable, and the arguments that go
/// before the caller's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub exe: PathBuf,
    pub lead: Vec<OsString>,
}

/// Find `program` on `path` (a PATH value), trying `pathext`'s extensions
/// in order in each folder on Windows. An `.exe` or `.com` is run as it is.
/// A `.cmd` or `.bat` shim never is: the `node.exe` beside it runs npm's
/// `bin/<program>-cli.js` from the `node_modules/npm` beside it, and a shim
/// without that layout is passed over. Only looks: nothing is run.
pub fn resolve(program: Program, path: Option<&OsStr>, pathext: Option<&OsStr>) -> Option<Resolved> {
    let path = path?;
    let name = program.name();
    for dir in std::env::split_paths(path) {
        if !cfg!(windows) {
            let f = dir.join(name);
            if f.is_file() {
                return Some(Resolved { exe: f, lead: vec![] });
            }
            continue;
        }
        let exts = pathext
            .map(|p| p.to_string_lossy().to_string())
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        for ext in exts.split(';').map(str::trim).filter(|e| !e.is_empty()) {
            let f = dir.join(format!("{name}{ext}"));
            if !f.is_file() {
                continue;
            }
            match ext.to_ascii_lowercase().as_str() {
                ".exe" | ".com" => return Some(Resolved { exe: f, lead: vec![] }),
                ".cmd" | ".bat" => {
                    let node = dir.join("node.exe");
                    let cli = dir.join("node_modules").join("npm").join("bin").join(program.cli());
                    if node.is_file() && cli.is_file() {
                        return Some(Resolved { exe: node, lead: vec![cli.into_os_string()] });
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// `resolve` for this process's PATH and PATHEXT.
pub fn resolve_here(program: Program) -> Option<Resolved> {
    resolve(program, std::env::var_os("PATH").as_deref(), std::env::var_os("PATHEXT").as_deref())
}

/// One command, checked and ready to start.
#[derive(Debug, Clone)]
pub struct Launch {
    exe: PathBuf,
    args: Vec<OsString>,
    cwd: PathBuf,
    label: String,
}

impl Launch {
    /// A repo command: `npx playwright …` or `npm run users -- …` in the
    /// clone at `clone`. Refused for any other program or command, a
    /// Playwright file outside the clone, a folder that is not the repo, a
    /// clone without `node_modules`, or when `resolve` finds no way to run
    /// the program.
    pub fn repo(
        clone: &Path,
        program: &str,
        args: &[String],
        resolve: impl FnOnce(Program) -> Option<Resolved>,
    ) -> Result<Launch, String> {
        let program = Program::parse(program)?;
        let allowed = match program {
            Program::Npx => args.first().is_some_and(|a| a == "playwright"),
            // Exactly `run users --`: an option before `--` is npm's config.
            Program::Npm => args.len() >= 3 && args[0] == "run" && args[1] == "users" && args[2] == "--",
        };
        if !allowed {
            return Err(ONLY_NPX_AND_NPM.to_string());
        }
        if clone.as_os_str().is_empty() {
            return Err(crate::pw_export::clone::NOT_A_CLONE.to_string());
        }
        crate::pw_export::clone::open(clone)?;
        if !clone.join("node_modules").is_dir() {
            return Err(NO_NODE_MODULES.to_string());
        }
        if program == Program::Npx {
            confine_playwright_args(clone, &args[1..])?;
        }
        let resolved = resolve(program).ok_or_else(|| NODE_NOT_FOUND.to_string())?;

        let mut all = resolved.lead;
        match program {
            Program::Npx => {
                // Never download a package the repo does not have.
                all.push("--no".into());
                all.extend(args.iter().map(OsString::from));
            }
            Program::Npm => {
                // `--silent`: no `> node … <args>` banner in the output.
                all.extend(["run", "--silent", "users"].map(OsString::from));
                all.extend(args[2..].iter().map(OsString::from));
            }
        }
        Ok(Launch { exe: resolved.exe, args: all, cwd: clone.to_path_buf(), label: label(program, args) })
    }

    /// Any program, for the tests of the job and the output: never reached
    /// from a command, which only builds `Launch::repo`.
    #[doc(hidden)]
    pub fn test_program(exe: PathBuf, args: Vec<OsString>, cwd: PathBuf, label: &str) -> Launch {
        Launch { exe, args, cwd, label: label.to_string() }
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Every argument the executable is given, in order.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// How the run is named to the person and in the log: the program and
    /// at most its first two words (`npx playwright test`,
    /// `npm run users`). Never an option or a value.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// Reporters Playwright has built in, which load no file.
const BUILT_IN_REPORTERS: &[&str] = &["list", "line", "dot", "json", "junit", "html", "blob", "github", "null"];

/// Refuse Playwright arguments (those after `playwright`) that would load a
/// file from outside the clone: `-c`/`--config` and `--tsconfig`, as
/// `--name=value` or with the value as the next argument, and `--reporter`
/// values that are not built in. Such a file must exist inside the clone.
/// A single-dash argument longer than two characters (`-cfile`, `-xc`) is
/// refused: it could hide `-c`. Nothing after `--` is looked at; Playwright
/// reads that as filters.
pub fn confine_playwright_args(clone: &Path, args: &[String]) -> Result<(), String> {
    let refuse = || OUTSIDE_THE_CLONE.to_string();
    let root = clone.canonicalize().map_err(|_| refuse())?;
    let inside = |value: &str| {
        let p = Path::new(value);
        let p = if p.is_absolute() { p.to_path_buf() } else { clone.join(p) };
        !value.is_empty() && p.canonicalize().is_ok_and(|c| c.starts_with(&root) && c.is_file())
    };
    let reporters = |value: &str| value.split(',').all(|r| BUILT_IN_REPORTERS.contains(&r.trim()) || inside(r.trim()));
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            break;
        }
        let (name, value) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v)),
            _ => (a, None),
        };
        if name.starts_with('-') && !name.starts_with("--") && name.chars().count() > 2 {
            return Err(refuse());
        }
        let is_reporter = match name {
            "-c" | "--config" | "--tsconfig" => false,
            "--reporter" => true,
            _ => {
                i += 1;
                continue;
            }
        };
        let (v, used) = match value {
            Some(v) => (v, 1),
            None => (args.get(i + 1).ok_or_else(refuse)?.as_str(), 2),
        };
        let ok = if is_reporter { reporters(v) } else { inside(v) };
        if !ok {
            return Err(refuse());
        }
        i += used;
    }
    Ok(())
}

fn label(program: Program, args: &[String]) -> String {
    let words = args.iter().take(2).take_while(|a| !a.starts_with('-') && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    std::iter::once(program.name()).chain(words.map(String::as_str)).collect::<Vec<_>>().join(" ")
}

/// Read `reader` to its end, passing each line to `emit` through
/// `clean_line`. A line longer than `LINE_CAP` bytes is cut there and ends
/// in `CUT_MARK`; the rest of it is dropped. A last line without a line
/// ending is passed on too.
pub fn stream_lines(reader: impl Read, mut emit: impl FnMut(String)) {
    let finish = |buf: &[u8], cut: bool| {
        let mut s = clean_line(buf);
        if cut {
            // A character the cut split in two.
            s.truncate(s.trim_end_matches('\u{fffd}').len());
            s.push_str(CUT_MARK);
        }
        s
    };
    let mut r = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut cut = false;
    loop {
        let avail = match r.fill_buf() {
            Ok(a) => a,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if avail.is_empty() {
            if !buf.is_empty() || cut {
                emit(finish(&buf, cut));
            }
            break;
        }
        let (take, content, ends) = match avail.iter().position(|&b| b == b'\n') {
            Some(at) => (at + 1, at, true),
            None => (avail.len(), avail.len(), false),
        };
        if !cut {
            let room = LINE_CAP - buf.len();
            buf.extend_from_slice(&avail[..content.min(room)]);
            cut = content > room;
        }
        r.consume(take);
        if ends {
            emit(finish(&buf, cut));
            buf.clear();
            cut = false;
        }
    }
}

/// A line of output as the person sees it: ANSI escapes stripped, the line
/// ending gone, and only what a terminal would show after the last
/// carriage return (a progress line redrawn in place).
pub fn clean_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim_end_matches(['\n', '\r']);
    let stripped = strip_ansi(text);
    let last = stripped.rsplit('\r').next().unwrap_or("");
    last.chars().filter(|c| *c == '\t' || !c.is_control()).collect()
}

/// `s` without its ANSI escape sequences: CSI (`ESC [ … final`, colours and
/// cursor moves), OSC (`ESC ] … BEL` or `ESC ] … ESC \`, titles and links),
/// and two-character `ESC x` sequences.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        if it.peek() == Some(&'\\') {
                            it.next();
                        }
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The run going now.
struct Current {
    id: RunId,
    label: String,
    state: Mutex<RunState>,
    #[cfg(windows)]
    job: crate::browser::tree::job::Job,
    #[cfg(not(windows))]
    child: Arc<Mutex<std::process::Child>>,
}

/// Whether the run's process was seen to end, and whether Stop came first.
/// One lock, so a Stop after the process ended (but before `Done`) leaves
/// its exit code alone.
#[derive(Default)]
struct RunState {
    finished: bool,
    stopped: bool,
}

impl Current {
    fn state(&self) -> std::sync::MutexGuard<'_, RunState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// End the whole run, unless its process has already ended.
    fn end(&self) {
        let mut state = self.state();
        if state.finished {
            return;
        }
        state.stopped = true;
        #[cfg(windows)]
        {
            if !self.job.terminate_all() {
                crate::applog::warn(format!("pw-repo: run {} could not be ended: {}", self.id, std::io::Error::last_os_error()));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = self.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }
}

fn slot() -> std::sync::MutexGuard<'static, Option<Arc<Current>>> {
    static SLOT: Mutex<Option<Arc<Current>>> = Mutex::new(None);
    SLOT.lock().unwrap_or_else(|e| e.into_inner())
}

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

/// The run going now, if any: its id and its label.
pub fn running() -> Option<(RunId, String)> {
    slot().as_ref().map(|c| (c.id, c.label.clone()))
}

/// The refusal a start meets while `label` runs.
pub fn busy_sentence(label: &str) -> String {
    format!("{label} is still running. Stop it, or wait for it to finish, before starting another.")
}

/// Start `launch` in a job of its own, streaming its output to `sink`.
/// Refused while another run is going.
pub fn start(launch: Launch, sink: Sink) -> Result<RunId, String> {
    start_split(launch, sink.clone(), sink)
}

/// `start`, with stdout's lines and `Done` going to `out` and stderr's lines
/// to `err`. The runner reads a listing's JSON from stdout this way while
/// Playwright's warnings on stderr still reach the output panel.
pub fn start_split(launch: Launch, out: Sink, err: Sink) -> Result<RunId, String> {
    let sink = out;
    let mut current = slot();
    if let Some(c) = current.as_ref() {
        return Err(busy_sentence(&c.label));
    }

    #[cfg(windows)]
    let job = crate::browser::tree::job::Job::new().map_err(|e| {
        crate::applog::warn(format!("pw-repo: {e}"));
        COULD_NOT_START.to_string()
    })?;

    let mut command = Command::new(&launch.exe);
    command
        .args(&launch.args)
        .current_dir(&launch.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Suspended until it is in its job, so nothing it starts is outside.
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED | CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| {
        crate::applog::warn(format!("pw-repo: {} could not be started: {e}", launch.label));
        COULD_NOT_START.to_string()
    })?;
    #[cfg(windows)]
    if let Err(e) = job.adopt_suspended(&child) {
        let _ = child.kill();
        let _ = child.wait();
        crate::applog::warn(format!("pw-repo: {} could not be put in its job: {e}", launch.label));
        return Err(COULD_NOT_START.to_string());
    }

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst) + 1;
    // Output passes only while this is set: a reader still going once the
    // grace after the end is over says nothing after `Done`.
    let open = Arc::new(AtomicBool::new(true));
    let (read_done, readers_done) = std::sync::mpsc::channel::<()>();
    let streams: Vec<(Box<dyn Read + Send>, Sink)> = [
        child.stdout.take().map(|o| (Box::new(o) as Box<dyn Read + Send>, sink.clone())),
        child.stderr.take().map(|e| (Box::new(e) as Box<dyn Read + Send>, err.clone())),
    ]
    .into_iter()
    .flatten()
    .collect();
    let readers = streams.len();
    for (stream, sink) in streams {
        let (open, read_done) = (open.clone(), read_done.clone());
        std::thread::spawn(move || {
            stream_lines(stream, |line| {
                if open.load(Ordering::SeqCst) {
                    sink(RunEvent::Output { run: id, line });
                }
            });
            let _ = read_done.send(());
        });
    }
    drop(read_done);

    let child = Arc::new(Mutex::new(child));
    let run = Arc::new(Current {
        id,
        label: launch.label.clone(),
        state: Mutex::new(RunState::default()),
        #[cfg(windows)]
        job,
        #[cfg(not(windows))]
        child: child.clone(),
    });
    *current = Some(run.clone());
    drop(current);
    crate::applog::info(format!("pw-repo: run {id} started: {}", launch.label));

    std::thread::spawn(move || {
        let status = loop {
            let polled = child.lock().unwrap_or_else(|e| e.into_inner()).try_wait();
            match polled {
                Ok(Some(s)) => break Some(s),
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break None,
            }
        };
        let stopped = {
            let mut state = run.state();
            state.finished = true;
            state.stopped
        };
        // Whatever it left running goes with it, which also closes the
        // output pipes those processes held.
        #[cfg(windows)]
        run.job.terminate_all();
        // The rest of the output, for a while: Done always comes.
        let deadline = std::time::Instant::now() + OUTPUT_GRACE;
        for _ in 0..readers {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if readers_done.recv_timeout(left).is_err() {
                crate::applog::warn(format!("pw-repo: run {id}'s output was still open {} s after it ended", OUTPUT_GRACE.as_secs()));
                break;
            }
        }
        open.store(false, Ordering::SeqCst);
        let code = if stopped { None } else { status.and_then(|s| s.code()) };
        {
            let mut current = slot();
            if current.as_ref().is_some_and(|c| c.id == id) {
                *current = None;
            }
        }
        crate::applog::info(match (stopped, code) {
            (true, _) => format!("pw-repo: run {id} stopped"),
            (false, Some(c)) => format!("pw-repo: run {id} ended with code {c}"),
            (false, None) => format!("pw-repo: run {id} ended without an exit code"),
        });
        sink(RunEvent::Done { run: id, code });
    });
    Ok(id)
}

/// The refusal `start_repo` gives `npm`: its output would stream to the
/// webview, and `npm run users -- list --json` prints every password.
pub const ACCOUNTS_ONLY: &str = "npm run users runs only from the Accounts section, which keeps passwords out of the output.";

/// The command's body: refused where the tab is not offered, then
/// `Launch::repo` with npx found on PATH, then `start`. `npm run users` is
/// refused here: its output streams as it is, and the accounts commands
/// (`users`) are the only way to it.
pub fn start_repo(offered: bool, clone: &str, program: &str, args: &[String], sink: Sink) -> Result<RunId, String> {
    super::ensure_offered(offered)?;
    if program == Program::Npm.name() {
        return Err(ACCOUNTS_ONLY.to_string());
    }
    let launch = Launch::repo(Path::new(clone.trim()), program, args, resolve_here)?;
    start(launch, sink)
}

/// End run `run`'s whole job. A run already over is not an error.
pub fn stop(run: RunId) -> Result<(), String> {
    let current = slot().as_ref().cloned();
    if let Some(c) = current.filter(|c| c.id == run) {
        crate::applog::info(format!("pw-repo: run {} stop asked", c.id));
        c.end();
    }
    Ok(())
}

/// End whatever run is going, as the app exits. Returns whether one was.
/// Does not wait: the job ends at once, and its `Done` follows.
pub fn stop_all() -> bool {
    let current = slot().as_ref().cloned();
    match current {
        Some(c) => {
            crate::applog::info(format!("pw-repo: run {} ended as the app exits", c.id));
            c.end();
            true
        }
        None => false,
    }
}

/// The pids in the running run's job now, for the tests that check a Stop
/// ends every one. Empty with no run, and off Windows.
#[doc(hidden)]
pub fn running_pids() -> Vec<u32> {
    #[cfg(windows)]
    {
        slot().as_ref().and_then(|c| c.job.pids()).unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}
