//! The AI Bridge switches belong to an environment. What the bridge
//! enforces - database writes, API template writes, and whether a database
//! change runs without asking - must follow the ACTIVE environment, and a
//! switch of environment must never leave the bridge allowing a write the
//! new environment has off: not before the switch, not during it, and not
//! because a push from the webview arrived late.

use std::path::Path;
use std::sync::{Arc, Mutex};
use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::commands::ai_bridge::push_context_with;
use v2_lib::commands::ai_tools::{set_auto_approve_with, AUTO_APPROVE_ENV_MOVED};
use v2_lib::commands::environments::{list_adopting, move_bridge_to, switch_with};
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
    let view = switch_with(&root, &store, &qa, Some(&ctx), after).await.unwrap();
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
