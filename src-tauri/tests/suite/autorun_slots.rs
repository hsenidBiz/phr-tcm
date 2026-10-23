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
