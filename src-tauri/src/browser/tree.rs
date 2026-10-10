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
//! same job, and the job cannot be left: no breakaway is allowed. Ending
//! the job ends the whole tree and nothing else, so the person's own Edge,
//! which is never in one of these jobs, can never be touched. The job is
//! set to kill on close, so a browser whose handle is dropped, or an app
//! that crashes, still takes its tree with it.
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

/// The start of every profile folder a launched browser gets. The full
/// name is this and the DevTools port, digits only.
pub const PROFILE_PREFIX: &str = "tcm-autorun-";

/// Something the app started that can be ended as a whole: a browser's
/// process tree and its profile folder. The registry holds these, so the
/// tests can stand in a fake for a real browser.
pub trait Ends: Send + Sync {
    /// Ask every process to end, without waiting.
    fn terminate(&self);
    /// End every process, wait until none is left (up to `END_WITHIN`),
    /// then remove the profile folder. True when no process is left.
    fn end(&self) -> bool;
    /// The profile folder it uses.
    fn profile_dir(&self) -> &Path;
}

/// Every tree the app started that is still alive. Weak, so a tree that
/// was dropped (and so killed) is never kept alive by the registry.
static LIVE: Mutex<Vec<Weak<dyn Ends>>> = Mutex::new(Vec::new());

fn live() -> std::sync::MutexGuard<'static, Vec<Weak<dyn Ends>>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keep `tree` in the registry until it is dropped.
pub fn register<E: Ends + 'static>(tree: &Arc<E>) {
    let tree: Arc<dyn Ends> = tree.clone();
    let mut live = live();
    live.retain(|w| w.strong_count() > 0);
    live.push(Arc::downgrade(&tree));
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
/// running on it never loses half its files. Never looks at processes.
/// Returns the names removed.
pub fn sweep_leftover_profiles(temp: &Path) -> Vec<String> {
    let held = held_profiles();
    let Ok(entries) = std::fs::read_dir(temp) else { return Vec::new() };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_profile_name(&name) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path();
        if held.iter().any(|h| h == &path) {
            continue;
        }
        let aside = temp.join(format!("{name}.sweep"));
        if std::fs::rename(&path, &aside).is_err() {
            continue;
        }
        if std::fs::remove_dir_all(&aside).is_ok() {
            removed.push(name);
        } else {
            let _ = std::fs::rename(&aside, &path);
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
}

impl Tree {
    /// A tree with a new, empty job (Windows), for a browser about to be
    /// started on `profile_dir`.
    pub fn new(profile_dir: PathBuf) -> Result<Tree, String> {
        Ok(Tree {
            #[cfg(windows)]
            job: job::Job::new()?,
            profile_dir,
        })
    }

    /// Put a just-spawned, still suspended process into this tree's job,
    /// then let it run. Every process it starts joins the job with it.
    #[cfg(windows)]
    pub fn adopt_suspended(&self, child: &std::process::Child) -> Result<(), String> {
        self.job.adopt_suspended(child)
    }

    /// How many processes are in the tree now. `None` off Windows, where
    /// there is no job to ask.
    pub fn processes(&self) -> Option<u32> {
        #[cfg(windows)]
        {
            self.job.active_processes()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Is this process one of the tree's? Always false off Windows.
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

    /// Wait until no process is left in the tree, up to `within`.
    fn wait_empty(&self, within: Duration) -> bool {
        let began = Instant::now();
        loop {
            match self.processes() {
                None | Some(0) => return true,
                Some(_) if began.elapsed() >= within => return false,
                Some(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

impl Ends for Tree {
    fn terminate(&self) {
        #[cfg(windows)]
        self.job.terminate();
    }

    fn end(&self) -> bool {
        self.terminate();
        if !self.wait_empty(END_WITHIN) {
            crate::applog::warn(format!(
                "auto-run: the browser on profile {} still had {} processes {} s after it was ended",
                folder_name(&self.profile_dir),
                self.processes().unwrap_or(0),
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
}

#[cfg(windows)]
mod job {
    //! The Windows job object a browser runs in.

    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenThread, ResumeThread, PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
    };

    /// An owned job handle. Closing it (dropping this) kills every
    /// process still in the job.
    pub struct Job(OwnedHandle);

    fn os_error(what: &str) -> String {
        format!("{what}: {}", std::io::Error::last_os_error())
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
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the struct is the size passed, and lives for the call.
            let ok = unsafe {
                SetInformationJobObject(
                    job.raw(),
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(os_error("could not set up the browser's job"));
            }
            Ok(job)
        }

        fn raw(&self) -> HANDLE {
            self.0.as_raw_handle() as HANDLE
        }

        /// Assign a process created suspended to this job, then resume
        /// its threads. Assigned before it runs a single instruction, so
        /// no process it starts can be outside the job.
        pub fn adopt_suspended(&self, child: &std::process::Child) -> Result<(), String> {
            // SAFETY: both handles are valid for the call.
            if unsafe { AssignProcessToJobObject(self.raw(), child.as_raw_handle() as HANDLE) } == 0 {
                return Err(os_error("could not put the browser in its job"));
            }
            resume_threads(child.id())
        }

        /// How many processes are in the job now. `None` if Windows will
        /// not say.
        pub fn active_processes(&self) -> Option<u32> {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: the struct is the size passed, and lives for the call.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.raw(),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then_some(info.ActiveProcesses)
        }

        /// End every process in the job. Returns at once; they go a
        /// moment later.
        pub fn terminate(&self) {
            // SAFETY: a valid job handle.
            unsafe { TerminateJobObject(self.raw(), 1) };
        }

        /// Is the process with this pid in this job?
        pub fn holds(&self, pid: u32) -> bool {
            // SAFETY: the process handle is checked, and closed below.
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if process.is_null() {
                return false;
            }
            let mut inside = 0;
            // SAFETY: both handles are valid; `inside` lives for the call.
            let ok = unsafe { IsProcessInJob(process, self.raw(), &mut inside) };
            // SAFETY: opened above, closed once.
            unsafe { CloseHandle(process) };
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
