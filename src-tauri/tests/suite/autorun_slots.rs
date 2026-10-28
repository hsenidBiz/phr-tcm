//! The Auto Run browser's numbered slots: main is the one the person and
//! every existing path use; two more sit beside it for assistants beyond the
//! first. These pin the table itself, the state each slot keeps apart, that
//! main's old names still mean main, and the rules that pick a slot.

use crate::serial;
use v2_lib::autorun::overrides::Overrides;
use v2_lib::autorun::slots::{SlotNo, EXTRA_CAP, SLOT_COUNT};
use v2_lib::autorun::store::{
    downloads_dir, empty_supervised_downloads, empty_supervised_downloads_for, supervised_downloads_dir,
    supervised_downloads_dir_for,
};
use v2_lib::commands::autorun::{
    any_discovery_active, auto_run_discovery_active, discovery_active_in, note_session_use, note_session_use_in,
    overrides_now, overrides_now_in, session_uses, session_uses_in, show_overrides, show_overrides_in,
};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-slots-test-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn changed() -> Overrides {
    let mut o = Overrides::new();
    o.headers.insert("x-test".to_string(), "1".to_string());
    o
}

/// Three browsers in all: main, and two beside it. A number from the cap up
/// is no slot.
#[test]
fn slot_numbers() {
    assert_eq!((EXTRA_CAP, SLOT_COUNT), (2, 3));
    assert_eq!(SlotNo::all(), [SlotNo(0), SlotNo(1), SlotNo(2)]);
    assert_eq!(SlotNo::extras(), [SlotNo(1), SlotNo(2)]);
    assert_eq!(SlotNo::from_number(0), Some(SlotNo::MAIN));
    assert_eq!(SlotNo::from_number(2), Some(SlotNo(2)));
    assert_eq!(SlotNo::from_number(3), None);
    assert_eq!(SlotNo::from_number(255), None);
    assert!(SlotNo::MAIN.is_main());
    assert!(SlotNo::extras().iter().all(|s| !s.is_main()));
    assert_eq!(SlotNo::all().map(SlotNo::index), [0, 1, 2]);
}

/// Each slot saves into a folder of its own, and emptying one leaves the
/// others' files and the runs' alone. Main's is today's folder.
#[test]
fn each_slot_downloads_into_its_own_folder() {
    let dir = TempDir::new();
    let root = dir.path();
    assert_eq!(supervised_downloads_dir_for(root, SlotNo::MAIN), supervised_downloads_dir(root));
    let one = supervised_downloads_dir_for(root, SlotNo(1));
    assert!(one.ends_with(std::path::Path::new("downloads").join("supervised-1")), "{one:?}");
    let two = supervised_downloads_dir_for(root, SlotNo(2));
    assert!(two.ends_with(std::path::Path::new("downloads").join("supervised-2")), "{two:?}");

    for s in SlotNo::all() {
        let d = supervised_downloads_dir_for(root, s);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.csv"), "x").unwrap();
    }
    empty_supervised_downloads_for(root, SlotNo(1)).unwrap();
    assert!(!one.join("a.csv").exists(), "slot 1's file stayed");
    assert!(supervised_downloads_dir(root).join("a.csv").exists(), "main's file went with slot 1's");
    assert!(two.join("a.csv").exists(), "slot 2's file went with slot 1's");

    // Main's emptying is still the old function, and leaves the extras.
    assert_eq!(empty_supervised_downloads(root).unwrap(), 1);
    assert!(!supervised_downloads_dir(root).join("a.csv").exists());
    assert!(two.join("a.csv").exists());
    // Nothing there is nothing to do.
    empty_supervised_downloads_for(&root.join("missing"), SlotNo(2)).unwrap();
}

/// A run whose id looks like an extra browser's folder never gets that
/// folder: it would share, and be emptied with, a live browser's downloads.
#[test]
fn a_run_never_gets_a_slots_download_folder() {
    let dir = TempDir::new();
    let root = dir.path();
    for s in SlotNo::all() {
        let name = supervised_downloads_dir_for(root, s).file_name().unwrap().to_string_lossy().to_string();
        assert_ne!(downloads_dir(root, &name), supervised_downloads_dir_for(root, s), "{name} shares a slot's folder");
        assert_ne!(
            downloads_dir(root, &name.to_uppercase()),
            supervised_downloads_dir_for(root, s),
            "{name} in capitals shares a slot's folder"
        );
        assert!(
            v2_lib::autorun::store::download_files(root, &name).is_empty(),
            "{name}'s folder is read as a run's"
        );
        assert!(v2_lib::autorun::store::download_file(root, &name, "a.csv").is_err());
    }
}

/// A slot keeps its own use count and overrides: a change in slot 1 moves
/// neither main's count nor main's overrides, and main's old names are main.
#[test]
fn per_slot_uses_and_overrides_are_separate() {
    let _g = serial::autorun();
    let one = SlotNo(1);
    let (main_uses, one_uses) = (session_uses(), session_uses_in(one));
    note_session_use_in(one);
    assert_eq!(session_uses_in(one), one_uses + 1);
    assert_eq!(session_uses(), main_uses, "main's count moved with slot 1's");
    note_session_use();
    assert_eq!(session_uses(), main_uses + 1);
    assert_eq!(session_uses_in(SlotNo::MAIN), main_uses + 1, "the old name is not main");
    assert_eq!(session_uses_in(one), one_uses + 1, "slot 1's count moved with main's");

    let main_before = overrides_now();
    show_overrides_in(one, &changed());
    assert_eq!(overrides_now_in(one), changed());
    assert_eq!(overrides_now(), main_before, "main's overrides moved with slot 1's");
    show_overrides(&changed());
    assert_eq!(overrides_now_in(SlotNo::MAIN), changed(), "the old name is not main");
    show_overrides(&Overrides::new());
    assert_eq!(overrides_now_in(one), changed(), "slot 1's overrides moved with main's");
    show_overrides_in(one, &Overrides::new());
    assert_eq!(overrides_now_in(one), Overrides::new());
    assert_eq!(overrides_now(), Overrides::new());
}

/// With no browser open, no slot is discovering, and main's old name says so.
#[test]
fn no_slot_is_discovering_when_nothing_is_open() {
    let _g = serial::autorun();
    assert!(!any_discovery_active());
    for s in SlotNo::all() {
        assert!(!discovery_active_in(s), "{s:?}");
    }
    assert!(!auto_run_discovery_active());
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn body(text: &str, start: &str) -> String {
    let from = &text[text.find(start).unwrap_or_else(|| panic!("{start} is gone"))..];
    from[..from.find("\n}\n").unwrap()].to_string()
}

/// A browser opens into a numbered slot, saves its downloads in that slot's
/// folder, and its Session says which slot it is; closing it and the person
/// taking it count and clear that slot, not main. The person's commands keep
/// locking `SESSION` as written.
#[test]
fn a_session_knows_its_slot() {
    let source = norm(include_str!("../../src/commands/autorun.rs"));
    let open = body(&source, "async fn open_into(");
    assert!(open.contains("n: SlotNo"), "open_into takes no slot");
    assert!(open.contains("slot: n,"), "the session does not record its slot");
    assert!(open.contains("keep_supervised_downloads(root, n, &mut cdp)"), "downloads are not the slot's");
    assert!(open.contains("note_session_use_in(n);"), "{open}");
    assert!(open.contains("answer_between_commands_in(n);"), "{open}");

    let keep = body(&source, "async fn keep_supervised_downloads(");
    assert!(keep.contains("empty_supervised_downloads_for(&root, n)"), "{keep}");
    assert!(keep.contains("supervised_downloads_dir_for(&root, n)"), "{keep}");

    let close = body(&source, "fn close_session(");
    assert!(close.contains("note_session_use_in(s.slot);"), "{close}");
    assert!(close.contains("show_overrides_in(s.slot,"), "{close}");
    assert!(close.contains("empty_supervised_downloads_for(&root, s.slot)"), "{close}");

    let taken = body(&source, "fn taken_over(&mut self) {");
    assert!(taken.contains("note_session_use_in(self.slot);"), "{taken}");

    let persons = body(&source, "pub(crate) async fn make_persons(");
    assert!(persons.contains("show_overrides_in(self.slot, &self.overrides);"), "{persons}");

    // The person's Open browser opens main.
    let person = body(&source, "pub async fn auto_run_open_browser(");
    let calls: Vec<&str> = person
        .lines()
        .filter(|l| l.contains("open_into(") && !l.trim_start().starts_with("//"))
        .collect();
    assert_eq!(calls.len(), 1, "the person's Open browser moved: {calls:?}");
    assert!(calls[0].contains("SlotNo::MAIN"), "the person's Open browser opens a slot other than main: {}", calls[0]);
    assert!(source.contains("tokio::sync::Mutex::const_new(None);"));
    assert!(source.contains("static SESSION: tokio::sync::Mutex<Option<Session>>"), "SESSION is renamed");
}

/// Release all and the app's exit stop the replay of every slot and every
/// batch before they wait for a browser; a replay for a slot locks that
/// slot's session and claims that slot's replay; the person's Stop and Close
/// are main's.
#[test]
fn release_all_and_exit_stop_every_slots_replay_and_batch() {
    let source = norm(include_str!("../../src/commands/autorun.rs"));
    // The first thing each one waits for: Release all's opening lock, the
    // exit's first slot lock.
    for (start, first_wait) in [
        ("pub async fn release_autorun_browsers()", "opening().write()"),
        ("pub async fn close_autorun_browsers()", "session_slot("),
    ] {
        let from = &source[source.find(start).unwrap_or_else(|| panic!("{start} is gone"))..];
        let before_lock = &from[..from.find(first_wait).unwrap_or_else(|| panic!("{start} no longer waits with {first_wait}"))];
        assert!(before_lock.contains("for n in SlotNo::all() {"), "{start} does not walk the slots: {before_lock}");
        assert!(before_lock.contains("crate::autorun::replay_to::state(n).stop();"), "{start}: {before_lock}");
        assert!(before_lock.contains("crate::ai_bridge::stop_all_batches();"), "{start}: {before_lock}");
    }
    let replay = body(&source, "pub(crate) async fn replay_supervised(");
    assert!(replay.contains("OneReplay::claim_for_assistant_in(n)") && replay.contains("OneReplay::claim_in(n)"), "{replay}");
    assert!(replay.contains("replay_to::stopped_before_opening(&replay_to::state(n).cancel)"), "{replay}");
    assert!(replay.contains("session_slot(n).lock().await"), "{replay}");
    assert!(replay.contains("open_if_none(app, &mut slot, n, by.opens_as())"), "{replay}");
    assert!(replay.contains("note_session_use_in(n);") && replay.contains("overrides_now_in(n)"), "{replay}");
    assert!(!replay.contains("replay_to::CANCEL") && !replay.contains("OneReplay::claim()"), "{replay}");
    // The person's own Stop and Close are the main browser's.
    let stop = body(&source, "pub fn auto_run_stop_replay(");
    assert!(stop.contains("crate::autorun::replay_to::stop();"), "{stop}");
}

// ---------------------------------------------------------------------------
// slot_for: which browser a bridge route uses (Task 7)
// ---------------------------------------------------------------------------

mod routing {
    use super::*;
    use v2_lib::autorun::agents::{AgentKey, Caller};
    use v2_lib::commands::autorun_slots::{need_of, slot_for, Need};

    /// Every route that uses a supervised browser names what it does with
    /// it; every other route names nothing.
    #[test]
    fn need_of_names_every_browser_route() {
        for path in ["/autorun-discover-start", "/autorun-replay"] {
            assert_eq!(need_of("POST", path), Some(Need::Open), "{path}");
        }
        for path in [
            "/autorun-try",
            "/autorun-discover-action",
            "/autorun-discover-actions",
            "/autorun-discover-area",
            "/autorun-discover-end",
            "/autorun-replay-stop",
            "/autorun-release",
            "/autorun-reset",
            "/autorun-cookies-clear",
            "/autorun-headers",
            "/autorun-cookies",
            "/autorun-cookie",
            "/api-template-send",
            "/api-template-explore-undo",
        ] {
            assert_eq!(need_of("POST", path), Some(Need::Act), "{path}");
        }
        for path in [
            "/autorun-probe",
            "/autorun-inspect",
            "/autorun-requests",
            "/autorun-requests-search",
            "/autorun-request",
            "/autorun-response",
            "/autorun-response-body",
            "/autorun-requests-export",
            "/autorun-component-save",
            "/autorun-validation-rule",
            "/autorun-explore-change",
        ] {
            assert_eq!(need_of("POST", path), Some(Need::Read), "{path}");
        }
        assert_eq!(need_of("GET", "/autorun-page"), Some(Need::Read));
        assert_eq!(need_of("GET", "/autorun-capture"), Some(Need::Read));
        assert_eq!(need_of("POST", "/autorun-capture"), Some(Need::Read));
        // Every other route uses no supervised browser.
        for (method, path) in [
            ("GET", "/ping"),
            ("POST", "/autorun-requests-import"),
            ("POST", "/autorun-explore-changes"),
            ("POST", "/api-template-run"),
            ("POST", "/autorun-script"),
            ("GET", "/autorun-guide"),
            ("POST", "/autorun-order"),
            ("POST", "/agent-bye"),
            // What All projects as one added: Azure DevOps reads and the
            // per-caller writing job, none of which uses a browser.
            ("GET", "/search-pbis"),
            ("GET", "/suites"),
            ("GET", "/suite-cases"),
            ("GET", "/run-results"),
            ("GET", "/search-wiki"),
            ("GET", "/wiki-page"),
            ("GET", "/tags"),
            ("POST", "/begin"),
            ("GET", "/accounts"),
        ] {
            assert_eq!(need_of(method, path), None, "{method} {path}");
        }
        // A method a route does not answer to is no browser route either.
        assert_eq!(need_of("POST", "/autorun-page"), None);
        assert_eq!(need_of("GET", "/autorun-try"), None);
    }

    /// A template run (and a proof) that was refused its account because the
    /// Auto Run browser holds it asks whether the CALLER's own browser does,
    /// not main's: prove and run share the check, and both are given their
    /// caller's slot (none when it has no browser, which then gets no hint).
    #[test]
    fn a_template_run_asks_its_callers_own_browser() {
        fn body<'a>(source: &'a str, head: &str) -> &'a str {
            let from = &source[source.find(head).unwrap_or_else(|| panic!("{head} is gone"))..];
            &from[..from.find("\n}\n").unwrap_or(from.len())]
        }
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        let request = body(&bridge, "async fn run_api_template_request<");
        assert!(request.contains("asker: Option<SlotNo>"), "{request}");
        assert!(request.contains("assistant_browser_holds(n, &req.account)"), "{request}");
        assert!(!request.contains("SlotNo::MAIN"), "a template run still asks main's browser: {request}");
        for head in ["pub async fn api_template_prove_for<", "pub async fn api_template_run_for<"] {
            assert!(body(&bridge, head).contains("run_api_template_request(ctx, &root, req,"), "{head}");
            assert!(body(&bridge, head).contains("asker"), "{head}");
        }
        // The default caller's entry points are the old names, asking main.
        assert!(body(&bridge, "pub async fn api_template_prove<").contains("api_template_prove_for(Some(SlotNo::MAIN),"));
        assert!(body(&bridge, "pub async fn api_template_run<").contains("api_template_run_for(Some(SlotNo::MAIN),"));
        let route_as = body(&bridge, "pub async fn route_as(");
        let prove = &route_as[route_as.find("(\"POST\", \"/api-template-prove\")").unwrap()..];
        let prove = &prove[..prove.find("(\"POST\", \"/api-template-run\")").unwrap()];
        assert!(prove.contains("slot_if_any(caller)") && prove.contains("api_template_prove_for("), "{prove}");
        let run = &route_as[route_as.find("(\"POST\", \"/api-template-run\")").unwrap()..];
        let run = &run[..run.find("(\"POST\", \"/api-template-send\")").unwrap()];
        let run = run.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(run.contains("api_template_run_for( slot, ctx,"), "{run}");
    }

    /// A capture read that names an imported set (`from`) uses no browser, so
    /// it is never refused for lack of one; the same read of the live
    /// capture, a filter and an import are as `need_of` says.
    #[test]
    fn a_capture_read_of_an_imported_set_needs_no_browser() {
        use v2_lib::commands::autorun_slots::reads_an_imported_set;
        let from = r#"{"from":"saved-run"}"#;
        for path in ["/autorun-requests", "/autorun-requests-search", "/autorun-request", "/autorun-response", "/autorun-response-body"] {
            assert!(reads_an_imported_set("POST", path, from), "{path}");
            assert!(reads_an_imported_set("POST", path, r#"{"from":" saved-run "}"#), "{path} with spaces");
            assert!(!reads_an_imported_set("POST", path, ""), "{path} without a body");
            assert!(!reads_an_imported_set("POST", path, "{}"), "{path} without from");
            assert!(!reads_an_imported_set("POST", path, r#"{"from":"live"}"#), "{path} from live");
            assert!(!reads_an_imported_set("POST", path, r#"{"from":"LIVE"}"#), "{path} from LIVE");
            assert!(!reads_an_imported_set("POST", path, r#"{"from":""}"#), "{path} from blank");
            assert!(!reads_an_imported_set("GET", path, from), "{path} by GET");
        }
        // The capture routes that read no set, and every other route, keep
        // their browser.
        for path in ["/autorun-capture", "/autorun-requests-export", "/autorun-probe", "/autorun-try"] {
            assert!(!reads_an_imported_set("POST", path, from), "{path}");
        }
        // A body that cannot be read is answered 400 before any browser is
        // used, so it needs none either.
        assert!(reads_an_imported_set("POST", "/autorun-requests", "not json"));
    }

    /// `route_as` looks for it before `slot_for`, and still calls `slot_for`
    /// once.
    #[test]
    fn route_as_does_not_pick_a_slot_for_a_read_of_an_imported_set() {
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        let head = bridge.find("pub async fn route_as(").expect("route_as");
        let after = &bridge[head..];
        let route_as = &after[..after.find("\n}").expect("end of route_as")];
        let reads = route_as.find("reads_an_imported_set(method, path, body)").expect("route_as does not ask");
        assert!(reads < route_as.find("slot_for(caller,").unwrap(), "asked after slot_for");
        assert_eq!(route_as.matches("slot_for(caller,").count(), 1);
    }

    /// A caller with no browser is answered by a bridge route that reads an
    /// imported set, whoever holds main.
    #[tokio::test]
    async fn an_assistant_with_no_browser_reads_an_imported_set() {
        use v2_lib::ai_bridge::route_as;
        use v2_lib::autorun::agents::{self, AgentHeaders};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        agents::clear();
        v2_lib::commands::autorun_slots::clear();
        let register = |id: &str| {
            let h = AgentHeaders { id: Some(id.to_string()), pid: Some(4242), client: Some("claude-code".into()), name: None };
            agents::note_call(&h.caller(), &h, 10_000_000);
            h.caller()
        };
        let (holder, other) = (register(&"1".repeat(32)), register(&"2".repeat(32)));
        assert_eq!(slot_for(&holder, Need::Act), Ok(SlotNo::MAIN));
        let live = serde_json::json!({ "limit": 1 }).to_string();
        let from = serde_json::json!({ "from": "no-such-set" }).to_string();
        let ctx = v2_lib::ai_bridge::BridgeContext {
            org: "acme".into(),
            project: "Web".into(),
            network_tools: true,
            ..Default::default()
        };
        // A read of the live capture needs the caller's own browser.
        let (status, said) = route_as(&other, &ctx, None, "POST", "/autorun-requests", &live, "1.0.0").await;
        assert_eq!((status, said.as_str()), (409, v2_lib::commands::autorun_slots::NO_BROWSER_OF_YOURS));
        // A read of an imported set is answered (here: no such set), not refused.
        let (status, said) = route_as(&other, &ctx, None, "POST", "/autorun-requests", &from, "1.0.0").await;
        assert_eq!(status, 400, "{said}");
        assert!(said.contains("No imported set named"), "{said}");
        agents::clear();
        v2_lib::commands::autorun_slots::clear();
    }

    /// `route_as` picks the slot once, after the guards, and no browser route
    /// or handler of the bridge locks main's session by name any more.
    #[test]
    fn route_as_picks_the_slot_once_and_no_handler_locks_main() {
        let bridge = include_str!("../../src/ai_bridge.rs");
        let head = bridge.find("pub async fn route_as(").expect("route_as");
        let after = &bridge[head..];
        let end = after.find("\n}").expect("end of route_as");
        let route_as = &after[..end];
        assert_eq!(route_as.matches("slot_for(caller,").count(), 1, "slot_for is called once");
        let guard = route_as.find("network_guard(").expect("network guard");
        assert!(route_as.find("slot_for(caller,").unwrap() > guard, "slot_for comes after the guards");
        assert!(route_as.contains("need_of(method, path)"));
        assert!(!route_as.contains("supervised().lock()"), "route_as locks a slot, not main's session");
        // Past the router, every lock of a session names its slot.
        assert!(
            !bridge.contains("autorun::supervised()"),
            "the bridge locks session_slot(n), never supervised()"
        );
    }

    /// Each handler the brief names takes its slot and locks THAT slot's
    /// session, not main's; the assistant's commands tell the panes only
    /// when the browser is main's.
    #[test]
    fn every_browser_handler_locks_the_slot_it_was_given() {
        fn body<'a>(source: &'a str, head: &str) -> &'a str {
            let from = &source[source.find(head).unwrap_or_else(|| panic!("{head} is gone"))..];
            &from[..from.find("\n}").unwrap_or(from.len())]
        }
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        for head in [
            "async fn autorun_capture(",
            "async fn autorun_override(",
            "async fn autorun_probe(",
            "async fn autorun_inspect(",
            "async fn autorun_try(",
            "async fn autorun_discover_start(",
            "async fn autorun_discover_action(",
            "async fn autorun_discover_actions(",
            "async fn autorun_component_save(",
            "async fn autorun_discover_area(",
            "async fn record_validation_rule(",
            "async fn api_template_explore_undo(",
            "async fn record_refused_on_page(",
            "async fn run_saved_in_session(",
            "async fn supervised_page_read_in(",
        ] {
            let b = body(&bridge, head);
            assert!(b.contains("session_slot(n).lock()"), "{head} does not lock its slot's session");
            assert!(!b.contains("SESSION"), "{head} names main's session");
        }
        let commands = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
        for head in [
            "pub(crate) async fn open_for_discovery(",
            "pub(crate) async fn end_discovery(",
            "pub async fn assistant_browser_holds(",
            "pub async fn release_for_assistant(",
            "pub async fn reset_for_assistant(",
            "pub async fn clear_cookies_for_assistant(",
        ] {
            let b = body(&commands, head);
            assert!(b.contains("session_slot(n).lock()"), "{head} does not lock its slot's session");
            assert!(!b.contains("SESSION.lock()") && !b.contains("tell_panes_closed()"), "{head} acts on main");
            assert!(!b.contains("publish_discovery(") && !b.contains("publish_overrides("), "{head} publishes to main");
        }
        // The people's panes hear only the main browser.
        assert!(body(&commands, "pub(crate) fn tell_panes_closed_in(").contains("is_main()"));
        assert!(body(&commands, "pub(crate) fn tell_panes_in(").contains("is_main()"));
        // The replay-going sentences read the run kept in the slot asked about.
        for head in ["fn call_the_replay_going(", "pub fn assistant_replay_going("] {
            assert!(body(&bridge, head).contains("assistant_run(n)"), "{head} reads main's run");
        }
        // A guarded try or step answers the paused requests in the slot of
        // the session it guards, never main's.
        let guard = body(&commands, "pub(crate) async fn guard_supervised(");
        assert!(guard.contains("answer_between_commands_in(session.slot)"), "{guard}");
        assert!(!guard.contains("answer_between_commands()"), "{guard}");
    }

    /// The guide lists the rules of the area the CALLER's own discovery is
    /// exploring. A caller with no browser has none: it is never lent main's.
    #[test]
    fn the_guide_reads_only_the_callers_own_discovery_area() {
        assert_eq!(v2_lib::ai_bridge::running_discovery_area(None), None);
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        let route = &bridge[bridge.find("(\"GET\", \"/autorun-guide\")").unwrap()..];
        let route = &route[..route.find("(\"GET\", \"/autorun-role\")").unwrap()];
        assert!(route.contains("let asker = crate::commands::autorun_slots::slot_if_any(caller);"), "{route}");
        assert!(!route.contains("unwrap_or(SlotNo::MAIN)"), "a caller with no browser is lent main's area");
        assert!(bridge.contains("fn autorun_guide_with_quirks(ctx: &BridgeContext, quick: bool, n: Option<SlotNo>)"));
        let area = &bridge[bridge.find("fn running_discovery_area(").unwrap()..];
        assert!(area[..area.find("\n}").unwrap()].contains("let n = n?;"));
    }

    fn ctx() -> v2_lib::ai_bridge::BridgeContext {
        v2_lib::ai_bridge::BridgeContext { org: "acme".into(), project: "Web".into(), ..Default::default() }
    }

    /// An agent's calls land in main, and are answered word for word as the
    /// default caller's are.
    #[tokio::test]
    async fn an_agent_is_answered_as_the_default_caller_is() {
        use v2_lib::ai_bridge::{route, route_as};
        let _a = crate::serial::autorun();
        let agent = Caller::Agent(AgentKey { id: "c".repeat(32), name: Some("writer".to_string()) });
        let probe = serde_json::json!({ "selector": "#save" }).to_string();
        for (method, path, body) in [
            ("GET", "/autorun-page", ""),
            ("POST", "/autorun-probe", probe.as_str()),
            ("POST", "/autorun-release", ""),
            ("POST", "/autorun-discover-end", ""),
            ("POST", "/autorun-cookies-clear", ""),
            ("POST", "/autorun-replay-stop", ""),
        ] {
            let by_default = route(&ctx(), None, method, path, body, "1.0.0").await;
            let by_agent = route_as(&agent, &ctx(), None, method, path, body, "1.0.0").await;
            assert_eq!(by_agent, by_default, "{method} {path}");
        }
    }

    /// The assistant's commands publish what changed to the slot they were
    /// given: a release or a reset in slot 1 never clears what is published
    /// for main.
    #[tokio::test]
    async fn the_assistants_commands_publish_to_their_own_slot() {
        use v2_lib::commands::autorun::{
            clear_cookies_for_assistant, release_for_assistant, reset_for_assistant, NO_BROWSER_OPEN,
        };
        let _a = crate::serial::autorun();
        let one = SlotNo(1);
        show_overrides(&changed());
        show_overrides_in(one, &changed());

        let (status, said) = release_for_assistant(one).await;
        assert_eq!(status, 200, "{said}");
        assert_eq!(overrides_now_in(one), Overrides::new(), "slot 1's release left its changes published");
        assert_eq!(overrides_now(), changed(), "slot 1's release unpublished main's changes");

        show_overrides_in(one, &changed());
        let (status, said) = clear_cookies_for_assistant(one).await;
        assert_eq!((status, said.as_str()), (404, NO_BROWSER_OPEN), "no browser is open in slot 1");
        let (status, said) = reset_for_assistant(one, "acme", "Web").await;
        assert_eq!((status, said.as_str()), (404, NO_BROWSER_OPEN));
        assert_eq!(overrides_now(), changed(), "a command of slot 1 reached main's published changes");

        show_overrides(&Overrides::new());
        show_overrides_in(one, &Overrides::new());
    }
}

// ---------------------------------------------------------------------------
// The Assistants card: bindings, views and commands (Task 10)
// ---------------------------------------------------------------------------

mod card {
    use super::*;
    use v2_lib::autorun::agents::{self, AgentHeaders, AgentKey, Caller, Sweep};
    use v2_lib::autorun::replay_to::{state, OneReplay};
    use v2_lib::commands::autorun_slots::{
        agent_views, auto_run_agents, auto_run_end_agent_discovery, auto_run_release_agent, auto_run_stop_agent,
        bind, bind_if_free, bound, clear, hold_binding, next_views_to_send, on_sweep, slot_for, slot_for_with,
        slot_if_any, slot_of, unbind, unbind_agent, window_position, AgentActivity, Need, ALL_BROWSERS_OPEN,
        NO_BROWSER_OF_YOURS, NO_SUCH_BROWSER,
    };

    const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const T0: u64 = 10_000_000;
    fn key(id: &str) -> AgentKey {
        AgentKey { id: id.to_string(), name: None }
    }

    fn register(id: &str, client: &str, now: u64) -> AgentKey {
        let h = AgentHeaders {
            id: Some(id.to_string()),
            pid: Some(4242),
            client: Some(client.to_string()),
            name: None,
        };
        agents::note_call(&h.caller(), &h, now);
        key(id)
    }

    fn fresh() {
        agents::clear();
        clear();
        for n in SlotNo::all() {
            state(n).cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            v2_lib::ai_bridge::batch_stop(n).store(false, std::sync::atomic::Ordering::SeqCst);
        }
        show_overrides(&Overrides::new());
    }

    const ID_C: &str = "cccccccccccccccccccccccccccccccc";
    const ID_D: &str = "dddddddddddddddddddddddddddddddd";

    fn caller(id: &str) -> Caller {
        Caller::Agent(key(id))
    }

    /// One assistant alone works exactly as it did before there were extra
    /// browsers: every call lands in main, and an Open or an Act binds it.
    #[test]
    fn one_agent_alone_always_lands_in_main_and_binds_it() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        assert_eq!(slot_for(&caller(ID_A), Need::Read), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), None, "a read binds nothing");
        assert_eq!(slot_for(&caller(ID_A), Need::Act), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)));
        for need in [Need::Open, Need::Act, Need::Read] {
            assert_eq!(slot_for(&caller(ID_A), need), Ok(SlotNo::MAIN), "{need:?}");
        }
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)));
        assert_eq!(slot_if_any(&caller(ID_A)), Some(SlotNo::MAIN));
        fresh();
    }

    /// An older proxy, a test and the person are never refused and never
    /// moved by another assistant's claim.
    #[test]
    fn the_default_caller_always_gets_main_even_when_main_is_bound() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &key(ID_A));
        bind(SlotNo(1), &key(ID_B));
        for need in [Need::Open, Need::Act, Need::Read] {
            assert_eq!(slot_for(&Caller::Default, need), Ok(SlotNo::MAIN), "{need:?}");
        }
        assert_eq!(slot_if_any(&Caller::Default), Some(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)), "the default caller changed main's binding");
        assert_eq!(bound(SlotNo(1)), Some(key(ID_B)));
        fresh();
    }

    /// Main is held by a live assistant: the next to open gets the lowest
    /// extra browser with nobody in it, and the fourth is told there are no
    /// more.
    #[test]
    fn a_second_agent_opening_gets_slot_1_then_slot_2_then_the_cap() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        for id in [ID_A, ID_B, ID_C, ID_D] {
            register(id, "claude-code", T0);
        }
        assert_eq!(slot_for(&caller(ID_A), Need::Act), Ok(SlotNo::MAIN));
        assert_eq!(slot_for(&caller(ID_B), Need::Open), Ok(SlotNo(1)));
        assert_eq!(bound(SlotNo(1)), Some(key(ID_B)), "the slot is bound in the same step");
        assert_eq!(slot_for(&caller(ID_C), Need::Open), Ok(SlotNo(2)));
        assert_eq!(slot_for(&caller(ID_D), Need::Open), Err((409, ALL_BROWSERS_OPEN.to_string())));
        assert_eq!(slot_of(&key(ID_D)), None, "a refused open binds nothing");
        // Each keeps its own browser on every later call.
        assert_eq!(slot_for(&caller(ID_B), Need::Read), Ok(SlotNo(1)));
        assert_eq!(slot_for(&caller(ID_B), Need::Act), Ok(SlotNo(1)));
        assert_eq!(slot_for(&caller(ID_B), Need::Open), Ok(SlotNo(1)));
        assert_eq!(slot_for(&caller(ID_C), Need::Act), Ok(SlotNo(2)));
        assert_eq!(slot_for(&caller(ID_A), Need::Read), Ok(SlotNo::MAIN));
        // A freed extra browser is the next to open's.
        unbind(SlotNo(1));
        assert_eq!(slot_for(&caller(ID_D), Need::Open), Ok(SlotNo(1)));
        assert_eq!(slot_if_any(&caller(ID_D)), Some(SlotNo(1)));
        fresh();
    }

    /// An assistant that only acts or reads, with main held by another and
    /// no browser of its own, is told how to get one.
    #[test]
    fn a_second_agent_without_a_browser_is_told_how_to_get_one() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &key(ID_A));
        for need in [Need::Read, Need::Act] {
            assert_eq!(slot_for(&caller(ID_B), need), Err((409, NO_BROWSER_OF_YOURS.to_string())), "{need:?}");
        }
        assert_eq!(slot_of(&key(ID_B)), None, "a refusal binds nothing");
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)), "main's holder was moved");
        // The routes that use a browser only when the caller has one are
        // refused nothing: they just find none.
        assert_eq!(slot_if_any(&caller(ID_B)), None);
        assert!(!NO_BROWSER_OF_YOURS.contains(['\u{2013}', '\u{2014}']));
        assert!(!ALL_BROWSERS_OPEN.contains(['\u{2013}', '\u{2014}']));
        assert!(ALL_BROWSERS_OPEN.starts_with("3 Auto Run browsers are open already"));
        fresh();
    }

    /// A subagent that leaves out its `agent` name is the session it was
    /// started by: the same id and no name is one agent, one browser. A
    /// named subagent is another agent.
    #[test]
    fn two_calls_with_the_same_id_and_name_share_one_slot() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &key(ID_A));
        let first = Caller::Agent(AgentKey { id: ID_B.to_string(), name: None });
        let second = Caller::Agent(AgentKey { id: ID_B.to_string(), name: None });
        assert_eq!(slot_for(&first, Need::Open), Ok(SlotNo(1)));
        assert_eq!(slot_for(&second, Need::Open), Ok(SlotNo(1)), "the same agent was given another browser");
        assert_eq!(slot_for(&second, Need::Act), Ok(SlotNo(1)));
        assert_eq!(slot_for(&second, Need::Read), Ok(SlotNo(1)));
        // The same id with a name is a subagent of its own.
        let named = AgentKey { id: ID_B.to_string(), name: Some("writer".to_string()) };
        agents::note_call(
            &Caller::Agent(named.clone()),
            &AgentHeaders { id: Some(ID_B.to_string()), pid: None, client: None, name: Some("writer".to_string()) },
            T0,
        );
        assert_eq!(slot_for(&Caller::Agent(named), Need::Open), Ok(SlotNo(2)));
        fresh();
    }

    /// An assistant that has gone does not keep main from the next.
    #[test]
    fn a_gone_main_binding_does_not_block_another_agent() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &key(ID_A));
        agents::note_bye(ID_A, T0 + 1);
        assert_eq!(slot_if_any(&caller(ID_B)), Some(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)), "a look binds nothing");
        assert_eq!(slot_for(&caller(ID_B), Need::Act), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_B)));
        assert_eq!(slot_of(&key(ID_A)), None);
        fresh();
    }

    /// Main is held by an assistant the app cannot see (never registered, or
    /// forgotten): the next takes it, as before there were extra browsers.
    #[test]
    fn main_held_by_an_unlisted_agent_is_taken_by_the_next() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &key(ID_A));
        assert_eq!(slot_for(&caller(ID_B), Need::Open), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_B)));
        fresh();
    }

    #[test]
    fn extra_windows_open_offset() {
        assert_eq!(window_position(SlotNo::MAIN), None);
        assert_eq!(window_position(SlotNo(1)), Some((60, 60)));
        assert_eq!(window_position(SlotNo(2)), Some((120, 120)));
    }

    /// An open that replaces the browser an assistant holds keeps its
    /// binding the whole time: another assistant's call cannot take main in
    /// the gap, and the slot is the holder's again when the open ends.
    #[test]
    fn a_replacing_open_keeps_the_holders_binding() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &key(ID_A));
        {
            let _hold = hold_binding(SlotNo::MAIN, &key(ID_A));
            // The browser being replaced is closed: its close unbinds the slot.
            unbind(SlotNo::MAIN);
            assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)), "the close unbound the holder");
            assert_eq!(
                slot_for(&caller(ID_B), Need::Act),
                Err((409, NO_BROWSER_OF_YOURS.to_string())),
                "another assistant took main in the gap"
            );
            // Another slot's binding is not held.
            bind(SlotNo(1), &key(ID_B));
            unbind(SlotNo(1));
            assert_eq!(bound(SlotNo(1)), None);
        }
        // Once the open is over an unbind is an unbind again.
        unbind(SlotNo::MAIN);
        assert_eq!(bound(SlotNo::MAIN), None);
        assert_eq!(slot_for(&caller(ID_B), Need::Act), Ok(SlotNo::MAIN));
        // A hold ends however its holder returns.
        bind(SlotNo::MAIN, &key(ID_A));
        let hold = hold_binding(SlotNo::MAIN, &key(ID_A));
        drop(hold);
        unbind(SlotNo::MAIN);
        assert_eq!(bound(SlotNo::MAIN), None);
        fresh();
    }

    /// Binding a slot to its holder after an open does nothing when another
    /// assistant has it by then.
    #[test]
    fn bind_if_free_binds_only_an_unbound_slot() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        register(ID_A, "claude-code", T0);
        register(ID_B, "claude-code", T0);
        bind_if_free(SlotNo(1), &key(ID_A));
        assert_eq!(bound(SlotNo(1)), Some(key(ID_A)));
        bind_if_free(SlotNo(1), &key(ID_B));
        assert_eq!(bound(SlotNo(1)), Some(key(ID_A)), "an assistant's slot was taken");
        fresh();
    }

    /// An assistant the sweeper unbound while its open ran (or that said
    /// goodbye) is not bound again by the open's end: nothing would free
    /// that slot afterwards.
    #[test]
    fn bind_if_free_never_binds_a_gone_assistant() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        // Never listed: not live.
        bind_if_free(SlotNo(1), &key(ID_A));
        assert_eq!(bound(SlotNo(1)), None, "an unlisted assistant was bound");
        // Listed, then gone.
        register(ID_B, "claude-code", T0);
        agents::note_bye(ID_B, T0 + 1);
        bind_if_free(SlotNo(2), &key(ID_B));
        assert_eq!(bound(SlotNo(2)), None, "a gone assistant was bound");
        // Live: bound.
        register(ID_C, "claude-code", T0);
        bind_if_free(SlotNo(2), &key(ID_C));
        assert_eq!(bound(SlotNo(2)), Some(key(ID_C)));
        fresh();
    }

    /// Rule 4 gives out an extra browser only when it has no session: one
    /// whose assistant was unbound while its browser stayed open is not free.
    /// The emptiness is the pick's input, so this fails if it is ignored.
    #[test]
    fn rule_4_skips_an_extra_browser_that_is_not_empty() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        for id in [ID_A, ID_B, ID_C, ID_D] {
            register(id, "claude-code", T0);
        }
        bind(SlotNo::MAIN, &key(ID_A));
        let slot_1_has_a_browser = |n: SlotNo| n != SlotNo(1);
        assert_eq!(slot_for_with(&caller(ID_B), Need::Open, &slot_1_has_a_browser), Ok(SlotNo(2)));
        assert_eq!(bound(SlotNo(1)), None, "a slot with a browser in it was bound");
        assert_eq!(
            slot_for_with(&caller(ID_C), Need::Open, &slot_1_has_a_browser),
            Err((409, ALL_BROWSERS_OPEN.to_string())),
            "the only empty slot was taken already"
        );
        // Neither slot empty: nothing to give.
        unbind(SlotNo(2));
        assert_eq!(slot_for_with(&caller(ID_D), Need::Open, &|_| false), Err((409, ALL_BROWSERS_OPEN.to_string())));
        // A read asks nothing of the slots' emptiness.
        assert_eq!(slot_for_with(&caller(ID_D), Need::Read, &|_| true), Err((409, NO_BROWSER_OF_YOURS.to_string())));
        // Both empty: the lowest.
        assert_eq!(slot_for_with(&caller(ID_D), Need::Open, &|_| true), Ok(SlotNo(1)));
        // `slot_for` asks the slots themselves.
        let src = include_str!("../../src/commands/autorun_slots.rs").replace("\r\n", "\n");
        let head = &src[src.find("pub fn slot_for(").unwrap()..];
        assert!(head[..head.find("\n}\n").unwrap()].contains("slot_for_with(caller, need, &slot_is_empty)"));
        fresh();
    }

    /// An Open that is refused or fails after it bound an extra browser to
    /// its assistant gives the browser back while it is still empty, so it is
    /// never left reserved and a fourth assistant is not told all three are
    /// open.
    #[tokio::test]
    async fn a_refused_open_frees_the_extra_browser_it_reserved() {
        use v2_lib::ai_bridge::{route_as, BridgeContext};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        for id in [ID_A, ID_B, ID_C] {
            register(id, "claude-code", T0);
        }
        assert_eq!(slot_for(&caller(ID_A), Need::Act), Ok(SlotNo::MAIN));
        let ctx = BridgeContext { org: "acme".into(), project: "Web".into(), ..Default::default() };
        let account = serde_json::json!({ "account": "no-such-account" }).to_string();
        let case = serde_json::json!({ "case_id": 1 }).to_string();
        for (path, body) in [
            ("/autorun-discover-start", "not json"),
            ("/autorun-discover-start", account.as_str()),
            ("/autorun-replay", "not json"),
            ("/autorun-replay", case.as_str()),
        ] {
            let (status, said) = route_as(&caller(ID_B), &ctx, None, "POST", path, body, "1.0.0").await;
            assert!(status >= 400, "{path} {body}: {status} {said}");
            assert_eq!(bound(SlotNo(1)), None, "{path} {body}: the refused Open left slot 1 reserved");
            assert_eq!(slot_of(&key(ID_B)), None);
            // And it is free for the next to open.
            assert_eq!(slot_for(&caller(ID_C), Need::Open), Ok(SlotNo(1)), "{path} {body}");
            unbind(SlotNo(1));
        }
        // Main is A's, never freed this way.
        assert_eq!(bound(SlotNo::MAIN), Some(key(ID_A)));
        fresh();
    }

    /// The Open arms hand the answer to `free_extra_if_unused`, which looks
    /// at an answer of 400 or more only and frees only an extra slot with
    /// nothing open in it.
    #[test]
    fn the_open_arms_free_an_unused_extra_browser_on_a_failure() {
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        for arm in ["(\"POST\", \"/autorun-discover-start\")", "(\"POST\", \"/autorun-replay\")"] {
            let from = &bridge[bridge.find(arm).unwrap()..];
            let arm_text = &from[..from.find("\n        }\n").unwrap()];
            assert!(arm_text.contains("free_extra_if_unused(slot, holder"), "{arm}: {arm_text}");
        }
        let src = include_str!("../../src/commands/autorun_slots.rs").replace("\r\n", "\n");
        let f = &src[src.find("pub fn free_extra_if_unused(").unwrap()..];
        let f = &f[..f.find("\n}\n").unwrap()];
        assert!(f.contains("status < 400") && f.contains("is_main()") && f.contains("slot_is_empty(n)"), "{f}");
    }

    /// The hold ends as soon as the open is over: a browser that closes
    /// during the replay, or at its end, leaves the slot unbound.
    #[test]
    fn the_replays_binding_hold_ends_with_its_open() {
        let src = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
        let head = &src[src.find("pub(crate) async fn replay_supervised(").unwrap()..];
        let replay = &head[..head.find("\n}\n").unwrap()];
        let bound_at = replay.find("bind_if_free(n, key);").expect("the holder is bound after the open");
        let drop_at = replay.find("drop(hold);").expect("the hold is never dropped early");
        assert!(drop_at > bound_at, "the hold ends before the holder is bound");
        let between = &replay[bound_at..drop_at];
        assert!(!between.contains("let_go_if_gone_in") && !between.contains("close_browser_in"), "{between}");
        // The panes are told, the replay runs and the browser is let go after it.
        assert!(drop_at < replay.find("emit_if_main(n, app, changed)").unwrap());
        assert!(drop_at < replay.find("replay_to::replay_to_traced(").unwrap());
        assert!(drop_at < replay.find("_ => let_go_if_gone_in(n, &mut slot).await,").unwrap());
        // A hold dropped is an unbind that works again.
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        bind(SlotNo(1), &key(ID_A));
        let hold = hold_binding(SlotNo(1), &key(ID_A));
        unbind(SlotNo(1));
        assert_eq!(bound(SlotNo(1)), Some(key(ID_A)));
        drop(hold);
        unbind(SlotNo(1));
        assert_eq!(bound(SlotNo(1)), None);
        fresh();
    }

    /// The paths that open a browser for an assistant take the assistant's
    /// real key from `route_as`, hold its binding through the replace, and
    /// never read the holder back from the bindings.
    #[test]
    fn the_open_paths_are_given_the_real_caller() {
        fn body<'a>(source: &'a str, head: &str) -> &'a str {
            let from = &source[source.find(head).unwrap_or_else(|| panic!("{head} is gone"))..];
            &from[..from.find("\n}\n").unwrap_or(from.len())]
        }
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        let commands = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
        let route_as = body(&bridge, "pub async fn route_as(");
        assert!(route_as.contains("let holder = caller.key();"), "{route_as}");
        assert!(route_as.contains("autorun_discover_start(ctx, body, &mut seen, slot, holder)"), "{route_as}");
        assert!(route_as.contains("autorun_replay(ctx, body, slot, holder)"), "{route_as}");
        assert!(body(&bridge, "async fn autorun_discover_start(").contains("holder: Option<&AgentKey>"));
        assert!(body(&bridge, "async fn autorun_discover_start(").contains(", n, holder)"));
        assert!(body(&bridge, "pub trait ReplayHost").contains("holder: Option<AgentKey>"));
        let host = include_str!("../../src/commands/ai_bridge.rs").replace("\r\n", "\n");
        assert!(host.contains("ReplayBy::Assistant, holder.as_ref())"), "the host does not pass the holder on");
        for head in ["pub(crate) async fn open_for_discovery(", "pub(crate) async fn replay_supervised("] {
            let b = body(&commands, head);
            assert!(b.contains("holder: Option<&crate::autorun::agents::AgentKey>"), "{head} takes no holder");
            assert!(b.contains("hold_binding(n, key)"), "{head} does not hold the binding");
            assert!(b.contains("bind_if_free(n, key)"), "{head} does not bind the holder after the open");
            assert!(!b.contains("= crate::commands::autorun_slots::bound(n)"), "{head} reads the holder from the bindings");
        }
        let into = body(&commands, "async fn open_if_none(");
        assert!(!into.contains("bound(n)") && !into.contains("rebind"), "open_if_none guesses the holder: {into}");
        assert!(!commands.contains("fn rebind_if_free("), "the old rebind is back");
    }

    /// An assistant's extra browser opens offset from the main window.
    #[test]
    fn an_opened_browser_takes_its_slots_window_position() {
        let commands = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
        let from = &commands[commands.find("async fn open_into(").unwrap()..];
        let open = &from[..from.find("\n}\n").unwrap()];
        assert!(
            open.contains("launch_in_at(which, crate::commands::autorun_slots::window_position(n))"),
            "{open}"
        );
        assert!(!open.contains("launch_in(which)"), "{open}");
    }

    /// The pane events of a replay are the main browser's: an extra browser
    /// opening, signing in, closing or stepping never tells the person's
    /// panes anything.
    #[test]
    fn a_replay_in_an_extra_slot_tells_the_panes_nothing() {
        fn body<'a>(source: &'a str, head: &str) -> &'a str {
            let from = &source[source.find(head).unwrap_or_else(|| panic!("{head} is gone"))..];
            &from[..from.find("\n}\n").unwrap_or(from.len())]
        }
        let commands = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
        for head in ["pub(crate) async fn replay_supervised(", "async fn open_into(", "async fn open_if_none("] {
            let b = body(&commands, head);
            // Every event is sent through `emit_if_main`; none is emitted bare.
            assert!(!b.contains(".emit(app)"), "{head} emits a pane event for any slot:\n{b}");
        }
        let replay = body(&commands, "pub(crate) async fn replay_supervised(");
        assert_eq!(replay.matches("emit_if_main(n, app, ").count(), 6, "the six pane events of a replay: {replay}");
        let helper = body(&commands, "fn emit_if_main<");
        assert!(helper.contains("if n.is_main()"), "{helper}");
    }

    #[test]
    fn an_agent_that_opens_or_acts_binds_main_and_a_read_does_not() {
        // These agents are never registered, so none of them is live and the
        // latest to open or act takes main (rule 3). Registered agents are
        // held apart (the rules tests above).
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let (a, b) = (key(ID_A), key(ID_B));
        assert_eq!(slot_for(&Caller::Agent(a.clone()), Need::Read), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), None, "a read binds nothing");
        assert_eq!(slot_for(&Caller::Agent(a.clone()), Need::Act), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(a.clone()));
        assert_eq!(slot_of(&a), Some(SlotNo::MAIN));
        assert_eq!(slot_for(&Caller::Agent(b.clone()), Need::Open), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(b.clone()), "the latest to open or act holds main");
        assert_eq!(slot_of(&a), None, "an agent is bound to one slot at most");
        assert_eq!(slot_for(&Caller::Default, Need::Act), Ok(SlotNo::MAIN));
        assert_eq!(bound(SlotNo::MAIN), Some(b), "the default caller changes no binding");
        assert_eq!(slot_for(&Caller::Agent(a.clone()), Need::Read), Ok(SlotNo::MAIN));
        assert_ne!(bound(SlotNo::MAIN), Some(a), "a read does not take main over");
        fresh();
    }

    #[test]
    fn bind_moves_an_agent_and_unbind_clears_a_slot() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let (a, b) = (key(ID_A), key(ID_B));
        bind(SlotNo(1), &a);
        bind(SlotNo::MAIN, &a);
        assert_eq!((bound(SlotNo::MAIN), bound(SlotNo(1))), (Some(a.clone()), None), "bind unbinds it elsewhere");
        bind(SlotNo(1), &b);
        unbind(SlotNo(1));
        assert_eq!(bound(SlotNo(1)), None);
        unbind_agent(&a);
        assert_eq!(bound(SlotNo::MAIN), None);
        assert_eq!(slot_of(&a), None);
        fresh();
    }

    #[tokio::test]
    async fn on_sweep_unbinds_an_agent_past_grace() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = key(ID_A);
        bind(SlotNo::MAIN, &a);
        on_sweep(&Sweep { newly_gone: vec![a.clone()], ..Sweep::default() }).await;
        assert_eq!(bound(SlotNo::MAIN), Some(a.clone()), "gone is not yet past its grace");
        on_sweep(&Sweep { past_grace: vec![a.clone()], ..Sweep::default() }).await;
        assert_eq!(bound(SlotNo::MAIN), None);
        fresh();
    }

    /// Review focus 3: a session closed mid-replay (its proxy killed, no
    /// goodbye). Past its grace, its extra browser's replay is stopped through
    /// that slot's own cancel, the slot is released and unbound, and its
    /// claims end. The log names the label and says the session ended.
    #[tokio::test]
    async fn a_gone_agent_after_grace_releases_its_extra_slot_and_stops_its_replay() {
        use v2_lib::autorun::claims;
        let logged = || v2_lib::applog::recent(2000).into_iter().map(|l| l.message).collect::<Vec<_>>().join("\n");
        let _g = crate::serial::agents();
        let _l = crate::serial::log_tail();
        let _a = crate::serial::autorun();
        fresh();
        claims::clear();
        register(ID_A, "claude-code", T0);
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        claims::claim(&b, 4521, T0).unwrap();
        let replay = OneReplay::claim_for_assistant_in(SlotNo(1)).expect("slot 1 is free");
        let cancelled = |n: SlotNo| state(n).cancel.load(std::sync::atomic::Ordering::SeqCst);

        on_sweep(&Sweep { newly_gone: vec![b.clone()], ..Sweep::default() }).await;
        assert_eq!(bound(SlotNo(1)), Some(b.clone()), "gone is not yet past its grace");
        assert!(!cancelled(SlotNo(1)));

        // A mark in the log tail: only what the sweep wrote after it is read,
        // so lines other tests left behind cannot decide the result.
        let mark = "autorun_slots test: the past-grace sweep starts here";
        v2_lib::applog::info(mark);
        on_sweep(&Sweep { past_grace: vec![b.clone()], ..Sweep::default() }).await;
        assert!(cancelled(SlotNo(1)), "its replay was stopped through its own slot's cancel");
        assert!(!cancelled(SlotNo::MAIN), "main's replay was not touched");
        assert_eq!(bound(SlotNo(1)), None);
        assert_eq!(claims::holder(4521, T0), None);
        let lines = logged();
        let after = &lines[lines.rfind(mark).expect("the mark is in the log tail")..];
        assert!(after.contains("Auto Run: released Assistant 2's browser (its session ended)"), "{after}");
        assert!(!after.contains("(the person released it)"), "the sweep's release was logged as the person's: {after}");
        assert!(!after.contains(ID_B) && !after.contains("4242"), "{after}");
        drop(replay);
        claims::clear();
        fresh();
    }

    /// A main binding is only cleared: the browser stays as it is, the
    /// replay in it is not stopped, and its claims end.
    #[tokio::test]
    async fn a_gone_agent_bound_to_main_is_only_unbound() {
        use v2_lib::autorun::claims;
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        claims::clear();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        claims::claim(&a, 4521, T0).unwrap();
        let replay = OneReplay::claim_for_assistant().expect("main is free");
        on_sweep(&Sweep { past_grace: vec![a.clone()], ..Sweep::default() }).await;
        assert_eq!(bound(SlotNo::MAIN), None);
        assert!(!state(SlotNo::MAIN).cancel.load(std::sync::atomic::Ordering::SeqCst), "main's browser is left alone");
        assert_eq!(claims::holder(4521, T0), None);
        drop(replay);
        claims::clear();
        fresh();
    }

    /// A release that found the browser busy is asked again on each later
    /// sweep (past grace is reported once), until it succeeds.
    #[tokio::test]
    async fn a_release_that_found_the_browser_busy_is_tried_again() {
        use v2_lib::commands::autorun::try_hold_slot;
        use v2_lib::commands::autorun_slots::on_sweep_within;
        let short = std::time::Duration::from_millis(40);
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        let busy = try_hold_slot(SlotNo(1)).expect("slot 1 is free");
        on_sweep_within(&Sweep { past_grace: vec![b.clone()], ..Sweep::default() }, short).await;
        assert_eq!(bound(SlotNo(1)), Some(b.clone()), "a busy browser keeps its binding");
        on_sweep_within(&Sweep::default(), short).await;
        assert_eq!(bound(SlotNo(1)), Some(b.clone()), "still busy, still kept");
        drop(busy);
        on_sweep_within(&Sweep::default(), short).await;
        assert_eq!(bound(SlotNo(1)), None, "the next sweep releases it");
        fresh();
    }

    /// A retry is given up when the slot is no longer bound to the gone
    /// agent: someone else's browser is never released for it.
    #[tokio::test]
    async fn a_retry_is_dropped_once_the_slot_is_someone_elses() {
        use v2_lib::commands::autorun::try_hold_slot;
        use v2_lib::commands::autorun_slots::on_sweep_within;
        let short = std::time::Duration::from_millis(40);
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        let c = register(ID_C, "claude-code", T0);
        bind(SlotNo(1), &b);
        let busy = try_hold_slot(SlotNo(1)).expect("slot 1 is free");
        on_sweep_within(&Sweep { past_grace: vec![b.clone()], ..Sweep::default() }, short).await;
        drop(busy);
        bind(SlotNo(1), &c);
        on_sweep_within(&Sweep::default(), short).await;
        assert_eq!(bound(SlotNo(1)), Some(c.clone()), "the new holder keeps its browser");
        // And the retry list is spent: a later sweep does nothing either.
        on_sweep_within(&Sweep::default(), short).await;
        assert_eq!(bound(SlotNo(1)), Some(c));
        fresh();
    }

    /// The writing jobs of an agent past its grace are forgotten exactly as
    /// its goodbye forgets them. (A unit test would take the writing-job
    /// lock beside the registry's, which `serial.rs` forbids.)
    #[test]
    fn a_gone_agents_writing_jobs_are_forgotten_as_its_goodbye_does() {
        let slots = include_str!("../../src/commands/autorun_slots.rs").replace("\r\n", "\n");
        let head = slots.find("pub async fn on_sweep_within(").expect("on_sweep_within");
        let body = &slots[head..];
        let body = &body[..body.find("\n}\n").unwrap()];
        assert!(body.contains("crate::ai_bridge::forget_writing_jobs_of(&key.id)"), "{body}");
        let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
        assert!(bridge.contains("pub fn forget_writing_jobs_of(agent_id: &str)"));
        let bye = &bridge[bridge.find("(\"POST\", \"/agent-bye\")").unwrap()..];
        assert!(bye[..400].contains("forget_writing_jobs_of(&key.id)"), "the goodbye still forgets them");
    }

    /// A quiet assistant (no call for an hour, no session seen to end) loses
    /// its binding of the main browser only: an extra browser it holds is
    /// the idle release's to deal with, not this sweep's.
    #[tokio::test]
    async fn on_sweep_unbinds_a_quiet_assistant_from_main_only() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let (a, b) = (key(ID_A), key(ID_B));
        bind(SlotNo::MAIN, &a);
        bind(SlotNo(1), &b);
        on_sweep(&Sweep { quiet: vec![a.clone(), b.clone()], ..Sweep::default() }).await;
        assert_eq!(bound(SlotNo::MAIN), None, "a quiet assistant does not hold main");
        assert_eq!(bound(SlotNo(1)), Some(b), "an extra browser's binding is left to the idle release");
        // Another assistant's quiet leaves main's holder alone.
        bind(SlotNo::MAIN, &a);
        on_sweep(&Sweep { quiet: vec![key(ID_B)], ..Sweep::default() }).await;
        assert_eq!(bound(SlotNo::MAIN), Some(a));
        fresh();
    }

    /// The sweeper logs a session ending only for `newly_gone`, never for a
    /// quiet assistant.
    #[test]
    fn a_quiet_assistant_is_not_logged_as_an_ended_session() {
        let sweeper = include_str!("../../src/commands/ai_bridge.rs");
        let head = sweeper.find("pub fn spawn_agent_sweeper()").expect("sweeper");
        let body = &sweeper[head..head + 1800];
        assert!(body.contains("swept.newly_gone"), "{body}");
        assert!(!body.contains("swept.quiet"), "{body}");
    }

    // -----------------------------------------------------------------------
    // Idle release (Task 20)
    // -----------------------------------------------------------------------

    /// Registered without a pid, so the sweep can call it quiet.
    fn register_unaskable(id: &str, now: u64) -> AgentKey {
        let h = AgentHeaders { id: Some(id.to_string()), pid: None, client: Some("claude-code".to_string()), name: None };
        agents::note_call(&h.caller(), &h, now);
        key(id)
    }

    /// An extra browser whose assistant made no call for 30 minutes is idle;
    /// one millisecond less is not. A replay or a batch going in it is never idle.
    #[test]
    fn an_extra_slot_idle_30_minutes_is_released() {
        use v2_lib::ai_bridge::BatchGoing;
        use v2_lib::commands::autorun_slots::{idle_extras, IDLE_RELEASE_MS};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        assert_eq!(IDLE_RELEASE_MS, 1_800_000);
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS - 1), vec![], "just under 30 minutes is not idle");
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![SlotNo(1)]);
        {
            let _replay = OneReplay::claim_in(SlotNo(1)).expect("slot 1 is free");
            assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![], "a replay is going");
        }
        {
            let _replay = OneReplay::claim_for_assistant_in(SlotNo(1)).expect("slot 1 is free");
            assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![], "an assistant's replay is going");
        }
        {
            let _batch = BatchGoing::begin(SlotNo(1));
            assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![], "a batch is going");
        }
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![SlotNo(1)], "idle again once they end");
        // A call after the 30 minutes starts the count again.
        register(ID_B, "claude-code", T0 + IDLE_RELEASE_MS);
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![]);
        fresh();
    }

    /// Only the slot of the idle assistant is named, and an unbound extra slot
    /// is nobody's to release.
    #[test]
    fn idleness_is_judged_per_slot_by_its_assistants_last_call() {
        use v2_lib::commands::autorun_slots::{idle_extras, IDLE_RELEASE_MS};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        let c = register(ID_C, "claude-code", T0 + IDLE_RELEASE_MS);
        bind(SlotNo(1), &b);
        bind(SlotNo(2), &c);
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![SlotNo(1)], "C called just now");
        unbind(SlotNo(1));
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![], "an unbound slot has no assistant");
        fresh();
    }

    /// The person's browser (main) is never let go for idleness.
    #[test]
    fn main_is_never_released_for_idleness() {
        use v2_lib::commands::autorun_slots::idle_extras;
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        assert_eq!(idle_extras(T0 + 3_600_000), vec![]);
        assert_eq!(idle_extras(T0 + 10 * 3_600_000), vec![]);
        fresh();
    }

    /// An assistant the sweep calls quiet (an hour without a call, nothing to
    /// ask about it) is not live, yet its extra browser is still released.
    #[test]
    fn a_quiet_assistants_extra_browser_is_still_released() {
        use v2_lib::commands::autorun_slots::idle_extras;
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register_unaskable(ID_B, T0);
        bind(SlotNo(1), &b);
        let swept = agents::sweep(T0 + agents::QUIET_AFTER_MS, &|_| true);
        assert_eq!(swept.quiet, vec![b.clone()], "B is reported quiet");
        assert!(!agents::is_live(&b));
        assert_eq!(idle_extras(T0 + agents::QUIET_AFTER_MS), vec![SlotNo(1)]);
        fresh();
    }

    /// An assistant the registry has forgotten (a machine that slept past
    /// every grace) holds nothing: its binding counts as idle.
    #[test]
    fn a_forgotten_assistants_binding_counts_as_idle() {
        use v2_lib::commands::autorun_slots::idle_extras;
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        bind(SlotNo(2), &key(ID_D));
        assert_eq!(agents::info(&key(ID_D)), None);
        assert_eq!(idle_extras(T0), vec![SlotNo(2)]);
        fresh();
    }

    /// The release for idleness lets the browser go, says why once to the
    /// assistant's next Act or Read, then the usual rules apply. The log names
    /// the label and the reason, never an id.
    #[tokio::test]
    async fn the_next_call_hears_why() {
        use v2_lib::commands::autorun_slots::{release_slot, ReleaseWhy, IDLE_RELEASED};
        let logged = || v2_lib::applog::recent(2000).into_iter().map(|l| l.message).collect::<Vec<_>>().join("\n");
        let _g = crate::serial::agents();
        let _l = crate::serial::log_tail();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        bind(SlotNo(1), &b);
        let mark = "autorun_slots test: the idle release starts here";
        v2_lib::applog::info(mark);
        release_slot(SlotNo(1), ReleaseWhy::Idle).await.expect("the release");
        assert_eq!(bound(SlotNo(1)), None);
        assert_eq!(slot_for(&caller(ID_B), Need::Read), Err((409, IDLE_RELEASED.to_string())));
        assert_eq!(slot_for(&caller(ID_B), Need::Read), Err((409, NO_BROWSER_OF_YOURS.to_string())));
        assert_eq!(slot_for(&caller(ID_B), Need::Act), Err((409, NO_BROWSER_OF_YOURS.to_string())));
        let lines = logged();
        let after = &lines[lines.rfind(mark).expect("the mark is in the log tail")..];
        assert!(after.contains("Auto Run: released Assistant 2's browser (30 minutes without a call)"), "{after}");
        assert!(!after.contains(ID_B) && !after.contains("4242"), "{after}");
        assert!(!IDLE_RELEASED.contains('\u{2013}') && !IDLE_RELEASED.contains('\u{2014}'));
        fresh();
    }

    /// An Act hears it too, and an Open drops the notice and opens as usual.
    /// The default caller never has one.
    #[tokio::test]
    async fn an_open_after_the_release_opens_as_usual() {
        use v2_lib::commands::autorun_slots::{release_slot, take_idle_notice, ReleaseWhy, IDLE_RELEASED};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        bind(SlotNo(1), &b);
        release_slot(SlotNo(1), ReleaseWhy::Idle).await.expect("the release");
        assert_eq!(slot_for(&caller(ID_B), Need::Act), Err((409, IDLE_RELEASED.to_string())));
        assert!(!take_idle_notice(&b), "the notice was heard once");

        bind(SlotNo(1), &b);
        release_slot(SlotNo(1), ReleaseWhy::Idle).await.expect("the release");
        assert_eq!(slot_for(&caller(ID_B), Need::Open), Ok(SlotNo(1)), "an open opens as usual");
        assert!(!take_idle_notice(&b), "the open dropped the notice");
        unbind(SlotNo(1));
        assert_eq!(slot_for(&caller(ID_B), Need::Read), Err((409, NO_BROWSER_OF_YOURS.to_string())));

        // The default caller is never told anything of the kind.
        assert_eq!(slot_for(&Caller::Default, Need::Act), Ok(SlotNo::MAIN));
        fresh();
    }

    /// A notice is for the next call after the release: an assistant bound to
    /// a browser again before then is not told of the old one later.
    #[tokio::test]
    async fn a_notice_does_not_outlive_a_new_binding() {
        use v2_lib::commands::autorun_slots::{release_slot, take_idle_notice, ReleaseWhy};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        release_slot(SlotNo(1), ReleaseWhy::Idle).await.expect("the release");
        bind(SlotNo(2), &b);
        assert_eq!(slot_for(&caller(ID_B), Need::Read), Ok(SlotNo(2)));
        assert!(!take_idle_notice(&b));
        fresh();
    }

    /// The sweeper's release for idleness frees each idle extra browser at
    /// once (side by side, never the person's main one) and leaves a recent one.
    #[tokio::test]
    async fn the_sweeps_idle_release_frees_only_the_idle_extra_slots() {
        use v2_lib::commands::autorun_slots::{release_idle, take_idle_notice, IDLE_RELEASE_MS};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        let b = register(ID_B, "claude-code", T0);
        let c = register(ID_C, "claude-code", T0 + IDLE_RELEASE_MS);
        bind(SlotNo::MAIN, &a);
        bind(SlotNo(1), &b);
        bind(SlotNo(2), &c);
        let now = T0 + IDLE_RELEASE_MS;
        release_idle(now).await;
        assert_eq!(bound(SlotNo(1)), None, "B was idle");
        assert_eq!(bound(SlotNo(2)), Some(c.clone()), "C called just now");
        assert_eq!(bound(SlotNo::MAIN), Some(a.clone()), "main is never idle-released");
        assert!(take_idle_notice(&b), "B is told once");
        assert!(!take_idle_notice(&b));
        assert!(!take_idle_notice(&c));
        fresh();
    }

    /// A slot whose session lock is held (a call, an open, a replay step is
    /// going in it) is not idle however long its assistant was quiet.
    #[test]
    fn a_slot_whose_session_lock_is_held_is_not_idle() {
        use v2_lib::commands::autorun::try_hold_slot;
        use v2_lib::commands::autorun_slots::{idle_extras, IDLE_RELEASE_MS};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        let held = try_hold_slot(SlotNo(1)).expect("slot 1 is free");
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![], "a call holds the browser");
        drop(held);
        assert_eq!(idle_extras(T0 + IDLE_RELEASE_MS), vec![SlotNo(1)]);
        fresh();
    }

    /// An idle release that finds a replay or a batch going answers busy at
    /// once, stops nothing, keeps the binding and leaves no notice.
    #[tokio::test]
    async fn an_idle_release_of_a_browser_with_a_replay_or_batch_going_stops_nothing() {
        use v2_lib::ai_bridge::BatchGoing;
        use v2_lib::commands::autorun::BROWSER_BUSY;
        use v2_lib::commands::autorun_slots::{release_slot_within, take_idle_notice, ReleaseWhy};
        let short = std::time::Duration::from_millis(40);
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        {
            let replay = OneReplay::claim_for_assistant_in(SlotNo(1)).expect("slot 1 is free");
            let answer = release_slot_within(SlotNo(1), ReleaseWhy::Idle, short).await;
            assert_eq!(answer, Err(BROWSER_BUSY.to_string()));
            assert!(state(SlotNo(1)).is_running(), "the replay was left running");
            assert!(!state(SlotNo(1)).cancel.load(std::sync::atomic::Ordering::SeqCst), "nothing was cancelled");
            assert_eq!(bound(SlotNo(1)), Some(b.clone()));
            assert!(!take_idle_notice(&b), "no notice for a release that did not happen");
            drop(replay);
        }
        {
            let batch = BatchGoing::begin(SlotNo(1));
            let answer = release_slot_within(SlotNo(1), ReleaseWhy::Idle, short).await;
            assert_eq!(answer, Err(BROWSER_BUSY.to_string()));
            assert!(
                !v2_lib::ai_bridge::batch_stop(SlotNo(1)).load(std::sync::atomic::Ordering::SeqCst),
                "the batch was not told to stop"
            );
            assert_eq!(bound(SlotNo(1)), Some(b.clone()));
            assert!(!take_idle_notice(&b));
            drop(batch);
        }
        fresh();
    }

    /// The release acts on what idleness was judged on: a call made, a
    /// rebind, or an unbind between the judgement and the release refuses it.
    #[tokio::test]
    async fn an_idle_release_refuses_when_the_slot_changed_since_it_was_judged() {
        use v2_lib::commands::autorun::BROWSER_BUSY;
        use v2_lib::commands::autorun_slots::{idle_slots, release_idle_slot, take_idle_notice, IDLE_RELEASE_MS};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        let now = T0 + IDLE_RELEASE_MS;

        // A call by B after the judgement.
        bind(SlotNo(1), &b);
        let judged = idle_slots(now).pop().expect("B's browser is idle");
        assert_eq!((judged.slot, judged.key.clone()), (SlotNo(1), b.clone()));
        register(ID_B, "claude-code", T0 + 1);
        assert_eq!(release_idle_slot(&judged).await, Err(BROWSER_BUSY.to_string()));
        assert_eq!(bound(SlotNo(1)), Some(b.clone()), "B called, so it keeps its browser");
        assert!(!take_idle_notice(&b));

        // The slot rebound to C after the judgement.
        fresh();
        let b = register(ID_B, "claude-code", T0);
        let c = register(ID_C, "claude-code", T0);
        bind(SlotNo(1), &b);
        let judged = idle_slots(now).pop().expect("B's browser is idle");
        bind(SlotNo(1), &c);
        assert_eq!(release_idle_slot(&judged).await, Err(BROWSER_BUSY.to_string()));
        assert_eq!(bound(SlotNo(1)), Some(c.clone()), "C keeps what it was given");
        assert!(!take_idle_notice(&b) && !take_idle_notice(&c));

        // Unbound after the judgement: nothing is released, nobody is told.
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        let judged = idle_slots(now).pop().expect("B's browser is idle");
        unbind(SlotNo(1));
        assert_eq!(release_idle_slot(&judged).await, Err(BROWSER_BUSY.to_string()));
        assert!(!take_idle_notice(&b), "nothing was released, so nothing is said");
        fresh();
    }

    /// An idle release never waits for a slot's lock: held, it answers busy at
    /// once and the next sweep asks again.
    #[tokio::test]
    async fn an_idle_release_of_a_held_slot_returns_at_once() {
        use v2_lib::commands::autorun::{try_hold_slot, BROWSER_BUSY};
        use v2_lib::commands::autorun_slots::{idle_slots, release_idle_slot, release_slot, ReleaseWhy, IDLE_RELEASE_MS};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let b = register(ID_B, "claude-code", T0);
        bind(SlotNo(1), &b);
        let judged = idle_slots(T0 + IDLE_RELEASE_MS).pop().expect("B's browser is idle");
        let held = try_hold_slot(SlotNo(1)).expect("slot 1 is free");
        let began = std::time::Instant::now();
        // release_slot waits 30 seconds for a lock; an idle release must not.
        assert_eq!(release_idle_slot(&judged).await, Err(BROWSER_BUSY.to_string()));
        assert_eq!(release_slot(SlotNo(1), ReleaseWhy::Idle).await, Err(BROWSER_BUSY.to_string()));
        assert!(began.elapsed() < std::time::Duration::from_secs(2), "{:?}", began.elapsed());
        assert_eq!(bound(SlotNo(1)), Some(b.clone()));
        drop(held);
        assert!(release_idle_slot(&judged).await.is_ok(), "free again, the next sweep releases it");
        assert_eq!(bound(SlotNo(1)), None);
        fresh();
    }

    /// The sweeper runs the idle release after the past-grace one.
    #[test]
    fn the_sweeper_runs_the_idle_release_after_the_sweep() {
        let sweeper = include_str!("../../src/commands/ai_bridge.rs");
        let head = sweeper.find("pub fn spawn_agent_sweeper()").expect("sweeper");
        let body = &sweeper[head..head + 2600];
        let after_grace = body.find("autorun_slots::on_sweep(").expect("on_sweep");
        let idle = body.find("autorun_slots::release_idle(").expect("the sweeper never releases the idle");
        assert!(after_grace < idle, "{body}");
    }

    /// Before a replay's first step begins it is `step 0 of 0`, not the step
    /// it was asked to reach.
    #[test]
    fn a_replay_before_its_first_step_reports_zero_of_zero() {
        use v2_lib::commands::autorun_slots::replaying_activity;
        assert_eq!(replaying_activity(4521, (0, 0)), AgentActivity::Replaying { case_id: 4521, step: 0, of: 0 });
        assert_eq!(replaying_activity(4521, (3, 8)), AgentActivity::Replaying { case_id: 4521, step: 3, of: 8 });
    }

    /// Batches in one slot can overlap: it is going until the last one ends.
    #[test]
    fn overlapping_batches_keep_the_slot_going_until_the_last_ends() {
        use v2_lib::ai_bridge::{batch_going, BatchGoing};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        let one = SlotNo(1);
        assert!(!batch_going(one));
        let first = BatchGoing::begin(one);
        let second = BatchGoing::begin(one);
        assert!(batch_going(one));
        assert!(!batch_going(SlotNo::MAIN), "another slot's batch is another count");
        drop(first);
        assert!(batch_going(one), "the second batch is still going");
        drop(second);
        assert!(!batch_going(one));
    }

    /// The person's replay is never stopped by a Release from the card: only
    /// an assistant's is.
    #[tokio::test]
    async fn release_stops_an_assistants_replay_but_never_the_persons() {
        use v2_lib::commands::autorun_slots::{release_slot, ReleaseWhy};
        let _g = crate::serial::agents();
        let _l = crate::serial::log_tail();
        let _a = crate::serial::autorun();
        fresh();
        let cancelled = || state(SlotNo::MAIN).cancel.load(std::sync::atomic::Ordering::SeqCst);
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);

        let persons = OneReplay::claim().expect("main is free");
        release_slot(SlotNo::MAIN, ReleaseWhy::Person).await.unwrap();
        assert!(!cancelled(), "the person's replay was asked to stop");
        drop(persons);

        bind(SlotNo::MAIN, &a);
        let assistants = OneReplay::claim_for_assistant().expect("main is free");
        release_slot(SlotNo::MAIN, ReleaseWhy::Person).await.unwrap();
        assert!(cancelled(), "an assistant's replay is stopped by the release");
        drop(assistants);
        fresh();
    }

    /// A lock the card's watcher holds for a moment is waited out; one a
    /// replay or a step holds is still reported busy.
    #[test]
    fn a_brief_hold_is_waited_out_and_a_real_one_is_not() {
        use v2_lib::commands::autorun::try_lock_with_retries;
        let m = tokio::sync::Mutex::new(1u8);
        let pauses = std::cell::Cell::new(0);

        // Held until the first pause: the second try gets it.
        let held = std::cell::RefCell::new(Some(m.try_lock().unwrap()));
        let got = try_lock_with_retries(&m, 5, &|| {
            pauses.set(pauses.get() + 1);
            held.borrow_mut().take();
        });
        assert!(got.is_some());
        assert_eq!(pauses.get(), 1);
        drop(got);

        // Held throughout: five tries, four pauses, then busy.
        pauses.set(0);
        let _held = m.try_lock().unwrap();
        let got = try_lock_with_retries(&m, 5, &|| pauses.set(pauses.get() + 1));
        assert!(got.is_none());
        assert_eq!(pauses.get(), 4);
    }

    /// The same, from another thread that holds the lock a moment.
    #[test]
    fn a_lock_another_thread_holds_briefly_is_waited_out() {
        use v2_lib::commands::autorun::{try_lock_briefly, try_lock_with_retries};
        use std::sync::{mpsc, Arc};
        let m = Arc::new(tokio::sync::Mutex::new(1u8));
        let (held_tx, held_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (released_tx, released_rx) = mpsc::channel::<()>();
        let holder = {
            let m = Arc::clone(&m);
            std::thread::spawn(move || {
                let guard = m.try_lock().unwrap();
                held_tx.send(()).unwrap();
                // Held until the reader's first pause asks for it back; no
                // clock decides when that is.
                release_rx.recv().unwrap();
                drop(guard);
                released_tx.send(()).unwrap();
            })
        };
        held_rx.recv().unwrap();
        let pauses = std::cell::Cell::new(0);
        let got = try_lock_with_retries(&m, 5, &|| {
            pauses.set(pauses.get() + 1);
            // The first pause: the other thread lets go, and the next try
            // finds the lock free.
            let _ = release_tx.send(());
            let _ = released_rx.recv();
        });
        assert!(got.is_some(), "a lock the other thread let go of during the wait was not taken");
        assert_eq!(pauses.get(), 1, "the lock was held until the first pause only");
        drop(got);
        holder.join().unwrap();
        // One held throughout reads as busy.
        let _long = m.try_lock().unwrap();
        assert!(try_lock_briefly(&m).is_none());
    }

    /// The two readers that answer the person from a try_lock wait out the
    /// watcher's brief hold, and the person's Open browser clears a binding.
    #[test]
    fn the_persons_readers_retry_and_open_clears_the_binding() {
        let src = include_str!("../../src/commands/autorun.rs");
        let open = src.find("pub fn supervised_session_is_open_now()").expect("reader");
        assert!(src[open..open + 400].contains("try_lock_briefly("), "{}", &src[open..open + 400]);
        let release = src.find("pub fn release_stepped_run(").expect("release_stepped_run");
        assert!(src[release..release + 400].contains("try_lock_briefly("), "{}", &src[release..release + 400]);
        let into = src.find("async fn open_into(").expect("open_into");
        let body = &src[into..into + 1200];
        let person = body.find("OpenedBy::Person").expect("open_into tells a person's open apart");
        assert!(body[person..].contains("unbind("), "{body}");
    }

    #[test]
    fn views_list_live_agents_and_their_browser() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        register(ID_B, "claude-desktop", T0);
        bind(SlotNo::MAIN, &a);
        let views = agent_views(T0);
        assert_eq!(views.len(), 2, "{views:?}");
        let (va, vb) = (&views[0], &views[1]);
        assert_eq!((va.label.as_str(), vb.label.as_str()), ("Assistant 1", "Assistant 2"));
        assert_eq!(va.client.as_deref(), Some("claude-code"));
        assert_eq!(vb.client.as_deref(), Some("claude-desktop"));
        assert_eq!(va.slot, Some(0));
        assert!(!va.own_browser);
        assert_eq!(va.browser, None, "no session is open");
        assert_eq!(va.account, None);
        assert_eq!(va.activity, AgentActivity::Idle);
        assert_eq!(va.idle_minutes, 0);
        assert!(!va.changed_for_testing && !va.saves_blocked && !va.can_stop);
        assert_eq!(vb.slot, None);
        assert_eq!(vb.activity, AgentActivity::Idle);
        // Idle minutes count from the last call; the command answers the same list.
        assert_eq!(agent_views(T0 + 3 * 60_000 + 5)[0].idle_minutes, 3);
        assert_eq!(auto_run_agents().len(), 2);
        // A gone agent is not listed.
        agents::note_bye(ID_B, T0 + 1);
        let views = agent_views(T0 + 2);
        assert_eq!(views.iter().map(|v| v.label.as_str()).collect::<Vec<_>>(), ["Assistant 1"]);
        fresh();
    }

    /// What the main browser's changes are is published, so the view needs no lock for it.
    #[test]
    fn a_view_reports_the_changes_published_for_its_slot() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        show_overrides(&changed());
        assert!(agent_views(T0)[0].changed_for_testing);
        show_overrides(&Overrides::new());
        assert!(!agent_views(T0)[0].changed_for_testing);
        fresh();
    }

    /// The person's replay is not the assistant's: the card never shows it
    /// as the assistant's case, and offers no stop for it.
    #[test]
    fn a_persons_replay_is_not_shown_as_an_assistants() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        let replay = OneReplay::claim().expect("main is free");
        assert!(state(SlotNo::MAIN).is_running());
        let v = &agent_views(T0)[0];
        assert_eq!(v.activity, AgentActivity::Idle, "{v:?}");
        assert!(!v.can_stop, "the card does not offer to stop the person's replay");
        drop(replay);
        fresh();
    }

    /// An assistant's replay going in the slot is stoppable from the card.
    #[test]
    fn an_assistants_replay_can_be_stopped_from_the_card() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        let replay = OneReplay::claim_for_assistant().expect("main is free");
        let v = &agent_views(T0)[0];
        assert!(v.can_stop, "{v:?}");
        drop(replay);
        assert!(!agent_views(T0)[0].can_stop);
        fresh();
    }

    /// Neither the agent's id nor its pid is in what the webview is sent.
    #[test]
    fn a_view_carries_neither_id_nor_pid() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        let json = serde_json::to_value(&agent_views(T0)[0]).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "account",
                "activity",
                "browser",
                "can_stop",
                "changed_for_testing",
                "client",
                "idle_minutes",
                "label",
                "own_browser",
                "saves_blocked",
                "slot"
            ]
        );
        let text = json.to_string();
        assert!(!text.contains(ID_A) && !text.contains("4242"), "{text}");
        assert_eq!(json["activity"], serde_json::json!({ "kind": "idle" }));
        fresh();
    }

    /// The watcher sends only when the views differ from the last ones sent.
    #[test]
    fn views_are_sent_only_when_they_change() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let mut last = Vec::new();
        assert_eq!(next_views_to_send(&mut last, T0), None, "nobody connected, nothing sent");
        let a = register(ID_A, "claude-code", T0);
        let sent = next_views_to_send(&mut last, T0).expect("a new assistant is news");
        assert_eq!(sent.len(), 1);
        assert_eq!(next_views_to_send(&mut last, T0 + 500), None, "the same views are not sent again");
        bind(SlotNo::MAIN, &a);
        assert_eq!(next_views_to_send(&mut last, T0 + 1_000).map(|v| v[0].slot), Some(Some(0)));
        agents::note_bye(ID_A, T0 + 2_000);
        assert_eq!(next_views_to_send(&mut last, T0 + 2_000), Some(Vec::new()), "the last assistant going is news");
        assert_eq!(next_views_to_send(&mut last, T0 + 3_000), None);
        fresh();
    }

    #[tokio::test]
    async fn stop_agent_stops_that_slot_only() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let cancelled = |n: u8| state(SlotNo(n)).cancel.load(std::sync::atomic::Ordering::SeqCst);
        let assistants = OneReplay::claim_for_assistant_in(SlotNo(1)).expect("slot 1 is free");
        assert_eq!(auto_run_stop_agent(1).await, Ok(()));
        assert!(cancelled(1));
        assert!(!cancelled(0), "main's replay was not asked to stop");
        assert!(!cancelled(2));
        drop(assistants);
        fresh();
    }

    /// Stop never ends the person's own replay: a card showing Stop for a
    /// queued batch, or one a second behind, is not the person's replay's.
    /// The slot's batch is still asked to give way.
    #[tokio::test]
    async fn stop_never_stops_the_persons_replay() {
        use v2_lib::ai_bridge::{batch_stop, BatchGoing};
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let persons = OneReplay::claim().expect("main is free");
        let batch = BatchGoing::begin(SlotNo::MAIN);
        assert_eq!(auto_run_stop_agent(0).await, Ok(()));
        assert!(!state(SlotNo::MAIN).cancel.load(std::sync::atomic::Ordering::SeqCst), "the person's replay was stopped");
        assert!(batch_stop(SlotNo::MAIN).load(std::sync::atomic::Ordering::SeqCst), "the batch was not asked to stop");
        drop(batch);
        drop(persons);
        fresh();
    }

    #[tokio::test]
    async fn a_stop_also_asks_that_slots_batch_to_give_way() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let flag = |n: u8| v2_lib::ai_bridge::batch_stop(SlotNo(n)).load(std::sync::atomic::Ordering::SeqCst);
        auto_run_stop_agent(0).await.unwrap();
        assert!(flag(0) && !flag(1));
        fresh();
    }

    #[tokio::test]
    async fn an_unknown_slot_is_refused() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let no = NO_SUCH_BROWSER.to_string();
        assert_eq!(auto_run_stop_agent(7).await, Err(no.clone()));
        assert_eq!(auto_run_end_agent_discovery(3).await, Err(no.clone()));
        assert_eq!(auto_run_release_agent(255).await, Err(no));
        assert!(NO_SUCH_BROWSER.contains("not open any more"));
        fresh();
    }

    #[tokio::test]
    async fn releasing_an_empty_slot_unbinds_it() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        assert!(auto_run_release_agent(0).await.is_ok());
        assert_eq!(bound(SlotNo::MAIN), None);
        assert_eq!(slot_of(&a), None);
        fresh();
    }

    /// Ending a discovery from the card with none going is not an error.
    #[tokio::test]
    async fn ending_a_discovery_with_none_going_is_fine() {
        let _g = crate::serial::agents();
        let _a = crate::serial::autorun();
        fresh();
        assert_eq!(auto_run_end_agent_discovery(0).await, Ok(()));
        fresh();
    }

    /// The release logs the assistant by label, never by id or pid.
    #[tokio::test]
    async fn the_release_is_logged_by_label_only() {
        use v2_lib::commands::autorun_slots::{release_slot, ReleaseWhy};
        let logged = || {
            v2_lib::applog::recent(500).into_iter().map(|l| l.message).collect::<Vec<_>>().join("\n")
        };
        let _g = crate::serial::agents();
        let _l = crate::serial::log_tail();
        let _a = crate::serial::autorun();
        fresh();
        let a = register(ID_A, "claude-code", T0);
        bind(SlotNo::MAIN, &a);
        release_slot(SlotNo::MAIN, ReleaseWhy::Person).await.unwrap();
        let lines = logged();
        assert!(lines.contains("Auto Run: released Assistant 1's browser (the person released it)"), "{lines}");
        assert!(!lines.contains(ID_A) && !lines.contains("4242"));
        release_slot(SlotNo::MAIN, ReleaseWhy::Person).await.unwrap();
        let lines = logged();
        assert!(lines.contains("Auto Run: released the main Auto Run browser (the person released it)"), "{lines}");
        fresh();
    }

    /// The paths that end a main browser's use also end its binding.
    #[test]
    fn closing_and_taking_the_main_browser_unbind_it() {
        let src = include_str!("../../src/commands/autorun.rs");
        let close = src.find("fn close_session(s: Session)").expect("close_session");
        let close_body = &src[close..close + 700];
        assert!(close_body.contains("unbind("), "{close_body}");
        // Every slot's browser, an extra one included, unbinds its own slot
        // when it closes, so an extra slot whose browser closes is empty again.
        assert!(close_body.contains("unbind(s.slot)"), "{close_body}");
        let persons = src.find("pub(crate) async fn make_persons").expect("make_persons");
        let persons_body = &src[persons..persons + 700];
        assert!(persons_body.contains("unbind("), "{persons_body}");
        let release = src.find("pub async fn release_for_assistant").expect("release_for_assistant");
        let release_body = &src[release..release + 1500];
        assert!(release_body.contains("unbind("), "{release_body}");
        let sweeper = include_str!("../../src/commands/ai_bridge.rs");
        assert!(sweeper.contains("autorun_slots::on_sweep(&swept)"), "the sweeper hands its sweep to the bindings");
        let lib = include_str!("../../src/lib.rs");
        assert!(lib.contains("spawn_agents_watch("), "the watcher is started where the discovery events are set");
    }
}

// ---------------------------------------------------------------------------
// Everything global looks at every slot (Task 14)
// ---------------------------------------------------------------------------

mod global_checks {
    use super::*;
    use v2_lib::commands::autorun::{
        any_session_open, any_session_open_now, first_open, open_session_refusal_any, try_hold_slot, EXTRA_BROWSER_OPEN,
    };

    /// Which slot an open browser is reported in: the lowest, so main's
    /// sentence is the one said when main and an extra are both open.
    #[test]
    fn first_open_finds_the_lowest_open_slot() {
        assert_eq!(first_open([false, true, true]), Some(SlotNo(1)));
        assert_eq!(first_open([false, false, true]), Some(SlotNo(2)));
        assert_eq!(first_open([true, false, true]), Some(SlotNo::MAIN));
        assert_eq!(first_open([false, false, false]), None);
    }

    /// With nothing open, every global check says so; a lock that is taken
    /// counts as a browser in use, in any slot.
    #[tokio::test]
    async fn with_no_browser_open_nothing_refuses_and_a_taken_slot_counts_as_open() {
        let _g = serial::autorun();
        assert!(!any_session_open().await);
        assert!(open_session_refusal_any().await.is_none());
        assert!(!any_session_open_now());
        for n in SlotNo::extras() {
            let taken = try_hold_slot(n).expect("the slot is free");
            assert!(any_session_open_now(), "slot {} is in use and was not seen", n.0);
            drop(taken);
            assert!(!any_session_open_now());
        }
        assert!(EXTRA_BROWSER_OPEN.contains("assistant"), "{EXTRA_BROWSER_OPEN}");
        // The person reads this one, not an assistant: it tells them what to
        // do, never to ask someone.
        assert!(!EXTRA_BROWSER_OPEN.contains("ask the person"), "{EXTRA_BROWSER_OPEN}");
        assert!(EXTRA_BROWSER_OPEN.contains("release it on the Assistants card at the top of the Auto Run screen"), "{EXTRA_BROWSER_OPEN}");
        assert!(!EXTRA_BROWSER_OPEN.contains('\u{2014}') && !EXTRA_BROWSER_OPEN.contains('\u{2013}'));
    }

    /// The unattended run, the recorder, the module switch and the
    /// environment save and switch ask about every slot, not main alone.
    #[test]
    fn every_global_check_looks_at_every_slot() {
        let replay = norm(include_str!("../../src/commands/autorun_replay.rs"));
        let record = norm(include_str!("../../src/commands/autorun_record.rs"));
        for (name, src) in [("autorun_replay.rs", &replay), ("autorun_record.rs", &record)] {
            assert!(src.contains("open_session_refusal_any()"), "{name} does not ask every slot");
            assert!(!src.contains("open_session_refusal()"), "{name} still asks main alone");
        }
        let modules = norm(include_str!("../../src/commands/modules.rs"));
        let body_of = body(&modules, "pub fn auto_run_browser_open()");
        assert!(body_of.contains("any_session_open_now()") && body_of.contains("any_discovery_active()"), "{body_of}");
        assert!(
            !body_of.contains("auto_run_discovery_active()") && !body_of.contains("supervised_session_is_open_now()"),
            "{body_of}"
        );

        let envs = norm(include_str!("../../src/commands/environments.rs"));
        for f in ["pub async fn save_with(", "pub async fn switch_with<"] {
            let b = body(&envs, f);
            assert!(b.contains("opening().write()"), "{f}: {b}");
            assert!(b.contains("lock_every_slot()"), "{f}: {b}");
            assert!(b.contains("refuse_switch(slots.any_open())"), "{f}: {b}");
            // A slot an assistant keeps taken refuses the switch at once.
            assert!(b.contains("slots.busy") && b.contains("SWITCH_BUSY"), "{f}: {b}");
            assert!(!b.contains("supervised().lock()"), "{f}: {b}");
        }

        // The one place the slots are locked together, in slot order: main's
        // lock is waited for, an assistant's only tried briefly.
        let autorun = norm(include_str!("../../src/commands/autorun.rs"));
        let every = body(&autorun, "pub(crate) async fn lock_every_slot(");
        assert!(every.contains("for n in SlotNo::all() {") && every.contains("session_slot(n).lock().await"), "{every}");
        assert!(every.contains("try_lock_briefly(session_slot(n))") && every.contains("every.busy = true"), "{every}");
        assert_eq!(every.matches(".lock().await").count(), 1, "only main's lock is waited for: {every}");

        // The clear buttons look at every slot too.
        let clear = body(&autorun, "pub async fn refuse_while_a_run_is_going(");
        assert!(clear.contains("open_session_refusal_any()"), "{clear}");
        // Making way for a setup looks at each slot in turn, and no longer
        // locks `SESSION` alone.
        let way = body(&autorun, "pub async fn make_way_for_setup(");
        assert!(way.contains("for n in SlotNo::all() {") && way.contains("session_slot(n).lock().await"), "{way}");
        assert!(!way.contains("SESSION.lock()"), "{way}");
        // Every slot is asked after letting a gone browser go, one lock at a time.
        let any = body(&autorun, "async fn slots_open(");
        assert!(any.contains("let_go_if_gone_in(n, &mut slot)") && any.contains("for n in SlotNo::all() {"), "{any}");
    }

    /// Release all locks the opening lock for writing and then every slot in
    /// order; the app's exit closes every slot in order. Neither locks
    /// `SESSION` any more.
    #[test]
    fn release_all_takes_every_slot_in_order() {
        let source = norm(include_str!("../../src/commands/autorun.rs"));
        let release = body(&source, "pub async fn release_autorun_browsers()");
        let batches = release.find("stop_all_batches()").expect("batches are stopped");
        let write = release.find("opening().write()").expect("Release all takes the opening lock for writing");
        assert!(batches < write, "the batches are stopped before the opening lock is waited for");
        let after_write = &release[write..];
        let walk = after_write.find("for n in SlotNo::all() {").expect("a loop over the slots after the write");
        let in_loop = &after_write[walk..];
        assert!(in_loop.contains("session_slot(n).lock()"), "each slot's lock is taken in the loop: {in_loop}");
        assert!(in_loop.contains("RELEASE_WAIT"), "each slot's wait is bounded: {in_loop}");
        assert!(!release.contains("SESSION.lock()"), "{release}");
        // Each slot's browser is closed, its discovery published and its
        // binding cleared.
        assert!(release.contains("close_browser_in("), "{release}");
        assert!(release.contains("publish_discovery_in("), "{release}");
        assert!(release.contains("unbind("), "{release}");
        assert!(release.contains("tell_panes_closed()"), "{release}");
        // The slot locks go only after `end_all` has returned.
        let ended = release.find("tree::end_all").expect("end_all");
        let dropped = release.rfind("drop(").expect("the locks are dropped");
        assert!(ended < dropped, "{release}");

        let exit = body(&source, "pub async fn close_autorun_browsers()");
        assert!(!exit.contains("SESSION.lock()") && !exit.contains("opening().write()"), "{exit}");
        let batches = exit.find("stop_all_batches()").expect("batches are stopped");
        let walk = exit[batches..].find("for n in SlotNo::all() {").expect("exit walks the slots after stopping");
        let rest = &exit[batches + walk..];
        assert!(rest.contains("session_slot(n).lock().await"), "{rest}");
        assert!(rest.contains("finish_mapping(") && rest.contains("finish_explore(") && rest.contains("close_session("), "{rest}");
    }

    /// An open runs with its slot's lock held, so it must never wait for the
    /// opening lock: a switch holding that for writing waits for the same
    /// slot's lock. It tries, and is refused at once when a write has it.
    #[test]
    fn an_open_never_waits_for_the_opening_lock_under_a_slot_lock() {
        let source = norm(include_str!("../../src/commands/autorun.rs"));
        let open = body(&source, "async fn open_into(");
        assert!(open.contains("opening().try_read()"), "{open}");
        for (name, src) in [("autorun.rs", &source), ("ai_bridge.rs", &norm(include_str!("../../src/ai_bridge.rs")))] {
            assert!(!src.contains("opening().read()"), "{name} waits for the opening lock");
        }
        // The refusal is a sentence for the person, no dash.
        let sentence = v2_lib::commands::autorun_slots::OPEN_WHILE_SWITCHING;
        assert!(sentence.contains("try again") && !sentence.contains('\u{2014}') && !sentence.contains('\u{2013}'), "{sentence}");
    }

    /// The opening lock is a read-write lock: opens read, and a write held or
    /// waited for refuses every new read.
    #[tokio::test]
    async fn a_write_waiting_for_the_opening_lock_refuses_new_opens() {
        use v2_lib::commands::autorun_slots::opening;
        let _g = serial::autorun();
        let read = opening().try_read().expect("free");
        let write = opening().write();
        tokio::pin!(write);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(100), &mut write).await.is_err());
        assert!(opening().try_read().is_err(), "a new open got past a switch that is waiting");
        drop(read);
        let guard = tokio::time::timeout(std::time::Duration::from_secs(5), write).await.expect("the write was never given");
        assert!(opening().try_read().is_err(), "an open got past a switch");
        drop(guard);
        assert!(opening().try_read().is_ok());
    }
}
