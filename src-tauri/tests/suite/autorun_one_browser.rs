//! An unattended run keeps one browser and gives each case a fresh browser
//! context: its own cookies and storage, its own page, its own tabs,
//! dialogs and downloads, disposed when the case ends.
//!
//! A fake launcher stands in for starting Edge, and a fake socket per
//! connection answers the way a real browser does: a context and a page in
//! it on request, and a session for the page. Everything they saw is kept
//! in one shared `World` for the tests to read.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use v2_lib::autorun::one_browser::{Launcher, OneBrowser, NO_FRESH_PAGE};
use v2_lib::browser::launch::still_alive;
use v2_lib::browser::tree::{self, Ends};
use v2_lib::autorun::replay::{run_selection, Browsers};
use v2_lib::autorun::sessions::now_ms;
use v2_lib::autorun::{store, CaseScript, LocalRun};
use v2_lib::browser::cdp::{Cdp, Transport};
use v2_lib::browser::session::{restore, SavedSession};

/// What every browser the fake started saw, in order.
#[derive(Default)]
struct World {
    /// Browsers started.
    launches: usize,
    /// Browsers closed, each with its profile.
    closes: usize,
    /// Every frame sent, with the browser it was sent to.
    sent: Vec<(usize, Value)>,
    /// Browsers whose process has gone.
    dead: Vec<usize>,
    /// A method that kills the browser it is sent to, once.
    die_on: Option<(usize, String)>,
    /// The context each page was made in.
    target_context: HashMap<String, String>,
    /// Contexts are refused, as a policy can switch them off.
    refuse_contexts: bool,
    /// A connection fails as reqwest says it, address and all.
    refuse_connect: bool,
    /// Each browser starts with its own blank page, as a launched Edge
    /// does, and reports it to `Target.getTargets`.
    launch_page: bool,
    /// Launches and closes, in the order they happened ("launch 1",
    /// "close 1").
    events: Vec<String>,
    /// Browsers whose spawned process ended while the browser itself
    /// kept running and answering, as when Edge hands its browser to
    /// another process.
    pid_ended: Vec<usize>,
    /// Browsers whose processes will not end when they are closed.
    undying: Vec<usize>,
    /// Browsers whose whole tree was ended (`Ends::end`).
    ended: Vec<usize>,
    /// Each started browser is kept in the app's registry of live trees.
    register: bool,
    /// A method that asks the run to stop when this browser is sent it.
    cancel_on: Option<(usize, String, Arc<AtomicBool>)>,
    next: u32,
}

impl World {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}-{}", self.next)
    }

    fn sent(&self, method: &str) -> Vec<(usize, Value)> {
        self.sent.iter().filter(|(_, f)| f["method"] == method).cloned().collect()
    }
}

/// The browser's own socket, one per connection.
struct FakeSocket {
    browser: usize,
    world: Arc<Mutex<World>>,
    incoming: VecDeque<String>,
}

impl Transport for FakeSocket {
    async fn send(&mut self, text: String) -> Result<(), String> {
        let v: Value = serde_json::from_str(&text).unwrap();
        let mut w = self.world.lock().unwrap();
        if w.dead.contains(&self.browser) {
            return Err("the socket closed".to_string());
        }
        let id = v["id"].as_u64().unwrap();
        let method = v["method"].as_str().unwrap_or("").to_string();
        w.sent.push((self.browser, v.clone()));
        if let Some((_, _, cancel)) = w.cancel_on.as_ref().filter(|(b, m, _)| *b == self.browser && *m == method) {
            cancel.store(true, Ordering::SeqCst);
        }
        if w.die_on.as_ref().is_some_and(|(b, m)| *b == self.browser && *m == method) {
            w.die_on = None;
            w.dead.push(self.browser);
            return Err("the socket closed".to_string());
        }
        if w.refuse_contexts && method == "Target.createBrowserContext" {
            self.incoming.push_back(json!({ "id": id, "error": { "code": -32000, "message": "Failed to create browser context." } }).to_string());
            return Ok(());
        }
        let result = match method.as_str() {
            "Target.createBrowserContext" => json!({ "browserContextId": w.id("C") }),
            "Target.createTarget" => {
                let target = w.id("T");
                let context = v["params"]["browserContextId"].as_str().unwrap_or("default").to_string();
                w.target_context.insert(target.clone(), context);
                json!({ "targetId": target })
            }
            "Target.attachToTarget" => json!({ "sessionId": format!("S-{}", v["params"]["targetId"].as_str().unwrap()) }),
            "Target.getTargets" if w.launch_page => json!({ "targetInfos": [
                { "targetId": format!("T-blank-{}", self.browser), "type": "page", "url": "about:blank",
                  "attached": false, "browserContextId": "default" }
            ] }),
            "Runtime.evaluate" if v["params"]["expression"] == "document" => json!({ "result": { "objectId": "doc" } }),
            "Runtime.callFunctionOn" => json!({ "result": { "value": true } }),
            _ => json!({}),
        };
        let mut reply = json!({ "id": id, "result": result });
        if let Some(s) = v.get("sessionId") {
            reply["sessionId"] = s.clone();
        }
        self.incoming.push_back(reply.to_string());
        Ok(())
    }

    async fn recv(&mut self) -> Option<Result<String, String>> {
        if self.world.lock().unwrap().dead.contains(&self.browser) {
            return None;
        }
        match self.incoming.pop_front() {
            Some(f) => Some(Ok(f)),
            None => {
                std::future::pending::<()>().await;
                None
            }
        }
    }
}

/// A started browser: which one, its profile folder, and its process
/// tree.
struct FakeProcess {
    browser: usize,
    profile: PathBuf,
    tree: Arc<FakeTree>,
}

/// A browser's processes, as the app's registry of live trees sees them.
/// Ending them kills the browser (its sockets close) and removes its
/// profile, unless they will not end.
struct FakeTree {
    browser: usize,
    profile: PathBuf,
    world: Arc<Mutex<World>>,
}

impl Ends for FakeTree {
    fn terminate(&self) {}

    fn end(&self) -> bool {
        let mut w = self.world.lock().unwrap();
        w.ended.push(self.browser);
        w.dead.push(self.browser);
        if w.undying.contains(&self.browser) {
            return false;
        }
        let _ = std::fs::remove_dir_all(&self.profile);
        true
    }

    fn profile_dir(&self) -> &std::path::Path {
        &self.profile
    }
}

struct FakeLauncher {
    world: Arc<Mutex<World>>,
    profiles: PathBuf,
}

impl Launcher for FakeLauncher {
    type Process = FakeProcess;
    type T = FakeSocket;

    async fn launch(&mut self) -> Result<FakeProcess, String> {
        let mut w = self.world.lock().unwrap();
        w.launches += 1;
        let browser = w.launches;
        w.events.push(format!("launch {browser}"));
        let profile = self.profiles.join(format!("profile-{browser}"));
        std::fs::create_dir_all(&profile).unwrap();
        let tree = Arc::new(FakeTree { browser, profile: profile.clone(), world: Arc::clone(&self.world) });
        if w.register {
            tree::register(&tree);
        }
        Ok(FakeProcess { browser, profile, tree })
    }

    async fn connect(&mut self, p: &FakeProcess) -> Result<Cdp<FakeSocket>, String> {
        let w = self.world.lock().unwrap();
        if w.refuse_connect {
            return Err(format!("error sending request for url (http://127.0.0.1:9{}/json/version)", p.browser));
        }
        if w.dead.contains(&p.browser) {
            return Err("the browser did not answer".to_string());
        }
        drop(w);
        Ok(Cdp::over(FakeSocket { browser: p.browser, world: Arc::clone(&self.world), incoming: VecDeque::new() }))
    }

    /// What the real launcher asks: does the job still have processes,
    /// and does DevTools answer? A dead browser that will not end still
    /// has its processes; it just no longer answers.
    async fn alive(&mut self, p: &mut FakeProcess) -> bool {
        let w = self.world.lock().unwrap();
        let dead = w.dead.contains(&p.browser);
        let processes = if dead && !w.undying.contains(&p.browser) { 0 } else { 5 };
        still_alive(Some(processes), w.pid_ended.contains(&p.browser), !dead)
    }

    fn close(&mut self, p: FakeProcess) -> Result<(), FakeProcess> {
        self.world.lock().unwrap().events.push(format!("close {}", p.browser));
        if !p.tree.end() {
            return Err(p);
        }
        assert!(!p.profile.exists(), "the profile went with the tree");
        self.world.lock().unwrap().closes += 1;
        Ok(())
    }
}

fn browsers(profiles: &std::path::Path) -> (OneBrowser<FakeLauncher>, Arc<Mutex<World>>) {
    let world = Arc::new(Mutex::new(World::default()));
    (OneBrowser::new(FakeLauncher { world: Arc::clone(&world), profiles: profiles.to_path_buf() }), world)
}

fn new_run(id: &str) -> LocalRun {
    LocalRun { id: id.into(), pbi_id: 42, started_at: "1700000000000".into(), cases: vec![], mode: "unattended".into(), published: None, environment: None, resets: vec![] }
}

/// A one-step script with a single `check_text`: passes on a page that
/// answers, and fails on a browser that has gone.
fn passing_script(case_id: i32) -> CaseScript {
    serde_json::from_value(json!({ "case_id": case_id, "title": format!("case {case_id}"), "steps": [
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] }
    ] }))
    .unwrap()
}

/// Run these cases through the unattended engine.
async fn run(b: &mut OneBrowser<FakeLauncher>, root: &std::path::Path, ids: &[i32]) -> LocalRun {
    run_until(b, root, ids, &AtomicBool::new(false)).await
}

/// The same, stopped when `cancel` is set, as Stop sets it.
async fn run_until(b: &mut OneBrowser<FakeLauncher>, root: &std::path::Path, ids: &[i32], cancel: &AtomicBool) -> LocalRun {
    for id in ids {
        store::save_script(root, &passing_script(*id)).unwrap();
    }
    let cases: Vec<(i32, String)> = ids.iter().map(|id| (*id, format!("case {id}"))).collect();
    let mut run = new_run("run-1");
    run_selection(b, root, "Acme", "Web", &mut run, &cases, &crate::common::quick(), cancel, &mut |_| {}).await.unwrap();
    run
}

/// The page session each case drove as `main`, by the context its page
/// was made in, in the order they were attached.
fn mains(w: &World) -> Vec<(String, String)> {
    w.sent("Target.attachToTarget")
        .iter()
        .map(|(_, f)| {
            let target = f["params"]["targetId"].as_str().unwrap().to_string();
            (format!("S-{target}"), w.target_context.get(&target).cloned().unwrap_or_default())
        })
        .collect()
}

#[tokio::test]
async fn a_run_launches_one_browser_and_a_context_per_case() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    let run = run(&mut b, dir.path(), &[1, 2, 3]).await;
    assert_eq!(run.cases.len(), 3);
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", run.cases);

    let w = world.lock().unwrap();
    assert_eq!(w.launches, 1, "one browser for the whole run");
    let made = w.sent("Target.createBrowserContext");
    assert_eq!(made.len(), 3, "a context per case");
    for (_, f) in &made {
        assert_eq!(f["params"], json!({ "disposeOnDetach": false }));
        assert!(f.get("sessionId").is_none(), "a context is the browser's own call");
    }

    // Each case's page was made in a context of its own, and driven.
    let mains = mains(&w);
    assert_eq!(mains.len(), 3);
    let contexts: Vec<&String> = mains.iter().map(|(_, c)| c).collect();
    assert!(contexts.iter().all(|c| c.starts_with("C-")), "{contexts:?}");
    assert_ne!(contexts[0], contexts[1]);
    assert_ne!(contexts[1], contexts[2]);

    // Each was disposed when its case ended, in order.
    let disposed: Vec<String> = w
        .sent("Target.disposeBrowserContext")
        .iter()
        .map(|(_, f)| f["params"]["browserContextId"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(disposed, contexts.iter().map(|c| c.to_string()).collect::<Vec<_>>());

    // Each case's downloads are asked of its own context.
    let downloads: Vec<String> = w
        .sent("Browser.setDownloadBehavior")
        .iter()
        .map(|(_, f)| f["params"]["browserContextId"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(downloads, disposed);

    // Each case's page commands went on its own page's session.
    for (session, _) in &mains {
        assert!(w.sent.iter().any(|(_, f)| f["sessionId"] == *session && f["method"] == "Runtime.callFunctionOn"), "{session}");
    }
}

#[tokio::test]
async fn a_second_case_sees_none_of_the_first_cases_tabs_dialogs_or_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());

    let mut first = b.open().await.unwrap();
    first.enable_downloads(&dir.path().join("dl")).await.unwrap();
    let first_context = first.browser_context().unwrap().to_string();
    let first_main = first.current().unwrap().session_id.clone();
    first.transport_mut().incoming.extend(
        [
            json!({ "method": "Target.attachedToTarget", "params": {
                "sessionId": "S-pop", "waitingForDebugger": false,
                "targetInfo": { "targetId": "T-pop", "type": "page", "url": "https://hr.example/pop", "browserContextId": first_context }
            } }),
            json!({ "method": "Page.javascriptDialogOpening", "sessionId": first_main, "params": { "type": "alert", "message": "first case's alert" } }),
            json!({ "method": "Browser.downloadWillBegin", "params": { "guid": "g-1", "suggestedFilename": "a.csv", "frameId": "x" } }),
        ]
        .map(|f| f.to_string()),
    );
    first.pump(Duration::from_millis(50)).await;
    assert_eq!(first.tabs().len(), 2, "the first case saw its popup");
    assert_eq!(first.all_downloads().len(), 1, "the first case saw its download");
    b.close(first).await;

    let mut second = b.open().await.unwrap();
    second.enable_downloads(&dir.path().join("dl")).await.unwrap();
    assert_ne!(second.browser_context(), Some(first_context.as_str()));
    // A page left over from the first case's context is not this case's.
    second.transport_mut().incoming.push_back(
        json!({ "method": "Target.attachedToTarget", "params": {
            "sessionId": "S-old", "waitingForDebugger": true,
            "targetInfo": { "targetId": "T-old", "type": "page", "url": "https://hr.example/pop", "browserContextId": first_context }
        } })
        .to_string(),
    );
    second.pump(Duration::from_millis(50)).await;
    assert_eq!(second.tabs().len(), 1, "only the second case's own page");
    assert_eq!(second.tabs()[0].name.as_deref(), Some("main"));
    assert!(second.all_downloads().is_empty());
    assert!(second.take_dialogs().is_empty());

    let w = world.lock().unwrap();
    assert_eq!(w.launches, 1);
    let detached = w.sent("Target.detachFromTarget");
    assert!(detached.iter().any(|(_, f)| f["params"]["sessionId"] == "S-old"), "the other context's page was let go");
    let ran = w.sent("Runtime.runIfWaitingForDebugger");
    assert!(ran.iter().any(|(_, f)| f["sessionId"] == "S-old"), "and not left paused");
    assert!(
        !w.sent.iter().any(|(_, f)| f["sessionId"] == "S-old" && f["method"] == "Page.enable"),
        "it was never set up as a tab"
    );
}

#[tokio::test]
async fn a_crashed_browser_is_relaunched_for_the_next_case() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    // The first browser dies in the middle of the first case's step.
    world.lock().unwrap().die_on = Some((1, "Runtime.callFunctionOn".to_string()));
    let run = run(&mut b, dir.path(), &[1, 2]).await;

    assert_eq!(run.cases.len(), 2, "the case the browser died in is still recorded");
    assert_ne!(run.cases[0].proposed, "Passed", "{:?}", run.cases[0]);
    assert!(!run.cases[0].steps.is_empty(), "{:?}", run.cases[0]);
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", run.cases[1]);

    let w = world.lock().unwrap();
    assert_eq!(w.launches, 2, "a new browser for the next case");
    assert_eq!(w.closes, 1, "the dead one was closed and its profile removed");
    assert!(!dir.path().join("profile-1").exists());
    let second_case_context = w.sent("Target.createBrowserContext");
    assert_eq!(second_case_context.last().unwrap().0, 2, "the next case's context is in the new browser");
}

#[tokio::test]
async fn the_profile_is_deleted_once_at_the_end_of_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    let run = run(&mut b, dir.path(), &[1, 2, 3]).await;
    assert_eq!(run.cases.len(), 3);
    {
        let w = world.lock().unwrap();
        assert_eq!(w.closes, 0, "the browser stays between cases");
    }
    assert!(dir.path().join("profile-1").is_dir());
    drop(b);
    let w = world.lock().unwrap();
    assert_eq!(w.closes, 1, "closed once, when the run is over");
    assert!(!dir.path().join("profile-1").exists(), "its profile went with it");
}

#[tokio::test]
async fn a_saved_session_is_loaded_into_each_new_context() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    let saved = SavedSession {
        saved_at_ms: now_ms(),
        cookies: vec![json!({ "name": "sid", "value": "abc", "domain": "hr.example.internal", "path": "/", "session": true })],
        local_storage: vec![],
    };
    let mut sessions = Vec::new();
    for _ in 0..2 {
        let mut d = b.open().await.unwrap();
        sessions.push((d.current().unwrap().session_id.clone(), d.browser_context().unwrap().to_string()));
        restore(&mut d, &saved).await.unwrap();
        b.close(d).await;
    }
    assert_ne!(sessions[0].1, sessions[1].1, "two contexts");

    let w = world.lock().unwrap();
    let mains = mains(&w);
    let set: Vec<(String, String)> = w
        .sent("Network.setCookies")
        .iter()
        .map(|(_, f)| {
            assert_eq!(f["params"]["cookies"][0]["name"], "sid");
            let session = f["sessionId"].as_str().unwrap_or("").to_string();
            let context = mains.iter().find(|(s, _)| *s == session).map(|(_, c)| c.clone()).unwrap_or_default();
            (session, context)
        })
        .collect();
    assert_eq!(set, sessions, "the cookies went into each case's own context, on its own page");
}

#[tokio::test]
async fn a_browser_that_will_not_open_reports_no_url() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    world.lock().unwrap().refuse_connect = true;
    let run = run(&mut b, dir.path(), &[1]).await;
    let case = &run.cases[0];
    assert_eq!(case.proposed, "Blocked", "{case:?}");
    assert!(case.reason.contains(NO_FRESH_PAGE), "{}", case.reason);
    assert!(!case.reason.contains("http://") && !case.reason.contains("127.0.0.1"), "{}", case.reason);
    let w = world.lock().unwrap();
    assert_eq!(w.launches, 1, "a fresh browser that fails is not started again");
    assert_eq!(w.closes, 1, "and it is not left running");
}

/// A page that does not say its context is not known to be another
/// case's: with cases run one at a time, no other context is live. It is
/// one of this case's tabs, and while the run is guarded it is guarded
/// before it runs. It is never let run bare.
#[tokio::test]
async fn a_page_that_does_not_say_its_context_is_guarded_before_it_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    let mut d = b.open().await.unwrap();
    d.guard_saves(&[]).await.unwrap();
    d.transport_mut().incoming.push_back(
        json!({ "method": "Target.attachedToTarget", "params": {
            "sessionId": "S-unsaid", "waitingForDebugger": true,
            "targetInfo": { "targetId": "T-unsaid", "type": "page", "url": "https://hr.example/pop" }
        } })
        .to_string(),
    );
    d.pump(Duration::from_millis(50)).await;
    assert_eq!(d.tabs().len(), 2, "the page is one of the case's tabs");
    let w = world.lock().unwrap();
    let on_it: Vec<(usize, &str)> = w
        .sent
        .iter()
        .enumerate()
        .filter(|(_, (_, f))| f["sessionId"] == "S-unsaid")
        .map(|(i, (_, f))| (i, f["method"].as_str().unwrap_or("")))
        .collect();
    let guarded = on_it.iter().find(|(_, m)| *m == "Fetch.enable").map(|(i, _)| *i);
    let ran = on_it.iter().find(|(_, m)| *m == "Runtime.runIfWaitingForDebugger").map(|(i, _)| *i);
    let guarded = guarded.unwrap_or_else(|| panic!("its interception was never asked: {on_it:?}"));
    let ran = ran.unwrap_or_else(|| panic!("it was left paused: {on_it:?}"));
    assert!(guarded < ran, "it ran before it was guarded: {on_it:?}");
    assert!(
        !w.sent("Target.detachFromTarget").iter().any(|(_, f)| f["params"]["sessionId"] == "S-unsaid"),
        "it was let go unguarded"
    );
}

#[tokio::test]
async fn a_refused_context_falls_back_to_a_browser_per_case() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    world.lock().unwrap().refuse_contexts = true;
    let run = run(&mut b, dir.path(), &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", run.cases);
    {
        let w = world.lock().unwrap();
        assert_eq!(w.sent("Target.createBrowserContext").len(), 1, "asked once, never again");
        assert!(w.sent("Target.disposeBrowserContext").is_empty());
        assert_eq!(w.launches, 2, "a browser per case");
        assert_eq!(w.closes, 2, "each closed with its case");
        let pages = w.sent("Target.createTarget");
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(|(_, f)| f["params"].get("browserContextId").is_none()), "{pages:?}");
        assert_eq!(pages[0].0, 1);
        assert_eq!(pages[1].0, 2);
    }
    drop(b);
    assert_eq!(world.lock().unwrap().closes, 2, "nothing left to close at the end");
}

/// A browser of its own per case drives the blank page the browser
/// started with, as a connection to a launched browser always did. A new
/// page beside it would leave that one open, to be taken for a tab the
/// case opened.
#[tokio::test]
async fn a_browser_per_case_drives_the_page_it_started_with() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    {
        let mut w = world.lock().unwrap();
        w.refuse_contexts = true;
        w.launch_page = true;
    }
    let run = run(&mut b, dir.path(), &[1, 2]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", run.cases);
    let w = world.lock().unwrap();
    assert!(w.sent("Target.createTarget").is_empty(), "no second page: {:?}", w.sent("Target.createTarget"));
    let driven: Vec<(usize, String)> = w
        .sent("Target.attachToTarget")
        .iter()
        .map(|(b, f)| (*b, f["params"]["targetId"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(driven, vec![(1, "T-blank-1".to_string()), (2, "T-blank-2".to_string())]);
}

/// A browser whose DevTools port answers but whose socket never finishes
/// its handshake is given up on, not waited on forever.
#[tokio::test]
async fn a_socket_that_never_opens_is_given_up_on() {
    use std::io::{Read, Write};
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ws_port = socket.local_addr().unwrap().port();
    let http = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = http.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = http.accept().unwrap();
        let mut buf = [0u8; 2048];
        let _ = s.read(&mut buf);
        let body = format!(r#"{{"webSocketDebuggerUrl":"ws://127.0.0.1:{ws_port}/devtools/browser/x"}}"#);
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = s.write_all(reply.as_bytes());
    });
    // Accepted, and never answered.
    std::thread::spawn(move || {
        let held = socket.accept();
        std::thread::sleep(Duration::from_secs(5));
        drop(held);
    });
    let began = std::time::Instant::now();
    let got = Cdp::connect_browser_within(port, Duration::from_millis(300)).await;
    let err = got.err().expect("a socket that never opened must be an error");
    assert!(err.contains("did not open within"), "{err}");
    assert!(began.elapsed() < Duration::from_secs(3), "{:?}", began.elapsed());
}

// ------------------------------------------ a browser as a process tree

/// Edge can hand its browser to another process: the pid the app spawned
/// ends while the browser keeps running on the same profile and keeps
/// answering. That browser is the run's browser still, not a dead one to
/// be replaced (which left one Edge per case behind).
#[tokio::test]
async fn a_browser_whose_process_ended_but_still_answers_is_kept_for_the_next_case() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    world.lock().unwrap().pid_ended.push(1);
    let run = run(&mut b, dir.path(), &[1, 2, 3]).await;
    assert!(run.cases.iter().all(|c| c.proposed == "Passed"), "{:?}", run.cases);
    let w = world.lock().unwrap();
    assert_eq!(w.launches, 1, "one browser for the whole run: {:?}", w.events);
    assert_eq!(w.closes, 0, "and it was never taken for closed");
    assert_eq!(w.sent("Target.createBrowserContext").iter().filter(|(b, _)| *b == 1).count(), 3);
}

#[tokio::test]
async fn a_dead_browser_is_closed_before_another_is_launched() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    world.lock().unwrap().die_on = Some((1, "Runtime.callFunctionOn".to_string()));
    let run = run(&mut b, dir.path(), &[1, 2]).await;
    assert_eq!(run.cases[1].proposed, "Passed", "{:?}", run.cases[1]);
    let w = world.lock().unwrap();
    assert_eq!(w.events, vec!["launch 1", "close 1", "launch 2"], "the old tree is gone before the new browser starts");
    assert_eq!(w.ended, vec![1], "the whole tree of the dead browser was ended");
    assert!(!dir.path().join("profile-1").exists());
}

/// A dead browser whose processes will not end is never left running
/// beside a new one: the case is Blocked in plain words instead.
#[tokio::test]
async fn a_browser_whose_tree_will_not_die_blocks_the_case_instead_of_launching_another() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    {
        let mut w = world.lock().unwrap();
        w.die_on = Some((1, "Runtime.callFunctionOn".to_string()));
        w.undying.push(1);
    }
    let run = run(&mut b, dir.path(), &[1, 2, 3]).await;
    assert_eq!(run.cases.len(), 3);
    for case in &run.cases[1..] {
        assert_eq!(case.proposed, "Blocked", "{case:?}");
        assert!(case.reason.contains(NO_FRESH_PAGE), "{}", case.reason);
    }
    {
        let w = world.lock().unwrap();
        assert_eq!(w.launches, 1, "no second browser beside the one still running: {:?}", w.events);
        // Once as case 1's context could not be disposed, and again before
        // each later case: it is never given up on, nor replaced.
        let tries = w.events.iter().filter(|e| *e == "close 1").count();
        assert!(tries >= 2, "it was tried again before a later case: {:?}", w.events);
    }
    drop(b);
    let w = world.lock().unwrap();
    assert_eq!(w.launches, 1);
    assert_eq!(w.events.last().map(String::as_str), Some("close 1"), "tried once more as the run ends");
}

#[tokio::test]
async fn stop_mid_case_closes_every_browser_the_run_started() {
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut w = world.lock().unwrap();
        // The first browser dies in case 1; Stop comes in the middle of
        // case 2, in the second browser.
        w.die_on = Some((1, "Runtime.callFunctionOn".to_string()));
        w.cancel_on = Some((2, "Runtime.callFunctionOn".to_string(), Arc::clone(&cancel)));
    }
    let run = run_until(&mut b, dir.path(), &[1, 2, 3], &cancel).await;
    assert!(run.cases.len() < 3, "the run stopped: {:?}", run.cases);
    drop(b);
    let w = world.lock().unwrap();
    assert_eq!(w.launches, 2, "{:?}", w.events);
    assert_eq!(w.closes, 2, "every browser the run started was closed: {:?}", w.events);
    assert_eq!(w.ended, vec![1, 2]);
    assert!(!dir.path().join("profile-1").exists() && !dir.path().join("profile-2").exists());
}

/// The app's exit closes the recorder, the supervised browser and the
/// held template browsers by name, and then every browser still in the
/// registry: an unattended run's, mid-case, which nothing else there
/// reaches.
#[tokio::test]
async fn the_exit_path_closes_an_unattended_runs_browser() {
    let _claims = crate::serial::autorun();
    let _held = crate::serial::held_browsers();
    let dir = tempfile::tempdir().unwrap();
    let (mut b, world) = browsers(dir.path());
    world.lock().unwrap().register = true;
    let d = b.open().await.unwrap();
    assert!(tree::held_profiles().contains(&dir.path().join("profile-1")), "the run's browser is registered");

    tokio::time::timeout(Duration::from_secs(5), v2_lib::commands::autorun::close_autorun_browsers())
        .await
        .expect("closing on exit is bounded");
    {
        let w = world.lock().unwrap();
        assert_eq!(w.ended, vec![1], "the run's browser was ended as the app exits");
        assert!(!dir.path().join("profile-1").exists(), "and its profile removed");
    }
    // The run then winds down as a stopped run does, with nothing left to
    // start or leak.
    b.close(d).await;
    drop(b);
    assert_eq!(world.lock().unwrap().launches, 1);
    assert!(!tree::held_profiles().contains(&dir.path().join("profile-1")), "dropped from the registry with the browser");
}

#[test]
fn a_browser_is_alive_by_its_processes_and_devtools_never_by_its_first_pid() {
    assert!(still_alive(Some(3), true, true), "the first pid ended, the browser runs on");
    assert!(!still_alive(Some(3), false, false), "running but not answering is wedged");
    assert!(!still_alive(Some(0), false, true), "no process left");
    assert!(still_alive(None, false, true), "no job to ask: the first pid decides");
    assert!(!still_alive(None, true, true));
}
