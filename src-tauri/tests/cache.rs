//! The app's one Rust-side cache (src/cache). Every case opens its own
//! `Store` on its own directory, so nothing here shares state with the
//! process-global store or with another case - no "whoever initialised
//! first wins" races.

use std::time::Duration;

use v2_lib::cache::{keys, Store};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tcm-cache-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_stored_value_reads_back_fresh_and_stale() {
    let store = Store::open(Some(&temp_dir("fresh")));
    store.put("k", &vec!["smoke".to_string(), "regression".to_string()]);

    assert_eq!(store.fresh::<Vec<String>>("k", 60_000).unwrap(), vec!["smoke", "regression"]);
    // A zero TTL makes everything stale - but `get` still serves it, which
    // is what the AI bridge relies on to never issue a request of its own.
    assert!(store.fresh::<Vec<String>>("k", 0).is_none());
    assert_eq!(store.get::<Vec<String>>("k").unwrap().len(), 2);
}

#[test]
fn any_serde_type_round_trips_and_a_wrong_shape_is_a_miss() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Suite {
        plan_id: i32,
        name: String,
    }
    let store = Store::open(Some(&temp_dir("typed")));
    store.put("suite", &Suite { plan_id: 9, name: "P".into() });

    assert_eq!(store.get::<Suite>("suite"), Some(Suite { plan_id: 9, name: "P".into() }));
    assert!(store.get::<Vec<String>>("suite").is_none());
}

#[test]
fn an_unknown_key_is_simply_absent() {
    let store = Store::open(Some(&temp_dir("absent")));
    assert!(store.get::<Vec<String>>("nobody").is_none());
    assert!(store.fresh::<Vec<String>>("nobody", 60_000).is_none());
}

#[test]
fn values_survive_a_restart() {
    let dir = temp_dir("restart");
    Store::open(Some(&dir)).put("k", &42u32);
    assert_eq!(Store::open(Some(&dir)).get::<u32>("k"), Some(42));
}

#[test]
fn forget_removes_a_value_from_memory_and_disk() {
    let dir = temp_dir("forget");
    let store = Store::open(Some(&dir));
    store.put("k", &1u8);
    store.forget("k");
    assert!(store.get::<u8>("k").is_none());
    assert!(Store::open(Some(&dir)).get::<u8>("k").is_none());
}

/// An update is new knowledge, not a refresh: the entry keeps the age of
/// its last real fetch, so the scheduled refetch still happens.
#[test]
fn update_changes_the_value_but_not_its_age_and_never_seeds_a_cold_key() {
    let dir = temp_dir("update");
    std::fs::write(
        dir.join("cache.json"),
        r#"{"owner":null,"entries":{"k":{"value":["a"],"at_ms":1}}}"#,
    )
    .unwrap();
    let store = Store::open(Some(&dir));

    store.update::<Vec<String>>("cold", |v| {
        v.push("x".into());
        true
    });
    assert!(store.get::<Vec<String>>("cold").is_none(), "a half list must not look authoritative");

    store.update::<Vec<String>>("k", |v| {
        v.push("b".into());
        true
    });
    assert_eq!(store.get::<Vec<String>>("k").unwrap(), vec!["a", "b"]);
    assert!(store.fresh::<Vec<String>>("k", 60_000).is_none(), "still aged from 1970");
    assert_eq!(Store::open(Some(&dir)).get::<Vec<String>>("k").unwrap(), vec!["a", "b"]);
}

/// The session tier holds values as they are - no serde round trip, so a
/// type with `#[serde(skip)]` fields (TestPlan) loses nothing - and it
/// never reaches the disk.
#[test]
fn session_values_stay_in_memory_whole() {
    #[derive(Clone, Debug, PartialEq)]
    struct Tree(Vec<i32>); // deliberately not serde at all

    let dir = temp_dir("session");
    let store = Store::open(Some(&dir));
    store.session_put("tree", Tree(vec![1, 2]));

    assert_eq!(store.session_fresh::<Tree>("tree", Duration::from_secs(600)), Some(Tree(vec![1, 2])));
    assert!(store.session_fresh::<Tree>("tree", Duration::ZERO).is_none());
    assert!(store.session_fresh::<String>("tree", Duration::from_secs(600)).is_none());
    assert!(Store::open(Some(&dir)).session_fresh::<Tree>("tree", Duration::from_secs(600)).is_none());
}

/// Keys carry org and project, which is not the same as carrying the
/// PERSON: a second account on the same Windows profile must not inherit
/// the first one's tags or suite ids.
#[test]
fn signing_in_as_someone_else_drops_the_previous_accounts_cache() {
    let dir = temp_dir("claim");
    let store = Store::open(Some(&dir));
    store.claim_for(Some("first@example.com"));
    store.put("tags:acme/Web", &vec!["smoke".to_string()]);
    store.session_put("tree", 7u8);

    store.claim_for(Some("first@example.com"));
    store.claim_for(None); // no account yet must not wipe anything
    assert!(store.get::<Vec<String>>("tags:acme/Web").is_some());
    assert_eq!(store.session_fresh::<u8>("tree", Duration::from_secs(600)), Some(7));

    store.claim_for(Some("second@example.com"));
    assert!(store.get::<Vec<String>>("tags:acme/Web").is_none());
    assert!(store.session_fresh::<u8>("tree", Duration::from_secs(600)).is_none());

    // The owner is remembered across a restart...
    store.put("k", &1u8);
    let reopened = Store::open(Some(&dir));
    reopened.claim_for(Some("second@example.com"));
    assert_eq!(reopened.get::<u8>("k"), Some(1));
    // ...without the address itself ever reaching the disk.
    let raw = std::fs::read_to_string(dir.join("cache.json")).unwrap();
    assert!(!raw.contains("example.com"), "{raw}");
}

/// The first launch after the guard ships has data but no owner. It was
/// the same person's data in practice; wiping it would cost a one-minute
/// suite scan per PBI for nothing.
#[test]
fn an_unowned_cache_is_adopted_not_wiped() {
    let dir = temp_dir("adopt");
    let store = Store::open(Some(&dir));
    store.put("k", &1u8);
    store.claim_for(Some("first@example.com"));
    assert_eq!(store.get::<u8>("k"), Some(1));
}

#[test]
fn the_old_tag_and_suite_files_are_carried_over_once() {
    let dir = temp_dir("legacy");
    std::fs::write(
        dir.join("reference-cache.json"),
        r#"{"acme/Web/tags":{"values":["smoke"],"at_ms":1},
            "acme/Web/assigned-seen":{"values":["12"],"at_ms":1},
            "mystery":{"values":[],"at_ms":1}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("suite-cache.json"),
        r#"{"http://x|acme|Web|42":{"plan_id":9,"plan_name":"P","suite_id":91,"created_plan":false}}"#,
    )
    .unwrap();

    let store = Store::open(Some(&dir));

    assert_eq!(store.get::<Vec<String>>(&keys::tags("acme", "Web")), Some(vec!["smoke".to_string()]));
    assert!(
        store.fresh::<Vec<String>>(&keys::tags("acme", "Web"), 60_000).is_none(),
        "migrated tags keep their original age"
    );
    assert_eq!(
        store.get::<Vec<String>>(&keys::assigned_seen("acme", "Web")),
        Some(vec!["12".to_string()])
    );
    let suite: v2_lib::ado_testplan::EnsuredSuite =
        store.get(&keys::suite("http://x", "acme", "Web", 42)).unwrap();
    assert_eq!((suite.plan_id, suite.suite_id), (9, 91));

    assert!(dir.join("cache.json").exists());
    assert!(!dir.join("reference-cache.json").exists());
    assert!(!dir.join("suite-cache.json").exists());
}

/// The cache is an optimisation: a corrupt file or no directory at all
/// degrades to an empty, memory-only cache - never a panic.
#[test]
fn a_corrupt_file_or_no_directory_still_works() {
    let dir = temp_dir("corrupt");
    std::fs::write(dir.join("cache.json"), "{not json").unwrap();
    let store = Store::open(Some(&dir));
    assert!(store.get::<u8>("k").is_none());
    store.put("k", &3u8);
    assert_eq!(store.get::<u8>("k"), Some(3));

    let memory_only = Store::open(None);
    memory_only.put("k", &4u8);
    assert_eq!(memory_only.get::<u8>("k"), Some(4));
}

#[test]
fn keys_separate_projects_and_kinds() {
    assert_ne!(keys::tags("acme", "Web"), keys::tags("acme", "Mobile"));
    assert_ne!(keys::tags("acme", "Web"), keys::assigned_seen("acme", "Web"));
    assert_ne!(keys::suite("http://a", "acme", "Web", 1), keys::suite("http://b", "acme", "Web", 1));
    assert_eq!(keys::tags("acme", "Web"), "tags:acme/Web");
}

/// Tags on cases the app just created provably exist now, so they are
/// folded into the cached list without a round trip - and without
/// passing for a refresh.
#[test]
fn new_tags_merge_case_insensitively_sorted_and_keep_the_age() {
    use v2_lib::commands::discovery::add_new_tags;
    let dir = temp_dir("tags");
    std::fs::write(
        dir.join("cache.json"),
        r#"{"owner":null,"entries":{"tags:acme/Web":{"value":["smoke","regression"],"at_ms":1}}}"#,
    )
    .unwrap();
    let store = Store::open(Some(&dir));
    let key = keys::tags("acme", "Web");

    store.update::<Vec<String>>(&key, |tags| {
        add_new_tags(
            tags,
            &["Login".into(), "SMOKE".into(), "  ".into(), "regression".into()],
        )
    });

    assert_eq!(store.get::<Vec<String>>(&key).unwrap(), vec!["Login", "regression", "smoke"]);
    assert!(store.fresh::<Vec<String>>(&key, 60_000).is_none(), "a merge is not a refresh");

    let mut unchanged = vec!["smoke".to_string()];
    assert!(!add_new_tags(&mut unchanged, &["Smoke".into()]), "nothing new, nothing written");
}

/// One cache means one: a module that needs to remember data uses
/// `crate::cache`, not a map of its own in a static. If this fails, move
/// that data onto the cache (a key in cache/keys.rs) instead of allowing it.
#[test]
fn no_module_keeps_a_private_cache_map() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "cache") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !path.extension().is_some_and(|e| e == "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let t = line.trim_start();
                if !(t.starts_with("static ") || t.starts_with("pub static ")) {
                    continue;
                }
                // A static's type can wrap onto following lines: read on
                // to the terminating `;`.
                let decl = lines[i..lines.len().min(i + 6)].join(" ");
                if decl.split(';').next().unwrap_or("").contains("HashMap") {
                    offenders.push(format!("{}:{}", path.strip_prefix(&src).unwrap().display(), i + 1));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "keep cached data in crate::cache, not a private static map: {offenders:?}");
}
