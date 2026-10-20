//! The AI Bridge switches belong to an environment. What the bridge
//! enforces - database writes, API template writes, and whether a database
//! change runs without asking - must follow the ACTIVE environment, and a
//! switch of environment must never leave the bridge allowing a write the
//! new environment has off: not before the switch, not during it, and not
//! because a push from the webview arrived late.

use std::path::Path;
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::{route, still_allowed, BridgeContext, API_WRITES_MOVED};
use v2_lib::ai_tools::CORE_TOOLS;
use v2_lib::commands::ai_bridge::{hold_writes_while, push_context_with, release_writes};
use v2_lib::commands::ai_tools::{
    match_allow_files_with, repair_allow_files, set_auto_approve_with, ToolHome, AUTO_APPROVE_ENV_MOVED,
};
use v2_lib::commands::environments::{env_list_with, list_adopting, move_bridge_to, switch_reconciler, switch_with};
use v2_lib::db::credentials::MemoryStore;
use v2_lib::environments::*;

fn known() -> Vec<String> {
    ["dev-read", "dev-login", "qa-read"].iter().map(|s| s.to_string()).collect()
}

fn env(name: &str, db_id: &str) -> Environment {
    Environment {
        id: String::new(),
        name: name.into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: db_id.into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
        db_auto_approve: false,
    }
}

/// Default (on `dev-login`, active) and QA (on `qa-read`).
fn two_envs(root: &Path) -> (String, String) {
    let first = load_or_init(root, Some("dev-login")).unwrap().active;
    let after = save_env(root, env("QA", "qa-read"), &known()).unwrap();
    let qa = after.environments.iter().find(|e| e.name == "QA").unwrap().id.clone();
    (first, qa)
}

fn find(root: &Path, id: &str) -> Environment {
    load_or_init(root, None).unwrap().environments.into_iter().find(|e| e.id == id).unwrap()
}

/// What the webview pushes for one environment: everything on.
fn pushed() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        db_id: Some("dev-login".into()),
        db_writes: true,
        api_writes: true,
        ..BridgeContext::default()
    }
}

// ------------------------------------------------------------ the record

#[test]
fn a_new_environment_starts_with_no_ask_off_and_a_save_never_moves_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    assert!(!find(root, &first).db_auto_approve);
    assert!(!find(root, &qa).db_auto_approve);

    set_db_auto_approve(root, &qa, true).unwrap();
    assert!(find(root, &qa).db_auto_approve);
    assert!(!find(root, &first).db_auto_approve, "only the environment it was set for");

    // An edit made from a copy read before the switch moved - or one that
    // says off - leaves it as it is: only its own command changes it.
    let mut edit = find(root, &qa);
    edit.name = "QA two".into();
    edit.db_auto_approve = false;
    save_env(root, edit, &known()).unwrap();
    assert!(find(root, &qa).db_auto_approve);

    set_db_auto_approve(root, &qa, false).unwrap();
    let mut edit = find(root, &qa);
    edit.db_auto_approve = true;
    save_env(root, edit, &known()).unwrap();
    assert!(!find(root, &qa).db_auto_approve, "a save can never turn it on either");

    // And a new environment never arrives with it on.
    let mut fresh = env("Staging", "qa-read");
    fresh.db_auto_approve = true;
    let after = save_env(root, fresh, &known()).unwrap();
    assert!(!after.environments.iter().find(|e| e.name == "Staging").unwrap().db_auto_approve);
}

#[test]
fn reading_the_active_environment_never_makes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(read_active(dir.path()).unwrap(), None);
    assert!(!dir.path().join("environments.json").exists());
}

/// Today's machine-wide "Run database changes without asking" goes to the
/// environment active when the switch moves over, and to no other - once.
#[test]
fn the_old_machine_wide_no_ask_goes_to_the_active_environment_only_and_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);
    set_active(root, &qa).unwrap();

    let view = list_adopting(root, &store, None, true).unwrap();
    assert!(find(root, &qa).db_auto_approve);
    assert!(!find(root, &first).db_auto_approve);
    assert!(view.environments.iter().find(|e| e.id == qa).unwrap().db_auto_approve, "the view says so too");

    // Run again - after a switch, with the old setting somehow still on -
    // and nothing else is turned on.
    set_active(root, &first).unwrap();
    list_adopting(root, &store, None, true).unwrap();
    assert!(!find(root, &first).db_auto_approve);
}

#[test]
fn with_the_old_setting_off_nothing_is_turned_on_and_it_never_moves_later() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, _) = two_envs(root);
    list_adopting(root, &store, None, false).unwrap();
    list_adopting(root, &store, None, true).unwrap();
    assert!(!find(root, &first).db_auto_approve);
}

#[test]
fn the_first_list_makes_default_and_gives_it_the_old_setting() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let view = list_adopting(root, &MemoryStore::default(), Some("dev-login"), true).unwrap();
    assert_eq!(view.environments.len(), 1);
    assert!(view.environments[0].db_auto_approve);
    assert_eq!(view.environments[0].db_id, "dev-login");
}

// ------------------------------------------------------------ the bridge

/// The moment the active environment changes, the bridge allows no write
/// at all: Rust cannot know the new environment's write switches (they are
/// the webview's), so it holds them off until the webview says what they
/// are FOR THAT environment. The database and "without asking" are the
/// new environment's at once.
#[test]
fn a_switch_turns_the_bridges_writes_off_and_takes_the_new_environments_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);
    set_db_auto_approve(root, &qa, true).unwrap();
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(root), pushed(), &first);
    assert!(ctx.lock().unwrap().db_writes, "the active environment's own push is honoured");

    move_bridge_to(root, &store, &qa, Some(&ctx)).unwrap();
    let c = ctx.lock().unwrap().clone();
    assert!(!c.db_writes && !c.api_writes, "{c:?}");
    assert_eq!(c.db_id.as_deref(), Some("qa-read"));
    assert!(c.db_no_ask, "QA's own choice");
    assert_eq!(c.org, "acme", "the rest of the context is left as it was");

    move_bridge_to(root, &store, &first, Some(&ctx)).unwrap();
    assert!(!ctx.lock().unwrap().db_no_ask, "Default's own choice");
}

#[test]
fn a_push_for_any_environment_but_the_active_one_never_turns_a_write_on() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());

    // A push that was on its way when the switch landed, a push from a
    // webview that does not know the environment yet, and one with no
    // environments file at all.
    for (root, tag) in [(Some(root), qa.as_str()), (Some(root), ""), (None, first.as_str())] {
        push_context_with(&ctx, root, pushed(), tag);
        let c = ctx.lock().unwrap().clone();
        assert!(!c.db_writes && !c.api_writes, "tag {tag:?}: {c:?}");
        assert_eq!(c.org, "acme", "the rest of a push still applies");
    }

    push_context_with(&ctx, Some(root), pushed(), &first);
    let c = ctx.lock().unwrap().clone();
    assert!(c.db_writes && c.api_writes, "{c:?}");
}

#[test]
fn a_push_takes_no_ask_from_the_active_environment_not_the_webview() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, _) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(root), BridgeContext { db_no_ask: true, ..pushed() }, &first);
    assert!(!ctx.lock().unwrap().db_no_ask);

    set_db_auto_approve(root, &first, true).unwrap();
    push_context_with(&ctx, Some(root), pushed(), &first);
    assert!(ctx.lock().unwrap().db_no_ask);
}

/// The guarantee under load: switches back and forth while the webview
/// keeps pushing "everything on" for Default. Looked at under the
/// context's own lock - which every switch holds while it moves the file -
/// writes are never on while anything but Default is active.
#[test]
fn no_push_or_switch_ever_leaves_writes_on_for_an_environment_that_is_not_active() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let (first, qa) = two_envs(&root);
    let ctx = Arc::new(Mutex::new(BridgeContext::default()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let switcher = {
        let (root, ctx, first, qa) = (root.clone(), Arc::clone(&ctx), first.clone(), qa.clone());
        std::thread::spawn(move || {
            let store = MemoryStore::default();
            for i in 0..150 {
                move_bridge_to(&root, &store, if i % 2 == 0 { &qa } else { &first }, Some(&ctx)).unwrap();
                // What `switch_with` does once the allow files match.
                release_writes(&ctx);
            }
        })
    };
    let pusher = {
        let (root, ctx, first, stop) = (root.clone(), Arc::clone(&ctx), first.clone(), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                push_context_with(&ctx, Some(&root), pushed(), &first);
            }
        })
    };
    while !switcher.is_finished() {
        let c = ctx.lock().unwrap();
        if c.db_writes || c.api_writes {
            assert_eq!(active_id(&root).unwrap(), first, "writes on while QA is active: {c:?}");
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    switcher.join().unwrap();
    pusher.join().unwrap();
}

#[tokio::test]
async fn the_switch_reapplies_the_allow_files_after_the_bridge_has_moved() {
    let _locks = switch_locks();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(&root);
    let ctx = Arc::new(Mutex::new(BridgeContext::default()));
    push_context_with(&ctx, Some(&root), pushed(), &first);

    let seen = Arc::new(Mutex::new(Vec::<(bool, String)>::new()));
    let after = {
        let (ctx, seen, root) = (Arc::clone(&ctx), Arc::clone(&seen), root.clone());
        move || {
            let writes = ctx.lock().unwrap().db_writes;
            seen.lock().unwrap().push((writes, active_id(&root).unwrap()));
        }
    };
    let (view, _) = switch_with(&root, &store, &qa, Some(&ctx), after).await.unwrap();
    assert_eq!(view.active, qa);
    // Once, with the bridge already on QA and every write off.
    assert_eq!(*seen.lock().unwrap(), vec![(false, qa.clone())]);
}

#[tokio::test]
async fn a_refused_switch_leaves_the_bridge_and_the_allow_files_alone() {
    let _locks = switch_locks();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(&root);
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(&root), pushed(), &first);

    let run = v2_lib::api_templates::runner::claim().expect("no template is running");
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&called);
    let err = switch_with(&root, &store, &qa, Some(&ctx), move || flag.store(true, std::sync::atomic::Ordering::SeqCst))
        .await
        .unwrap_err();
    drop(run);
    assert!(err.contains("API template"), "{err}");
    assert!(ctx.lock().unwrap().db_writes, "nothing moved, so nothing was switched off");
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
}

/// The switch's locks: the same ones `environments.rs`'s switch tests take
/// (the Auto Run claims, the template run slot, the held browsers).
type Guard = std::sync::MutexGuard<'static, ()>;
fn switch_locks() -> (Guard, Guard, Guard) {
    (crate::serial::autorun(), crate::serial::api_template_run(), crate::serial::held_browsers())
}

#[test]
fn turning_no_ask_on_is_for_the_environment_shown_and_refused_once_it_is_not_active() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());

    set_auto_approve_with(root, Some(&ctx), &first, true).unwrap();
    assert!(find(root, &first).db_auto_approve);
    assert!(ctx.lock().unwrap().db_no_ask);

    // The tab showed QA's switch, but Default is active by the time it lands.
    assert_eq!(set_auto_approve_with(root, Some(&ctx), &qa, true).unwrap_err(), AUTO_APPROVE_ENV_MOVED);
    assert!(!find(root, &qa).db_auto_approve);

    set_auto_approve_with(root, Some(&ctx), &first, false).unwrap();
    assert!(!find(root, &first).db_auto_approve);
    assert!(!ctx.lock().unwrap().db_no_ask);
}

/// `/tools` tells the assistant it need not ask only while the context's
/// environment says so AND writes are on.
#[tokio::test]
async fn tools_says_no_ask_only_with_the_environments_choice_and_writes_on() {
    for (no_ask, writes, expected) in [(true, true, true), (true, false, false), (false, true, false)] {
        let ctx = BridgeContext { db_no_ask: no_ask, db_writes: writes, ..BridgeContext::default() };
        let (status, body) = route(&ctx, None, "GET", "/tools", "", "test").await;
        assert_eq!(status, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["db_no_ask"], expected, "no_ask {no_ask}, writes {writes}");
    }
}

// ------------------------------------------------- holding writes off

/// A push for the new environment can land while the switch is still
/// making the allow files match it - a list read for another screen
/// tells the webview the new environment, and that pushes. Its writes wait
/// until the allow files say what the new environment says, then apply.
#[tokio::test]
async fn a_push_for_the_new_environment_during_the_allow_file_update_waits_for_it() {
    let _locks = switch_locks();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(&root);
    let ctx = Arc::new(Mutex::new(BridgeContext::default()));
    push_context_with(&ctx, Some(&root), pushed(), &first);

    let during = {
        let (ctx, root, qa) = (Arc::clone(&ctx), root.clone(), qa.clone());
        move || {
            push_context_with(&ctx, Some(&root), pushed(), &qa);
            let c = ctx.lock().unwrap().clone();
            (c.db_writes, c.api_writes)
        }
    };
    let (_, seen) = switch_with(&root, &store, &qa, Some(&ctx), during).await.unwrap();
    assert_eq!(seen, Some((false, false)), "held while the allow files were brought in line");
    let c = ctx.lock().unwrap().clone();
    assert!(c.db_writes && c.api_writes, "then QA's own push applies: {c:?}");
}

#[tokio::test]
async fn the_hold_ends_even_when_the_allow_file_update_fails() {
    let _locks = switch_locks();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = MemoryStore::default();
    let (_, qa) = two_envs(&root);
    let ctx = Mutex::new(BridgeContext::default());
    let (_, seen) = switch_with(&root, &store, &qa, Some(&ctx), || -> () { panic!("the allow file could not be read") })
        .await
        .unwrap();
    assert_eq!(seen, None, "the answer says it did not finish");
    push_context_with(&ctx, Some(&root), pushed(), &qa);
    assert!(ctx.lock().unwrap().db_writes, "nothing is held any more");
}

/// The repair that runs when the bridge starts (and on the first list
/// after) holds writes the same way: on is restored, never granted.
#[test]
fn a_repair_holds_writes_off_and_puts_back_only_what_was_on() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, _) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(root), pushed(), &first);
    let inside = hold_writes_while(Some(&ctx), || ctx.lock().unwrap().db_writes);
    assert!(!inside);
    assert!(ctx.lock().unwrap().db_writes, "what was on before comes back");

    let off = Mutex::new(BridgeContext::default());
    hold_writes_while(Some(&off), || ());
    assert!(!off.lock().unwrap().db_writes, "and nothing that was off is turned on");
}

// ------------------------------------------------- tools and database after a switch

fn disabled_by_tools(c: &BridgeContext) -> Vec<String> {
    c.disabled_tools.clone()
}

#[tokio::test]
async fn after_a_switch_every_switchable_tool_is_off_until_the_new_environments_push() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(root), pushed(), &first);
    move_bridge_to(root, &store, &qa, Some(&ctx)).unwrap();
    release_writes(&ctx);

    let off = disabled_by_tools(&ctx.lock().unwrap());
    for t in ["get_tags", "db_query", "search_wiki", "run_api_template"] {
        assert!(off.iter().any(|o| o == t), "{t} is off: {off:?}");
    }
    assert!(off.iter().all(|o| !CORE_TOOLS.contains(&o.as_str())), "core tools are never off: {off:?}");
    let c = ctx.lock().unwrap().clone();
    let (_, body) = route(&c, None, "GET", "/tools", "", "test").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["disabled"].as_array().unwrap().iter().any(|t| t == "get_tags"), "{body}");

    // QA's own push brings its own list.
    push_context_with(&ctx, Some(root), pushed(), &qa);
    assert!(ctx.lock().unwrap().disabled_tools.is_empty());
}

#[test]
fn a_stale_push_keeps_the_contexts_database_and_tools() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    move_bridge_to(root, &store, &qa, Some(&ctx)).unwrap();
    release_writes(&ctx);
    let before = ctx.lock().unwrap().clone();

    // Default's push, still on its way when the switch landed.
    let stale = BridgeContext { db_id: Some("dev-login".into()), disabled_tools: vec![], ..pushed() };
    push_context_with(&ctx, Some(root), stale, &first);
    let c = ctx.lock().unwrap().clone();
    assert_eq!(c.db_id.as_deref(), Some("qa-read"), "QA's database stays");
    assert_eq!(c.disabled_tools, before.disabled_tools, "and the switch's tool list");
    assert!(!c.db_writes && !c.api_writes);
    assert_eq!(c.org, "acme", "the rest of the push still applies");
}

/// Before any environment is the context's own (a webview that does not
/// know it yet), there is nothing of an environment's to keep: the push's
/// tool list is taken, its writes are not.
#[test]
fn before_any_environment_a_push_brings_its_tools_but_no_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    let early = BridgeContext { disabled_tools: vec!["get_tags".into()], ..pushed() };
    push_context_with(&ctx, Some(root), early, "");
    let c = ctx.lock().unwrap().clone();
    assert_eq!(c.disabled_tools, vec!["get_tags".to_string()]);
    assert!(!c.db_writes && !c.api_writes);
}

/// The network tools' switch is the environment's own, like the tool list:
/// the active environment's push brings it, a push for any other never
/// turns it on (a stale one keeps the context's), and a switch of
/// environment turns it off until the new environment's push.
#[test]
fn the_network_tools_switch_follows_the_active_environment() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    let on = || BridgeContext { network_tools: true, ..pushed() };

    // Before any environment is the context's own, nothing turns it on.
    push_context_with(&ctx, Some(root), on(), "");
    assert!(!ctx.lock().unwrap().network_tools);
    push_context_with(&ctx, Some(root), on(), &first);
    assert!(ctx.lock().unwrap().network_tools, "the active environment's own push is honoured");

    // A push for another environment keeps the context's: on stays on, off
    // stays off.
    push_context_with(&ctx, Some(root), BridgeContext { network_tools: false, ..pushed() }, &qa);
    assert!(ctx.lock().unwrap().network_tools, "a stale push turned it off");
    move_bridge_to(root, &store, &qa, Some(&ctx)).unwrap();
    assert!(!ctx.lock().unwrap().network_tools, "a switch leaves it off until QA's push");
    push_context_with(&ctx, Some(root), on(), &first);
    assert!(!ctx.lock().unwrap().network_tools, "Default's push turned it on in QA");
    push_context_with(&ctx, Some(root), on(), &qa);
    assert!(ctx.lock().unwrap().network_tools, "QA's own push brings QA's");
}

/// Fail closed: while the active environment cannot be read, a push that
/// turns the network tools OFF for the environment the context already
/// holds is still honoured; nothing there turns them on, and a push for
/// another environment changes nothing.
#[test]
fn an_off_push_for_the_held_environment_is_honoured_when_the_file_cannot_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    let on = || BridgeContext { network_tools: true, ..pushed() };
    let off = || BridgeContext { network_tools: false, ..pushed() };
    push_context_with(&ctx, Some(root), on(), &first);
    assert!(ctx.lock().unwrap().network_tools);

    // The file is now unreadable (corrupt after a crash).
    std::fs::write(root.join("environments.json"), "{ not json").unwrap();
    assert!(v2_lib::environments::read_active(root).is_err());

    // Another environment's push, on or off, changes nothing.
    push_context_with(&ctx, Some(root), off(), &qa);
    assert!(ctx.lock().unwrap().network_tools, "a stale push turned it off");
    // An ON push is not brought in by this path, and it keeps what was on.
    push_context_with(&ctx, Some(root), on(), &first);
    assert!(ctx.lock().unwrap().network_tools);
    // The held environment's OFF push is honoured.
    push_context_with(&ctx, Some(root), off(), &first);
    assert!(!ctx.lock().unwrap().network_tools, "the person's OFF was ignored");
    // And an ON push cannot turn it back on while the file is unreadable.
    push_context_with(&ctx, Some(root), on(), &first);
    assert!(!ctx.lock().unwrap().network_tools, "an ON push was believed without the environment");
}

// ------------------------------------------------- the allow files

/// A machine where Claude Code is installed, our server registered for the
/// whole machine, and its settings carrying the database "always allow".
fn tool_home(settings: &str) -> (tempfile::TempDir, ToolHome) {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude.json"), r#"{"mcpServers": {"tcm": {"command": "x", "args": ["--mcp"]}}}"#)
        .unwrap();
    std::fs::write(home.path().join(".claude").join("settings.json"), settings).unwrap();
    let tools = ToolHome {
        home: home.path().to_string_lossy().to_string(),
        appdata: home.path().join("appdata").to_string_lossy().to_string(),
        on_path: |_| false,
    };
    (home, tools)
}

const GRANTED: &str = r#"{"permissions": {"allow": ["mcp__tcm__db_query", "mcp__tcm__preview_report_live"]}}"#;

fn user_settings(home: &tempfile::TempDir) -> String {
    std::fs::read_to_string(home.path().join(".claude").join("settings.json")).unwrap()
}

#[test]
fn the_repair_at_start_takes_out_entries_an_environment_with_no_ask_off_does_not_want() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    two_envs(root);
    let (home, tools) = tool_home(GRANTED);
    let ctx = Mutex::new(BridgeContext::default());
    repair_allow_files(Some(&ctx), root, None, &tools);
    assert!(!user_settings(&home).contains("db_query"), "{}", user_settings(&home));
}

#[test]
fn the_repair_writes_the_entries_an_environment_with_no_ask_on_wants() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, _) = two_envs(root);
    set_db_auto_approve(root, &first, true).unwrap();
    let (home, tools) = tool_home("{}");
    repair_allow_files(None, root, None, &tools);
    assert!(user_settings(&home).contains("mcp__tcm__db_query"), "{}", user_settings(&home));
}

/// What `env_set_active` runs after the bridge has moved: the old
/// environment's entries go, and what was done comes back in the answer.
#[tokio::test]
async fn the_switch_takes_out_the_old_environments_entries_and_answers_what_it_did() {
    let _locks = switch_locks();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(&root);
    set_db_auto_approve(&root, &first, true).unwrap();
    let (home, tools) = tool_home(GRANTED);
    let (_, outcomes) = switch_with(&root, &store, &qa, None, switch_reconciler(root.clone(), None, tools)).await.unwrap();
    assert!(!user_settings(&home).contains("db_query"), "{}", user_settings(&home));
    let outcomes = outcomes.unwrap();
    assert!(outcomes.iter().any(|o| o.tool == "Claude Code" && o.applied), "{outcomes:?}");
}

#[tokio::test]
async fn a_removal_that_fails_is_in_the_switch_answer() {
    let _locks = switch_locks();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let store = MemoryStore::default();
    let (_, qa) = two_envs(&root);
    let (_home, tools) = tool_home("{ this is not json");
    let (_, outcomes) = switch_with(&root, &store, &qa, None, switch_reconciler(root.clone(), None, tools)).await.unwrap();
    let outcomes = outcomes.unwrap();
    let cc = outcomes.iter().find(|o| o.tool == "Claude Code").expect("Claude Code is named");
    assert!(!cc.applied, "{cc:?}");
    assert!(!cc.note.is_empty(), "and says why");
}

/// Two updates at once - a switch taking entries out while a turn-on writes
/// them - must end with the files saying what the active environment says.
/// Each reads the environment and applies under one lock; without it the
/// slower one, reading first and writing last, wins with a stale answer.
#[test]
fn allow_file_updates_happen_one_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let (first, qa) = two_envs(&root);
    set_db_auto_approve(&root, &first, true).unwrap();
    let applied = Arc::new(Mutex::new(Vec::<bool>::new()));

    let slow = {
        let (root, applied) = (root.clone(), Arc::clone(&applied));
        std::thread::spawn(move || {
            match_allow_files_with(&root, |on| {
                std::thread::sleep(std::time::Duration::from_millis(300));
                applied.lock().unwrap().push(on);
            })
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    // The switch to QA (no-ask off), and its own update.
    set_active(&root, &qa).unwrap();
    match_allow_files_with(&root, |on| applied.lock().unwrap().push(on));
    slow.join().unwrap();
    let applied = applied.lock().unwrap().clone();
    assert_eq!(applied.last(), Some(&false), "the last word is QA's: {applied:?}");
}

// ------------------------------------------------- the old setting

#[test]
fn listing_the_environments_clears_the_old_machine_wide_setting() {
    let _settings = crate::serial::app_settings();
    let dir = std::env::temp_dir().join(format!("tcm-suite-app-settings-{}", std::process::id()));
    v2_lib::app_settings::init(dir.clone());
    v2_lib::app_settings::update(|s| s.db_auto_approve = true).unwrap();
    let envs = tempfile::tempdir().unwrap();
    let view = env_list_with(envs.path(), &MemoryStore::default(), Some("dev-login")).unwrap();
    assert!(view.environments[0].db_auto_approve, "it moved to the environment");
    assert!(!v2_lib::app_settings::current().db_auto_approve, "and is gone from the machine");
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------- API templates

/// A template call is judged on the context it arrived with, but runs only
/// once it holds the run slot - which a switch also takes. Checked again
/// there: the environment it was allowed for must still be the active one,
/// and the switch still on.
#[test]
fn a_template_run_is_refused_when_the_environment_or_the_switch_moved_while_it_waited() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(root), pushed(), &first);
    let allowed = ctx.lock().unwrap().clone();
    assert!(allowed.api_writes);

    assert_eq!(still_allowed(&allowed, Some(&first), Some(&allowed)), Ok(()));
    assert_eq!(still_allowed(&allowed, Some(&qa), None), Err(API_WRITES_MOVED.to_string()), "switched");
    let off = BridgeContext { api_writes: false, ..allowed.clone() };
    assert_eq!(still_allowed(&allowed, Some(&first), Some(&off)), Err(API_WRITES_MOVED.to_string()), "turned off");
    let mut elsewhere = allowed.clone();
    elsewhere.gate.env_id = Some(qa.clone());
    assert_eq!(still_allowed(&allowed, Some(&first), Some(&elsewhere)), Err(API_WRITES_MOVED.to_string()));
}

#[test]
fn a_context_with_writes_on_always_names_its_environment() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, _) = two_envs(root);
    let ctx = Mutex::new(BridgeContext::default());
    push_context_with(&ctx, Some(root), pushed(), &first);
    assert_eq!(ctx.lock().unwrap().gate.env_id.as_deref(), Some(first.as_str()));
}
