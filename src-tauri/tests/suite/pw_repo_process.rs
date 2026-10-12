//! The Playwright tab's repo commands (`pw_repo::process`): only `npx
//! playwright` and `npm run users`, never through a shell, one at a time, in
//! a Windows job that Stop and the app exiting end whole.
//!
//! The tests that start a process use `Launch::test_program` (cmd.exe and
//! ping, or this test binary itself), which no command can reach, and hold
//! `serial::pw_repo`: there is one run slot for the whole process.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;
use v2_lib::pw_repo::process::{
    self, busy_sentence, clean_line, resolve, start, start_repo, stream_lines, strip_ansi, Launch, Program, Resolved,
    RunEvent, RunId, Sink, CUT_MARK, LINE_CAP, NODE_NOT_FOUND, NO_NODE_MODULES, ONLY_NPX_AND_NPM, OUTSIDE_THE_CLONE,
};
use v2_lib::pw_repo::NOT_OFFERED;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw_repo")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A copy of the fixture clone with `node_modules`.
fn clone_with_node_modules() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), d.path());
    fs::create_dir_all(d.path().join("node_modules/@playwright/test")).unwrap();
    d
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn fake_node(_: Program) -> Option<Resolved> {
    Some(Resolved { exe: PathBuf::from("C:/node/node.exe"), lead: vec![OsString::from("C:/node/cli.js")] })
}

// ---- What can run ----------------------------------------------------------

#[test]
fn only_npx_and_npm_are_allowed() {
    let d = clone_with_node_modules();
    for program in ["node", "cmd", "cmd.exe", "powershell", "npx.cmd", "NPX", " npx", "git", ""] {
        let e = Launch::repo(d.path(), program, &strings(&["playwright", "test"]), fake_node).unwrap_err();
        assert_eq!(e, ONLY_NPX_AND_NPM, "{program:?} was not refused");
    }
    // npx only runs playwright; npm only runs the users script.
    for (program, args) in [
        ("npx", vec![]),
        ("npx", vec!["some-package"]),
        ("npx", vec!["--yes", "playwright"]),
        ("npm", vec!["install"]),
        ("npm", vec!["run"]),
        ("npm", vec!["run", "qa"]),
        ("npm", vec!["exec", "users"]),
        // npm reads an option before `--` as its own config.
        ("npm", vec!["run", "users"]),
        ("npm", vec!["run", "users", "list"]),
        ("npm", vec!["run", "users", "--script-shell=C:\\x.exe", "--", "list"]),
        ("npm", vec!["run", "users", "--node-options=--require=C:\\evil.js", "--", "list"]),
        ("npm", vec!["run", "users", "--prefix", "C:\\elsewhere", "--", "list"]),
        ("npm", vec!["run", "--prefix", "C:\\elsewhere", "users", "--", "list"]),
        ("npm", vec!["--prefix", "C:\\elsewhere", "run", "users", "--", "list"]),
    ] {
        let e = Launch::repo(d.path(), program, &strings(&args), fake_node).unwrap_err();
        assert_eq!(e, ONLY_NPX_AND_NPM, "{program} {args:?} was not refused");
    }

    let npx = Launch::repo(d.path(), "npx", &strings(&["playwright", "test", "--list"]), fake_node).unwrap();
    assert_eq!(npx.exe(), Path::new("C:/node/node.exe"));
    assert_eq!(npx.args(), ["C:/node/cli.js", "--no", "playwright", "test", "--list"].map(OsString::from));
    assert_eq!(npx.cwd(), d.path());
    assert_eq!(npx.label(), "npx playwright test");

    let npm = Launch::repo(d.path(), "npm", &strings(&["run", "users", "--", "list", "--json"]), fake_node).unwrap();
    // The app's own `--silent`: npm prints no banner carrying the arguments.
    assert_eq!(npm.args(), ["C:/node/cli.js", "run", "--silent", "users", "--", "list", "--json"].map(OsString::from));
    assert_eq!(npm.label(), "npm run users");
    // Options after `--` are the users script's, not npm's.
    let npm = Launch::repo(d.path(), "npm", &strings(&["run", "users", "--", "--prefix", "x"]), fake_node).unwrap();
    assert_eq!(npm.args(), ["C:/node/cli.js", "run", "--silent", "users", "--", "--prefix", "x"].map(OsString::from));
}

#[test]
fn a_playwright_config_or_reporter_file_must_be_inside_the_clone() {
    let d = clone_with_node_modules();
    let outside = tempfile::tempdir().unwrap();
    let evil = outside.path().join("evil.config.ts");
    fs::write(&evil, "").unwrap();
    let evil = evil.to_string_lossy().to_string();
    let escape = format!("..{}{}", std::path::MAIN_SEPARATOR, "x.config.ts");
    fs::write(d.path().join("other.config.ts"), "").unwrap();
    fs::create_dir_all(d.path().join("reporters")).unwrap();
    fs::write(d.path().join("reporters/mine.js"), "").unwrap();

    let (config_eq, tsconfig_eq) = (format!("--config={evil}"), format!("--tsconfig={evil}"));
    let (reporter_eq, attached) = (format!("--reporter=list,{evil}"), format!("-c{evil}"));
    let refused: [Vec<&str>; 16] = [
        vec!["-c", &evil],
        vec!["--config", &evil],
        vec![&config_eq],
        vec!["--tsconfig", &evil],
        vec![&tsconfig_eq],
        vec!["--reporter", &evil],
        vec![&reporter_eq],
        vec!["-c", &escape],
        vec!["-c", "missing.config.ts"],
        vec!["-c", "suites"],
        vec!["-c"],
        vec!["--config"],
        vec!["-c="],
        vec![&attached],
        vec!["-xc", &evil],
        vec!["test", "--list", "--config", &evil],
    ];
    for extra in refused {
        let mut args = vec!["playwright".to_string()];
        args.extend(extra.iter().map(|s| s.to_string()));
        let e = Launch::repo(d.path(), "npx", &args, fake_node).unwrap_err();
        assert_eq!(e, OUTSIDE_THE_CLONE, "{extra:?} was not refused");
    }

    let allowed = [
        vec!["test", "-c", "playwright.config.ts"],
        vec!["test", "--config", "other.config.ts"],
        vec!["test", "--config=other.config.ts"],
        vec!["test", "--reporter=json"],
        vec!["test", "--reporter", "list,reporters/mine.js"],
        vec!["test", "--list", "--test-list", ".qa-cli/run.list", "-x"],
        // After `--`, Playwright reads filters, never a file it loads.
        vec!["test", "--", "-c", "anything"],
    ];
    for extra in allowed {
        let mut args = vec!["playwright".to_string()];
        args.extend(extra.iter().map(|s| s.to_string()));
        assert!(Launch::repo(d.path(), "npx", &args, fake_node).is_ok(), "{extra:?} was refused");
    }
}

#[test]
fn a_long_line_is_cut_and_every_line_is_passed_on() {
    let mut text = Vec::new();
    text.extend_from_slice(b"first\r\n");
    text.extend(std::iter::repeat_n(b'a', LINE_CAP + 10_000));
    text.extend_from_slice(b"\nexactly\n");
    text.extend(std::iter::repeat_n(b'b', LINE_CAP));
    text.extend_from_slice(b"\n\x1b[1mlast\x1b[0m");
    let mut lines = Vec::new();
    stream_lines(std::io::Cursor::new(text), |l| lines.push(l));
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "first");
    assert_eq!(lines[1], format!("{}{CUT_MARK}", "a".repeat(LINE_CAP)));
    assert_eq!(lines[2], "exactly");
    assert_eq!(lines[3], "b".repeat(LINE_CAP), "a line of exactly the cap is whole");
    assert_eq!(lines[4], "last");

    // A cut inside a character leaves no half of it behind.
    let mut text = vec![b'a'; LINE_CAP - 1];
    text.extend_from_slice("\u{e9}\u{e9}\n".as_bytes());
    let mut lines = Vec::new();
    stream_lines(std::io::Cursor::new(text), |l| lines.push(l));
    assert_eq!(lines, [format!("{}{CUT_MARK}", "a".repeat(LINE_CAP - 1))]);
}

#[test]
fn the_label_never_carries_an_option_or_a_value() {
    let d = clone_with_node_modules();
    let l = Launch::repo(d.path(), "npm", &strings(&["run", "users", "--", "set", "A", "--password", "s3cret"]), fake_node);
    assert_eq!(l.unwrap().label(), "npm run users");
    let l = Launch::repo(d.path(), "npx", &strings(&["playwright", "--list"]), fake_node);
    assert_eq!(l.unwrap().label(), "npx playwright");
    let l = Launch::repo(d.path(), "npx", &strings(&["playwright", "suites/a b.spec.ts"]), fake_node);
    assert_eq!(l.unwrap().label(), "npx playwright");
    let l = Launch::repo(d.path(), "npx", &strings(&["playwright", "test", "--reporter=json"]), fake_node);
    assert_eq!(l.unwrap().label(), "npx playwright test");
    let l = Launch::repo(d.path(), "npx", &strings(&["playwright", "suites/a b.spec.ts"]), fake_node);
    assert_eq!(l.unwrap().label(), "npx playwright");
}

#[test]
fn a_folder_that_is_not_the_repo_or_has_no_node_modules_starts_nothing() {
    let args = strings(&["playwright", "test"]);
    let resolved = std::cell::Cell::new(false);
    let watch = |p: Program| {
        resolved.set(true);
        fake_node(p)
    };
    // The fixture itself has no node_modules.
    assert_eq!(Launch::repo(&fixture(), "npx", &args, watch).unwrap_err(), NO_NODE_MODULES);
    let empty = tempfile::tempdir().unwrap();
    let e = Launch::repo(empty.path(), "npx", &args, fake_node).unwrap_err();
    assert_eq!(e, v2_lib::pw_export::clone::NOT_A_CLONE);
    let e = Launch::repo(Path::new(""), "npx", &args, fake_node).unwrap_err();
    assert_eq!(e, v2_lib::pw_export::clone::NOT_A_CLONE);
    assert!(!resolved.get(), "nothing is looked up for a clone that cannot run");

    let d = clone_with_node_modules();
    assert_eq!(Launch::repo(d.path(), "npx", &args, |_| None).unwrap_err(), NODE_NOT_FOUND);
}

#[test]
fn every_start_is_refused_with_advanced_features_off() {
    let d = clone_with_node_modules();
    let (sink, _rx) = collect();
    let e = start_repo(false, &d.path().to_string_lossy(), "npx", &strings(&["playwright", "test"]), sink).unwrap_err();
    assert_eq!(e, NOT_OFFERED);
}

// ---- How npx and npm are found ---------------------------------------------

#[cfg(windows)]
mod resolving {
    use super::*;

    /// A folder laid out as Node's installer leaves it.
    fn node_install(dir: &Path) {
        fs::create_dir_all(dir.join("node_modules/npm/bin")).unwrap();
        for f in ["node.exe", "npx.cmd", "npm.cmd", "npx", "npm"] {
            fs::write(dir.join(f), "").unwrap();
        }
        for f in ["npx-cli.js", "npm-cli.js"] {
            fs::write(dir.join("node_modules/npm/bin").join(f), "").unwrap();
        }
    }

    fn path_of(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    const PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.JS";

    #[test]
    fn a_cmd_shim_is_never_run_its_node_runs_npms_own_script() {
        let d = tempfile::tempdir().unwrap();
        node_install(d.path());
        let path = path_of(&[d.path()]);
        for (program, cli) in [(Program::Npx, "npx-cli.js"), (Program::Npm, "npm-cli.js")] {
            let r = resolve(program, Some(&path), Some(PATHEXT.as_ref())).unwrap();
            assert_eq!(r.exe, d.path().join("node.exe"));
            assert_eq!(r.lead, vec![d.path().join("node_modules").join("npm").join("bin").join(cli).into_os_string()]);
        }
    }

    #[test]
    fn a_shim_without_nodes_layout_is_passed_over() {
        let bare = tempfile::tempdir().unwrap();
        fs::write(bare.path().join("npx.cmd"), "").unwrap();
        let path = path_of(&[bare.path()]);
        assert_eq!(resolve(Program::Npx, Some(&path), Some(PATHEXT.as_ref())), None);

        // ... and a later folder with the layout is used instead.
        let install = tempfile::tempdir().unwrap();
        node_install(install.path());
        let path = path_of(&[bare.path(), install.path()]);
        let r = resolve(Program::Npx, Some(&path), Some(PATHEXT.as_ref())).unwrap();
        assert_eq!(r.exe, install.path().join("node.exe"));
    }

    #[test]
    fn an_exe_is_run_as_it_is_and_the_first_folder_wins() {
        let first = tempfile::tempdir().unwrap();
        fs::write(first.path().join("npm.exe"), "").unwrap();
        let second = tempfile::tempdir().unwrap();
        node_install(second.path());
        let path = path_of(&[first.path(), second.path()]);
        // PATHEXT spells it .EXE; Windows file names ignore case.
        let r = resolve(Program::Npm, Some(&path), Some(PATHEXT.as_ref())).unwrap();
        assert_eq!(r, Resolved { exe: first.path().join("npm.EXE"), lead: vec![] });
    }

    #[test]
    fn no_path_or_nothing_on_it_finds_nothing() {
        assert_eq!(resolve(Program::Npx, None, None), None);
        let empty = tempfile::tempdir().unwrap();
        let path = path_of(&[empty.path()]);
        assert_eq!(resolve(Program::Npx, Some(&path), None), None);
    }
}

// ---- Output ------------------------------------------------------------------

#[test]
fn output_is_ansi_stripped() {
    assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m plain"), "red plain");
    assert_eq!(strip_ansi("\u{1b}[1;32m  ✓ \u{1b}[22m1 passed\u{1b}[39m"), "  ✓ 1 passed");
    assert_eq!(strip_ansi("\u{1b}[2K\u{1b}[1Gnext"), "next");
    assert_eq!(strip_ansi("\u{1b}]8;;https://x\u{7}link\u{1b}]8;;\u{7}"), "link");
    assert_eq!(strip_ansi("\u{1b}]0;title\u{1b}\\after"), "after");
    assert_eq!(strip_ansi("\u{1b}=keypad"), "keypad");
    assert_eq!(strip_ansi("trailing \u{1b}"), "trailing ");

    assert_eq!(clean_line(b"\x1b[33mwarn\x1b[0m\r\n"), "warn");
    assert_eq!(clean_line(b"[1/3] a\r[2/3] b\r[3/3] c\n"), "[3/3] c");
    assert_eq!(clean_line(b"tab\there\x07\n"), "tab\there");
    assert_eq!(clean_line(b"caf\xc3\xa9 \xff\n"), "café \u{fffd}");
}

// ---- Running -----------------------------------------------------------------

fn collect() -> (Sink, Receiver<RunEvent>) {
    let (tx, rx) = channel();
    let tx = std::sync::Mutex::new(tx);
    let sink: Sink = Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    (sink, rx)
}

/// Every event up to and including `Done`, within `within`.
fn until_done(rx: &Receiver<RunEvent>, within: Duration) -> (Vec<String>, Option<i32>, RunId) {
    let deadline = std::time::Instant::now() + within;
    let mut lines = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left).expect("the run ended in time") {
            RunEvent::Output { line, .. } => lines.push(line),
            RunEvent::Done { run, code } => return (lines, code, run),
        }
    }
}

#[cfg(windows)]
mod running {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    fn cmd() -> PathBuf {
        std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"))
    }

    /// cmd.exe (the run's process) starting ping (its child), which runs
    /// for about `seconds`.
    fn cmd_then_ping(seconds: u32) -> Launch {
        let args = ["/D", "/C", "ping", "-n", &seconds.to_string(), "127.0.0.1"].map(OsString::from).to_vec();
        Launch::test_program(cmd(), args, std::env::temp_dir(), "cmd ping")
    }

    /// Handles to `pids`, held so each pid stays that process.
    struct Held(Vec<HANDLE>);

    impl Held {
        fn open(pids: &[u32]) -> Held {
            Held(
                pids.iter()
                    .map(|&pid| {
                        // SAFETY: checked for null below.
                        let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
                        assert!(!h.is_null(), "process {pid} could be opened");
                        h
                    })
                    .collect(),
            )
        }

        fn all_ended_within(&self, ms: u32) -> bool {
            // SAFETY: valid handles, held for the call.
            self.0.iter().all(|&h| unsafe { WaitForSingleObject(h, ms) } == WAIT_OBJECT_0)
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            for &h in &self.0 {
                // SAFETY: opened above, closed once.
                unsafe { CloseHandle(h) };
            }
        }
    }

    /// The run's job holds at least `n` processes, waited for up to 10 s.
    fn job_holds(n: usize) -> Vec<u32> {
        for _ in 0..200 {
            let pids = process::running_pids();
            if pids.len() >= n {
                return pids;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the run's job never held {n} processes: {:?}", process::running_pids());
    }

    #[test]
    fn stop_ends_the_whole_run() {
        let _held_browsers = crate::serial::held_browsers();
        let _slot = crate::serial::pw_repo();
        let (sink, rx) = collect();
        let run = start(cmd_then_ping(60), sink).unwrap();
        // cmd.exe and the ping it started: a child and a grandchild of the app.
        let pids = job_holds(2);
        let held = Held::open(&pids);

        process::stop(run).unwrap();
        let (_, code, done) = until_done(&rx, Duration::from_secs(10));
        assert_eq!(done, run);
        assert_eq!(code, None, "a stopped run has no exit code");
        assert!(held.all_ended_within(5000), "every process of the run ended");
        assert_eq!(process::running(), None, "the slot is free again");
        // Stopping a run that is over is fine.
        process::stop(run).unwrap();
    }

    #[test]
    fn exit_ends_a_running_repo_command() {
        let _claims = crate::serial::autorun();
        let _held_browsers = crate::serial::held_browsers();
        let _slot = crate::serial::pw_repo();
        let (sink, rx) = collect();
        let run = start(cmd_then_ping(60), sink).unwrap();
        let held = Held::open(&job_holds(2));

        // The app's exit hook and an update restart both go through here.
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(5), v2_lib::commands::autorun::close_autorun_browsers())
                .await
                .expect("closing on exit is bounded")
        });
        let (_, code, done) = until_done(&rx, Duration::from_secs(10));
        assert_eq!((done, code), (run, None));
        assert!(held.all_ended_within(5000), "every process of the run ended");
        assert!(!process::stop_all(), "nothing is left running");
    }

    #[test]
    fn a_second_start_is_refused_with_a_sentence_naming_the_running_one() {
        let _held_browsers = crate::serial::held_browsers();
        let _slot = crate::serial::pw_repo();
        let (sink, rx) = collect();
        let run = start(cmd_then_ping(60), sink.clone()).unwrap();
        assert_eq!(process::running(), Some((run, "cmd ping".to_string())));

        let e = start(cmd_then_ping(1), sink).unwrap_err();
        assert_eq!(e, busy_sentence("cmd ping"));
        assert!(e.starts_with("cmd ping is still running."), "{e}");

        process::stop(run).unwrap();
        until_done(&rx, Duration::from_secs(10));
        // Free again once it is over.
        let (sink, rx) = collect();
        let again = start(cmd_then_ping(1), sink).unwrap();
        assert!(again > run);
        until_done(&rx, Duration::from_secs(10));
    }

    #[test]
    fn output_streams_line_by_line_stripped_then_done_with_the_exit_code() {
        let _held_browsers = crate::serial::held_browsers();
        let _slot = crate::serial::pw_repo();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.txt");
        fs::write(&file, "\u{1b}[31mred\u{1b}[0m\r\nplain\r\n\u{1b}[1mlast\u{1b}[22m").unwrap();
        let args = ["/D", "/C", "type", "out.txt"].map(OsString::from).to_vec();
        let (sink, rx) = collect();
        start(Launch::test_program(cmd(), args, dir.path().to_path_buf(), "cmd type"), sink).unwrap();
        let (lines, code, _) = until_done(&rx, Duration::from_secs(10));
        assert_eq!(lines, ["red", "plain", "last"]);
        assert_eq!(code, Some(0));

        let args = ["/D", "/C", "exit", "3"].map(OsString::from).to_vec();
        let (sink, rx) = collect();
        start(Launch::test_program(cmd(), args, dir.path().to_path_buf(), "cmd exit"), sink).unwrap();
        assert_eq!(until_done(&rx, Duration::from_secs(10)).1, Some(3));
    }

    /// The arguments a run is given reach its program exactly, whatever
    /// they hold: `&`, `^`, `%` and `"` would mean something to cmd.exe,
    /// and none passes through it. The program is this test binary, run
    /// again as the child that prints its own arguments.
    #[test]
    fn arguments_reach_the_program_unchanged() {
        const CHILD: &str = "pw-repo-echo-child";
        const NAME: &str = "pw_repo_process::running::arguments_reach_the_program_unchanged";
        let tricky = [
            "a b",
            "&",
            "a&b",
            "^",
            "x^y",
            "%PATH%",
            "100%",
            "\"",
            "say \"hi\"",
            "ends\\",
            "with space\\",
            "|<>",
            "",
        ];
        let mine: Vec<String> = std::env::args().collect();
        if mine.iter().any(|a| a == CHILD) {
            let after = mine.iter().position(|a| a == CHILD).unwrap() + 1;
            println!("ARGS:{}", serde_json::to_string(&mine[after..]).unwrap());
            return;
        }
        let _held_browsers = crate::serial::held_browsers();
        let _slot = crate::serial::pw_repo();
        let mut args: Vec<OsString> = [NAME, "--exact", "--nocapture", "--test-threads=1", CHILD].map(OsString::from).to_vec();
        args.extend(tricky.iter().map(OsString::from));
        let exe = std::env::current_exe().unwrap();
        let (sink, rx) = collect();
        start(Launch::test_program(exe, args, std::env::temp_dir(), "echo"), sink).unwrap();
        let (lines, code, _) = until_done(&rx, Duration::from_secs(60));
        let echoed = lines.iter().find_map(|l| l.split_once("ARGS:").map(|(_, json)| json)).unwrap_or_else(|| panic!("no ARGS line: {lines:?}"));
        let echoed: Vec<String> = serde_json::from_str(echoed).unwrap();
        assert_eq!(echoed, tricky);
        assert_eq!(code, Some(0));
    }
}
