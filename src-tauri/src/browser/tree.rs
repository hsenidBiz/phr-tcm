//! Every browser the app starts, as a whole process tree.
//!
//! A browser is many processes: the one the app spawned, and the gpu,
//! utility, renderer and crashpad processes it starts. Edge can also
//! replace its first process with another one that keeps the same
//! profile. Tracking only the pid the app spawned lost those browsers:
//! the pid ended, the app took the browser for closed, killed nothing,
//! could not delete the profile the browser still held, and started
//! another one (one per case in an unattended run).
//!
//! On Windows each browser therefore starts inside a job object of its
//! own (`Job`). Windows puts every process the browser starts into the
//! same job, and the job cannot be left: no breakaway is allowed.
//!
//! A job is not only the browser, though. A program the person opens from
//! a visible app browser (a downloaded file in Excel, a `mailto:` link in
//! Outlook, a link handed to the default browser, which can start the
//! person's own Edge) is started by the browser and so lands in its job
//! too. So the browser's own processes are told apart by their command
//! line: Chromium gives every process of a browser its
//! `--user-data-dir=<profile>`, and the profile is this tree's throwaway
//! folder (`carries_profile`). The command line is read with
//! `NtQueryInformationProcess(ProcessCommandLineInformation)`, which needs
//! only `PROCESS_QUERY_LIMITED_INFORMATION` and reads no other process's
//! memory. Closing a browser ends only those processes, one by one,
//! through a handle held from the moment each was looked at (so its pid
//! cannot be reused by another program in between), and counts only
//! those; anything else in the job is left running. The job is never
//! ended as a whole. A process that cannot be opened, other than one that
//! has already ended, counts as someone else's.
//!
//! The job kills its processes when its last handle closes, so a browser
//! the app forgets, or an app that crashes, still takes its tree with it.
//! That would also take a program the person opened from the browser, so a
//! watcher (one thread, every `WATCH_EVERY`) looks at every live tree and,
//! the first time it finds a process without the profile in one, takes
//! kill-on-close off that job and logs the program's file name once. The
//! cost, accepted: after that, a crash of the app can leave that browser's
//! own processes running; the next start's sweep removes its profile once
//! they are gone.
//!
//! Edge restarting itself (after an update, or "Restart" in its settings)
//! asks to break away from its job. That is refused here, so such a
//! browser simply exits; it is then judged dead and the app starts a new
//! one, which is the safe outcome.
//!
//! Every live tree is kept in one registry (`register`), and the app's
//! exit ends them all (`end_all`), including one an unattended run still
//! holds. `sweep_leftover_profiles` removes, at start, the profile folders
//! a browser of an older version left behind.
//!
//! Off Windows there is no job: the tree is only its profile folder, and
//! closing a browser kills the one process the app spawned, as before.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// How long a closed browser's processes get to be gone. A headful tree
/// takes a few seconds to unwind after its first process goes.
pub const END_WITHIN: Duration = Duration::from_secs(5);

/// How often the watcher looks for programs the person opened from a
/// browser.
pub const WATCH_EVERY: Duration = Duration::from_secs(2);

/// The start of every profile folder a launched browser gets. The full
/// name is this and the DevTools port, digits only.
pub const PROFILE_PREFIX: &str = "tcm-autorun-";

/// Something the app started that can be ended as a whole: a browser's
/// process tree and its profile folder. The registry holds these, so the
/// tests can stand in a fake for a real browser.
pub trait Ends: Send + Sync {
    /// Ask every process of the browser to end, without waiting.
    fn terminate(&self);
    /// End every process of the browser, wait until none is left (up to
    /// `END_WITHIN`), then remove the profile folder. True when no process
    /// of the browser is left.
    fn end(&self) -> bool;
    /// The profile folder it uses.
    fn profile_dir(&self) -> &Path;
    /// Called by the watcher every `WATCH_EVERY`.
    fn watch(&self) {}
}

/// Every tree the app started that is still alive. Weak, so a tree that
/// was dropped (and so killed) is never kept alive by the registry.
static LIVE: Mutex<Vec<Weak<dyn Ends>>> = Mutex::new(Vec::new());

fn live() -> std::sync::MutexGuard<'static, Vec<Weak<dyn Ends>>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keep `tree` in the registry until it is dropped. The first one starts
/// the watcher.
pub fn register<E: Ends + 'static>(tree: &Arc<E>) {
    let tree: Arc<dyn Ends> = tree.clone();
    {
        let mut live = live();
        live.retain(|w| w.strong_count() > 0);
        live.push(Arc::downgrade(&tree));
    }
    static WATCHER: std::sync::Once = std::sync::Once::new();
    WATCHER.call_once(|| {
        let spawned = std::thread::Builder::new().name("browser-tree-watch".into()).spawn(|| loop {
            std::thread::sleep(WATCH_EVERY);
            for t in registered() {
                t.watch();
            }
        });
        if let Err(e) = spawned {
            crate::applog::warn(format!(
                "auto-run: the watcher for programs opened from a browser could not start ({e}) - a browser's job keeps kill-on-close until it is closed"
            ));
        }
    });
}

fn registered() -> Vec<Arc<dyn Ends>> {
    let mut live = live();
    live.retain(|w| w.strong_count() > 0);
    live.iter().filter_map(Weak::upgrade).collect()
}

/// End every registered tree: all are asked to end first, then each is
/// waited for and its profile removed. Returns how many were still left
/// running. Called as the app exits.
pub fn end_all() -> usize {
    let trees = registered();
    for t in &trees {
        t.terminate();
    }
    trees.iter().filter(|t| !t.end()).count()
}

/// The profile folders of the trees alive now.
pub fn held_profiles() -> Vec<PathBuf> {
    registered().iter().map(|t| t.profile_dir().to_path_buf()).collect()
}

/// Run blocking work (closing a browser waits for its processes) without
/// stalling the async runtime's other tasks: moved off the worker on a
/// multi-threaded runtime, run as it is anywhere else (a test's
/// current-thread runtime, or no runtime at all).
pub fn blocking<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
        _ => f(),
    }
}

fn normalise(path: &str) -> String {
    path.trim().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// The values of every `<switch>=` in a command line: `--x=v`, `--x="v"`
/// and `"--x=v"` (a whole argument quoted, as a path with spaces is).
fn switch_values<'a>(command_line: &'a str, switch: &str) -> Vec<&'a str> {
    let lower = command_line.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find(switch).map(|i| from + i) {
        let whole_quoted = at > 0 && command_line.as_bytes()[at - 1] == b'"';
        let start = at + switch.len();
        let rest = &command_line[start..];
        let value = if let Some(inner) = rest.strip_prefix('"') {
            inner.split('"').next().unwrap_or("")
        } else if whole_quoted {
            rest.split('"').next().unwrap_or("")
        } else {
            rest.split(|c: char| c.is_whitespace() || c == '"').next().unwrap_or("")
        };
        out.push(value);
        from = start;
    }
    out
}

/// Is this a process of the browser on `profile`? Its command line names
/// that folder as its `--user-data-dir` (Chromium copies the switch to
/// every process it starts), or keeps its crash database inside it.
/// Compared case-insensitively on the normalised path, never as a prefix
/// of another folder (`tcm-autorun-1` is not `tcm-autorun-12`).
pub fn carries_profile(command_line: &str, profile: &Path) -> bool {
    let profile = normalise(&profile.to_string_lossy());
    if profile.is_empty() {
        return false;
    }
    let inside = format!("{profile}\\");
    switch_values(command_line, "--user-data-dir=").iter().any(|v| normalise(v) == profile)
        || switch_values(command_line, "--database=").iter().any(|v| normalise(v).starts_with(&inside))
}

/// Is this exactly a launched browser's profile folder name:
/// `tcm-autorun-` and digits, nothing else? The test suite's own
/// `tcm-autorun-bridge-*` and `-discovery-*` folders are not.
pub fn is_profile_name(name: &str) -> bool {
    name.strip_prefix(PROFILE_PREFIX).is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Remove, under `temp`, every folder named exactly like a launched
/// browser's profile that no live tree of this app holds and that nothing
/// has open. A folder in use is left as it is: it is renamed aside first,
/// which Windows refuses while a file in it is open, so a browser still
/// running on it never loses half its files. A folder an earlier sweep
/// left aside (`<name>.sweep`) is removed too. Never looks at processes.
/// Returns the names removed.
pub fn sweep_leftover_profiles(temp: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(temp) else { return Vec::new() };
    let dirs: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    let mut removed = Vec::new();
    // Leftovers first: one still in the way would stop its profile's
    // folder from being moved aside.
    for (name, path) in &dirs {
        if name.strip_suffix(".sweep").is_some_and(is_profile_name) && std::fs::remove_dir_all(path).is_ok() {
            removed.push(name.clone());
        }
    }
    for (name, path) in &dirs {
        if !is_profile_name(name) {
            continue;
        }
        // Asked per folder: a browser started while the sweep runs is
        // left alone too.
        if held_profiles().iter().any(|h| h == path) {
            continue;
        }
        let aside = temp.join(format!("{name}.sweep"));
        if std::fs::rename(path, &aside).is_err() {
            continue;
        }
        if std::fs::remove_dir_all(&aside).is_ok() {
            removed.push(name.clone());
        } else {
            let _ = std::fs::rename(&aside, path);
        }
    }
    removed
}

/// Remove a closed browser's profile folder. Windows can release a
/// process's file handles a moment after the process is gone, so a
/// "being used by another process" (os error 32) is tried again a few
/// times. A folder already gone is fine.
pub fn remove_profile(dir: &Path) -> Result<(), std::io::Error> {
    let mut last = None;
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(200));
        }
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("the profile could not be removed")))
}

fn folder_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// One launched browser's processes and profile.
pub struct Tree {
    #[cfg(windows)]
    job: job::Job,
    profile_dir: PathBuf,
    /// A process without the profile was found in the job: kill-on-close
    /// is off, and the job is never ended as a whole again.
    #[cfg(windows)]
    foreign_seen: std::sync::atomic::AtomicBool,
}

/// A process in a browser's job: whether it is the browser's own
/// (carries the profile), its exe file name, and a handle to it held
/// since it was looked at, which pins its pid. `None` for a process that
/// could not be opened: never the browser's, never ended.
#[cfg(windows)]
struct Member {
    ours: bool,
    image: String,
    process: Option<job::Process>,
}

#[cfg(windows)]
impl Member {
    /// End it, through the held handle. Only the browser's own are.
    fn kill(&self, job: &job::Job) {
        if let (true, Some(p)) = (self.ours, self.process.as_ref()) {
            job.kill(p);
        }
    }
}

impl Tree {
    /// A tree with a new, empty job (Windows), for a browser about to be
    /// started on `profile_dir`.
    pub fn new(profile_dir: PathBuf) -> Result<Tree, String> {
        Ok(Tree {
            #[cfg(windows)]
            job: job::Job::new()?,
            profile_dir,
            #[cfg(windows)]
            foreign_seen: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Put a just-spawned, still suspended process into this tree's job,
    /// then let it run. Every process it starts joins the job with it.
    #[cfg(windows)]
    pub fn adopt_suspended(&self, child: &std::process::Child) -> Result<(), String> {
        self.job.adopt_suspended(child)
    }

    /// Put a running process into this tree's job, as a program the
    /// person opened from the browser would be. Only the live tests use
    /// this.
    #[cfg(windows)]
    #[doc(hidden)]
    pub fn put_in_job(&self, child: &std::process::Child) -> Result<(), String> {
        self.job.assign(child)
    }

    /// Does the job still kill what is in it when its last handle closes?
    /// `None` off Windows, or if Windows will not say.
    pub fn kills_on_close(&self) -> Option<bool> {
        #[cfg(windows)]
        {
            self.job.kills_on_close()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    #[cfg(windows)]
    fn members(&self) -> Option<Vec<Member>> {
        let pids = self.job.pids()?;
        Some(
            pids.into_iter()
                .filter_map(|pid| match job::inspect(pid) {
                    job::Inspected::Gone => None,
                    job::Inspected::Unopenable => Some(Member { ours: false, image: String::new(), process: None }),
                    job::Inspected::Seen { process, command_line, image } => {
                        let ours = command_line.as_deref().is_some_and(|c| carries_profile(c, &self.profile_dir));
                        Some(Member { ours, image, process: Some(process) })
                    }
                })
                .collect(),
        )
    }

    /// The first time a process without the profile is seen in the job:
    /// kill-on-close comes off, so a crash never ends it, and its file
    /// name is logged.
    #[cfg(windows)]
    fn note_foreign(&self, members: &[Member]) {
        use std::sync::atomic::Ordering;
        let Some(stranger) = members.iter().find(|m| !m.ours) else { return };
        if self.foreign_seen.swap(true, Ordering::SeqCst) {
            return;
        }
        self.job.clear_kill_on_close();
        let image = if stranger.image.is_empty() { "a program" } else { stranger.image.as_str() };
        crate::applog::warn(format!(
            "auto-run: a program opened from the browser on profile {} runs beside it and is left running when the browser closes: {image}",
            folder_name(&self.profile_dir)
        ));
    }

    /// How many of the browser's own processes (those carrying its
    /// profile) run now. `None` off Windows, where there is no job to ask,
    /// or if Windows will not list the job.
    pub fn processes(&self) -> Option<u32> {
        #[cfg(windows)]
        {
            self.members().map(|m| m.iter().filter(|m| m.ours).count() as u32)
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Is this process in the tree's job? Always false off Windows.
    pub fn holds(&self, pid: u32) -> bool {
        #[cfg(windows)]
        {
            self.job.holds(pid)
        }
        #[cfg(not(windows))]
        {
            let _ = pid;
            false
        }
    }

    /// Wait until none of the browser's own processes is left, up to
    /// `within`, ending again any it started meanwhile. On Windows a job
    /// that cannot be listed counts as not empty: a safety check fails
    /// closed.
    fn wait_empty(&self, within: Duration) -> bool {
        let began = Instant::now();
        let mut asked = Instant::now();
        loop {
            match self.processes() {
                Some(0) => return true,
                #[cfg(not(windows))]
                None => return true,
                _ if began.elapsed() >= within => return false,
                _ => {
                    if asked.elapsed() >= Duration::from_millis(500) {
                        self.terminate();
                        asked = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

impl Ends for Tree {
    /// Each of the browser's own processes, through its held handle, and
    /// nothing else. Never the job as a whole: a program the person opened
    /// could join it between a look and a whole-job kill. One the browser
    /// starts meanwhile is caught by `wait_empty` asking again.
    fn terminate(&self) {
        #[cfg(windows)]
        {
            let Some(members) = self.members() else { return };
            self.note_foreign(&members);
            for m in &members {
                m.kill(&self.job);
            }
        }
    }

    fn end(&self) -> bool {
        self.terminate();
        if !self.wait_empty(END_WITHIN) {
            crate::applog::warn(format!(
                "auto-run: the browser on profile {} still had {} processes {} s after it was ended",
                folder_name(&self.profile_dir),
                self.processes().map_or("an unknown number of".to_string(), |n| n.to_string()),
                END_WITHIN.as_secs()
            ));
            return false;
        }
        if let Err(e) = remove_profile(&self.profile_dir) {
            crate::applog::warn(format!("auto-run: could not remove browser profile {}: {e}", folder_name(&self.profile_dir)));
        }
        true
    }

    fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }

    fn watch(&self) {
        #[cfg(windows)]
        if let Some(members) = self.members() {
            self.note_foreign(&members);
        }
    }
}

/// A dropped tree looks at its job once more: a program the person opened
/// since the watcher's last round takes kill-on-close off first, so the
/// handle closing cannot end it. Then the browser's own processes are
/// ended. With nothing else in the job, the handle closing ends the rest.
#[cfg(windows)]
impl Drop for Tree {
    fn drop(&mut self) {
        if let Some(members) = self.members() {
            self.note_foreign(&members);
            for m in &members {
                m.kill(&self.job);
            }
        }
    }
}

#[cfg(windows)]
mod job {
    //! The Windows job object a browser runs in.

    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE, UNICODE_STRING,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectBasicProcessIdList,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, OpenThread, QueryFullProcessImageNameW, ResumeThread, TerminateProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, THREAD_SUSPEND_RESUME,
    };

    /// An owned job handle. Closing it (dropping this) kills every
    /// process still in the job, unless kill-on-close was taken off.
    pub struct Job(OwnedHandle);

    fn os_error(what: &str) -> String {
        format!("{what}: {}", std::io::Error::last_os_error())
    }

    /// A process handle, closed when dropped. Held, it keeps the pid
    /// from being given to another process.
    pub struct Process(HANDLE);

    // SAFETY: a process handle may be used and closed from any thread.
    unsafe impl Send for Process {}
    unsafe impl Sync for Process {}

    impl Process {
        fn open(access: u32, pid: u32) -> Option<Process> {
            // SAFETY: the handle is checked before it is owned.
            let h = unsafe { OpenProcess(access, 0, pid) };
            (!h.is_null()).then_some(Process(h))
        }

        fn ended(&self) -> bool {
            let mut code = 0u32;
            // SAFETY: a valid process handle; `code` lives for the call.
            unsafe { GetExitCodeProcess(self.0, &mut code) == 0 || code != STILL_ACTIVE as u32 }
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: opened by `Process::open`, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// What looking at a process in the job found.
    pub enum Inspected {
        /// It has ended, and is only still listed: a browser process on its
        /// way out must never be taken for a program the person opened.
        Gone,
        /// It could not be opened, for a reason other than having ended:
        /// someone else's, never ended by the app.
        Unopenable,
        /// Opened, with the rights to end it: its command line (if Windows
        /// lets it be read) and its exe file name, never its path.
        Seen { process: Process, command_line: Option<String>, image: String },
    }

    /// Look at a process, opening it once with the rights both to read and
    /// to end it. That handle is what it is ended through, if it is.
    pub fn inspect(pid: u32) -> Inspected {
        let Some(p) = Process::open(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, pid) else {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(code) if code == ERROR_INVALID_PARAMETER as i32 => Inspected::Gone,
                _ => Inspected::Unopenable,
            };
        };
        if p.ended() {
            return Inspected::Gone;
        }
        let command_line = command_line(&p);
        // A read that failed because it ended meanwhile is not a stranger.
        if command_line.is_none() && p.ended() {
            return Inspected::Gone;
        }
        let image = image_name(&p);
        Inspected::Seen { process: p, command_line, image }
    }

    fn command_line(p: &Process) -> Option<String> {
        let mut size: u32 = 32 * 1024;
        for _ in 0..3 {
            // u64s, so the UNICODE_STRING at the front is aligned.
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let mut needed = 0u32;
            // SAFETY: the buffer is `size` bytes and lives for the call.
            let status = unsafe {
                NtQueryInformationProcess(p.0, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), size, &mut needed)
            };
            if status == 0 {
                // SAFETY: on success the buffer starts with a UNICODE_STRING
                // whose text lies inside the same buffer.
                let us = unsafe { &*(buf.as_ptr() as *const UNICODE_STRING) };
                if us.Buffer.is_null() || us.Length == 0 {
                    return Some(String::new());
                }
                let text = unsafe { std::slice::from_raw_parts(us.Buffer, (us.Length / 2) as usize) };
                return Some(String::from_utf16_lossy(text));
            }
            if needed <= size {
                return None;
            }
            size = needed;
        }
        None
    }

    fn image_name(p: &Process) -> String {
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` u16s and lives for the call.
        if unsafe { QueryFullProcessImageNameW(p.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) } == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or("").to_string()
    }

    impl Job {
        /// A new, unnamed job that kills its processes when its last
        /// handle closes and lets none of them break away.
        pub fn new() -> Result<Job, String> {
            // SAFETY: no security attributes and no name; the handle is
            // checked before it is owned.
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(os_error("could not make a job for the browser"));
            }
            // SAFETY: `raw` is a valid handle this function owns.
            let job = Job(unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) });
            if !job.set_limits(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE) {
                return Err(os_error("could not set up the browser's job"));
            }
            Ok(job)
        }

        fn raw(&self) -> HANDLE {
            self.0.as_raw_handle() as HANDLE
        }

        fn set_limits(&self, flags: u32) -> bool {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = flags;
            // SAFETY: the struct is the size passed, and lives for the call.
            unsafe {
                SetInformationJobObject(
                    self.raw(),
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) != 0
            }
        }

        /// Take kill-on-close off: closing the job's last handle then
        /// leaves its processes running. No other limit is set, so none
        /// is lost.
        pub fn clear_kill_on_close(&self) {
            if !self.set_limits(0) {
                crate::applog::warn(format!("auto-run: {}", os_error("could not take kill-on-close off a browser's job")));
            }
        }

        pub fn kills_on_close(&self) -> Option<bool> {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // SAFETY: the struct is the size passed, and lives for the call.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.raw(),
                    JobObjectExtendedLimitInformation,
                    &mut limits as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then_some(limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0)
        }

        /// Assign a process created suspended to this job, then resume
        /// its threads. Assigned before it runs a single instruction, so
        /// no process it starts can be outside the job.
        pub fn adopt_suspended(&self, child: &std::process::Child) -> Result<(), String> {
            self.assign(child)?;
            resume_threads(child.id())
        }

        pub fn assign(&self, child: &std::process::Child) -> Result<(), String> {
            // SAFETY: both handles are valid for the call.
            if unsafe { AssignProcessToJobObject(self.raw(), child.as_raw_handle() as HANDLE) } == 0 {
                return Err(os_error("could not put the browser in its job"));
            }
            Ok(())
        }

        /// Every process in the job now. `None` if Windows will not say.
        pub fn pids(&self) -> Option<Vec<u32>> {
            let header = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
            let mut room = 256usize;
            for _ in 0..4 {
                let bytes = header + room * std::mem::size_of::<usize>();
                let mut buf = vec![0u64; bytes.div_ceil(8)];
                // SAFETY: the buffer is `bytes` long, aligned for the
                // struct, and lives for the call.
                let ok = unsafe {
                    QueryInformationJobObject(
                        self.raw(),
                        JobObjectBasicProcessIdList,
                        buf.as_mut_ptr().cast(),
                        bytes as u32,
                        std::ptr::null_mut(),
                    )
                };
                // SAFETY: the buffer starts with the struct's header.
                let list = unsafe { &*(buf.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };
                let assigned = list.NumberOfAssignedProcesses as usize;
                if ok == 0 && assigned <= room {
                    return None;
                }
                if assigned > room {
                    room = assigned + 32;
                    continue;
                }
                let n = list.NumberOfProcessIdsInList as usize;
                // SAFETY: Windows wrote `n` ids after the header.
                let ids = unsafe {
                    std::slice::from_raw_parts((buf.as_ptr() as *const u8).add(header) as *const usize, n)
                };
                return Some(ids.iter().map(|&id| id as u32).collect());
            }
            None
        }

        /// End one process through the handle held since it was looked at
        /// (so it is the same process), and only while it is in this job.
        pub fn kill(&self, p: &Process) {
            let mut inside = 0;
            // SAFETY: both handles are valid; `inside` lives for the call.
            if unsafe { IsProcessInJob(p.0, self.raw(), &mut inside) } != 0 && inside != 0 {
                // SAFETY: a valid process handle.
                unsafe { TerminateProcess(p.0, 1) };
            }
        }

        /// Is the process with this pid in this job?
        pub fn holds(&self, pid: u32) -> bool {
            let Some(p) = Process::open(PROCESS_QUERY_LIMITED_INFORMATION, pid) else { return false };
            let mut inside = 0;
            // SAFETY: both handles are valid; `inside` lives for the call.
            let ok = unsafe { IsProcessInJob(p.0, self.raw(), &mut inside) };
            ok != 0 && inside != 0
        }
    }

    /// Resume every thread of a process created suspended (it has one).
    fn resume_threads(pid: u32) -> Result<(), String> {
        // SAFETY: the snapshot handle is checked, and closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
            return Err(os_error("could not start the browser"));
        }
        let mut entry = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut resumed = 0;
        // SAFETY: `entry` is sized as the API asks, and lives for each call.
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: the thread handle is checked, and closed below.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // SAFETY: a valid thread handle, closed once.
                    unsafe {
                        if ResumeThread(thread) != u32::MAX {
                            resumed += 1;
                        }
                        CloseHandle(thread);
                    }
                }
            }
            // SAFETY: as above.
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        // SAFETY: opened above, closed once.
        unsafe { CloseHandle(snapshot) };
        if resumed == 0 {
            return Err("could not start the browser: its first thread would not resume".to_string());
        }
        Ok(())
    }
}
