//! The Auto Run browser's numbered slots: main is the one the person and
//! every existing path use; two more sit beside it. In this phase only main
//! is ever opened, so these pin the table itself, the state each slot keeps
//! apart, and that main's old names still mean main.

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
    for start in ["pub async fn release_autorun_browsers()", "pub async fn close_autorun_browsers()"] {
        let from = &source[source.find(start).unwrap_or_else(|| panic!("{start} is gone"))..];
        let before_lock = &from[..from.find("SESSION.lock()").unwrap()];
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
    use v2_lib::commands::autorun_slots::{need_of, slot_for, slot_if_any, Need};

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
        ] {
            assert_eq!(need_of(method, path), None, "{method} {path}");
        }
        // A method a route does not answer to is no browser route either.
        assert_eq!(need_of("POST", "/autorun-page"), None);
        assert_eq!(need_of("GET", "/autorun-try"), None);
    }

    /// In this phase every caller, whatever it needs, gets the main browser.
    #[test]
    fn slot_for_is_main_for_now() {
        let agent = Caller::Agent(AgentKey { id: "a".repeat(32), name: Some("writer".to_string()) });
        for caller in [Caller::Default, agent] {
            for need in [Need::Open, Need::Act, Need::Read] {
                assert_eq!(slot_for(&caller, need), Ok(SlotNo::MAIN), "{caller:?} {need:?}");
            }
        }
    }

    #[test]
    fn slot_if_any_is_main_for_now() {
        let agent = Caller::Agent(AgentKey { id: "b".repeat(32), name: None });
        assert_eq!(slot_if_any(&Caller::Default), Some(SlotNo::MAIN));
        assert_eq!(slot_if_any(&agent), Some(SlotNo::MAIN));
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
            "pub async fn supervised_page_within_in(",
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
