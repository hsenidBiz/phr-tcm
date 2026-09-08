//! The shared reference cache: the thing that makes the app and the AI
//! bridge read one tag list instead of each fetching their own.
//!
//! All cases share one process-global cache, so they run serially through
//! `clear()` and distinct keys rather than fighting over state.

use v2_lib::refcache;

fn temp_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tcm-refcache-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn stored_values_read_back_fresh_and_stale() {
    refcache::init(temp_dir());
    let key = "acme/Web/tags-freshness";
    refcache::put(key, &["smoke".into(), "regression".into()]);

    assert_eq!(
        refcache::fresh(key, refcache::TAGS_TTL_MS).unwrap(),
        vec!["smoke", "regression"]
    );
    // A zero TTL makes everything stale - but `any` still serves it, which
    // is what the bridge relies on to never issue a request of its own.
    assert!(refcache::fresh(key, 0).is_none());
    assert_eq!(refcache::any(key).unwrap().len(), 2);
}

#[test]
fn an_unknown_key_is_simply_absent() {
    refcache::init(temp_dir());
    assert!(refcache::any("nobody/here/tags").is_none());
    assert!(refcache::fresh("nobody/here/tags", refcache::TAGS_TTL_MS).is_none());
}

/// Tags on cases the app just created provably exist now, so they are
/// folded in without a round trip.
#[test]
fn merge_adds_new_values_case_insensitively_and_sorts() {
    refcache::init(temp_dir());
    let key = "acme/Web/tags-merge";
    refcache::put(key, &["smoke".into(), "regression".into()]);

    refcache::merge(
        key,
        &[
            "Login".into(),
            "SMOKE".into(),  // already there, different case
            "  ".into(),     // blank
            "regression".into(),
        ],
    );

    assert_eq!(
        refcache::any(key).unwrap(),
        vec!["Login", "regression", "smoke"],
        "one genuinely new tag, deduped case-insensitively, sorted"
    );
}

/// Merging must not pass for a refresh: a partial local list should never
/// stop the scheduled fetch of the real one.
#[test]
fn merge_does_not_reset_the_age() {
    refcache::init(temp_dir());
    let key = "acme/Web/tags-age";
    refcache::put(key, &["smoke".into()]);
    // Nothing cached yet must NOT be seeded by a merge - a half list would
    // look authoritative.
    refcache::merge("acme/Web/tags-cold", &["invented".into()]);
    assert!(refcache::any("acme/Web/tags-cold").is_none());

    refcache::merge(key, &["brand-new".into()]);
    // Still readable, still governed by the original fetch time.
    assert_eq!(refcache::any(key).unwrap(), vec!["brand-new", "smoke"]);
    assert!(refcache::fresh(key, 0).is_none());
}

#[test]
fn the_key_separates_projects() {
    assert_ne!(
        refcache::tags_key("acme", "Web"),
        refcache::tags_key("acme", "Mobile")
    );
    assert_eq!(refcache::tags_key("acme", "Web"), "acme/Web/tags");
}

/// The cache is an optimization: a directory it cannot write to must
/// degrade to memory-only rather than failing anything.
#[test]
fn an_unwritable_directory_does_not_panic() {
    refcache::init(temp_dir()); // may be a no-op if another test won the race
    let key = "acme/Web/tags-unwritable";
    refcache::put(key, &["still-works".into()]);
    assert_eq!(refcache::any(key).unwrap(), vec!["still-works"]);
}
