# Unified Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each side of the app gets exactly one cache. Everything that needs to remember data imports that cache instead of writing its own.

**Architecture:** The webview and the Rust backend are separate processes and cannot share memory, so each gets one cache module:
- **Webview:** `src/lib/cache.ts` merges `localCache.ts` and `persistentQuery.ts`, and adds named keys (`cacheKeys`) and more shelf-life presets (`CACHE`).
- **Rust:** `src-tauri/src/cache/` is a generic, typed store. It replaces `refcache.rs`, the suite map in `ado_testplan/mod.rs` and the suite-tree map in `ai_bridge.rs`. Every Rust key and TTL lives in `cache/keys.rs`.

Both caches wipe themselves when a different account signs in. A source-scanning test on each side fails if a private cache reappears.

**Tech Stack:** Rust (serde, serde_json, std `OnceLock`/`Mutex`), React 19 + TypeScript, TanStack React Query, vitest (jsdom).

**Spec:** No separate design doc. The decisions below were settled with the user on 2026-09-14 and are the spec.

## Decisions (the spec)

1. **One cache per side, not one Rust store behind IPC.** The webview keeps `localStorage` because `initialData` must be read synchronously so screens paint instantly. Data both sides need (tags, resolved suites) stays in Rust, as it does today.
2. **The Rust cache gets the same account guard as the webview.** When a different account signs in, everything cached in Rust is dropped. An *unowned* cache (the first launch after this ships, which carries the migrated legacy data) is adopted, not wiped.
3. **Existing user caches survive the upgrade:**
   - Webview key strings stay byte-for-byte identical.
   - Rust migrates `reference-cache.json` and `suite-cache.json` into the new `cache.json` once, then removes the old files.
4. **Behaviour stays the same except for the account guard:**
   - Orgs, projects and members are still served from disk for 24h with no request. They now also paint instantly from the seed.
   - The work-item detail size cap (400 KB) stays.
   - PR pipelines of finished PRs are still kept 30 days.
   - The AI bridge's suite tree stays memory-only for 10 minutes.

## Global Constraints

- **No DELETE calls** to Azure DevOps anywhere. Removing a local cache *file* with `std::fs::remove_file` is fine; that invariant is about the ADO client.
- **Every Rust test is an integration test under `src-tauri/tests/`.** Never add a `#[cfg(test)]` module inside `src/`.
- **Never hand-edit `src/bindings.ts`.** No command signature changes in this plan, so it must not change. If it shows as modified with identical content (a line-ending artifact), compare `git hash-object` against HEAD and `git checkout -- src/bindings.ts`.
- **Never weaken `src/ui-consistency.test.ts`.**
- **Don't run `npx prettier`.** The repo has no prettier config; match the surrounding style by hand.
- **Run one build or test command at a time.** The machine is shared with the user.
- If `npm run tauri dev` is running and a cargo build fails on a locked file, **ask the user to close it; never kill it.**
- **Commit with a Bash heredoc:** `git commit -F - <<'EOF' … EOF`. End every message with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Confirm with `git log -1`.
- All commands run from the repository root unless a step says `cd src-tauri`.
- **No release is part of this plan.** Shipping is a separate request.

---

## File Structure

**Rust**
- Create `src-tauri/src/cache/mod.rs`: the store.
  - Durable tier: `get`, `fresh`, `put`, `update`, `forget`.
  - Session tier: `session_fresh`, `session_put`.
  - Also: `claim_for`, `clear`, `init`, plus the one-time legacy migration.
- Create `src-tauri/src/cache/keys.rs`: every cache key builder and TTL on the Rust side.
- Create `src-tauri/tests/cache.rs`: store behaviour, migration, the tag merge, and the "no private cache" guard.
- Delete:
  - `src-tauri/src/refcache.rs`
  - `src-tauri/tests/refcache.rs`
- Modify:
  - `src-tauri/src/lib.rs`: module list and `init`.
  - `src-tauri/src/commands/auth.rs`: claim the cache on sign-in.
  - `src-tauri/src/commands/discovery.rs`: tags go through the cache; add `add_new_tags`.
  - `src-tauri/src/commands/queue.rs`: tag merge through `cache::update`.
  - `src-tauri/src/ai_bridge.rs`: tags and the suite tree go through the cache.
  - `src-tauri/src/assigned_watch.rs`: the baseline goes through the cache.
  - `src-tauri/src/ado_testplan/mod.rs`: the suite wrappers become one-liners over the cache.
  - `src-tauri/tests/ado_testplan.rs`: drop the obsolete disk test.
  - `src-tauri/tests/ai_bridge.rs`: new key and API.

**Webview**
- Create `src/lib/cache.ts`: the whole webview cache.
- Create `src/lib/cache.test.ts`: the merged tests, the new behaviour, and the "one cache" guard.
- Delete:
  - `src/lib/localCache.ts`
  - `src/lib/localCache.test.ts`
  - `src/lib/persistentQuery.ts`
  - `src/lib/persistentQuery.test.ts`
- Modify:
  - Import paths and keys: `src/App.tsx`, `src/components/CommentsPanel.tsx`, `src/screens/ManageCases/index.tsx`, `src/screens/RunPanel/index.tsx`, `src/screens/Suites.tsx`, `src/screens/WorkBoard.tsx`.
  - Move onto `persistentQuery` or `cacheKeys`: `src/components/ContextBar.tsx`, `src/components/CommandPalette.tsx`, `src/screens/CreateWorkItem.tsx`, `src/components/WorkItemDrawer.tsx`, `src/screens/PrPanel.tsx`.

**Docs**
- Modify `CLAUDE.md`: add one convention bullet saying where caching lives and how to use it.

---

### Task 1: The Rust cache store

**Files:**
- Create: `src-tauri/src/cache/mod.rs`
- Create: `src-tauri/src/cache/keys.rs`
- Create: `src-tauri/tests/cache.rs`
- Modify: `src-tauri/src/lib.rs` (module list only; the `init` call moves in Task 2)

**Interfaces:**
- Consumes: nothing new. `v2_lib::ado_testplan::EnsuredSuite` (already `Serialize + Deserialize + PartialEq`) is used in the migration test.
- Produces (all used in Task 2):
  - `v2_lib::cache::Store` with these methods:
    - `Store::open(dir: Option<&Path>) -> Store`
    - `get<T: DeserializeOwned>(&self, key: &str) -> Option<T>`
    - `fresh<T: DeserializeOwned>(&self, key: &str, ttl_ms: u64) -> Option<T>`
    - `put<T: Serialize>(&self, key: &str, value: &T)`
    - `update<T: Serialize + DeserializeOwned>(&self, key: &str, f: impl FnOnce(&mut T) -> bool)`
    - `forget(&self, key: &str)`
    - `session_fresh<T: Clone + 'static>(&self, key: &str, ttl: Duration) -> Option<T>`
    - `session_put<T: Send + 'static>(&self, key: &str, value: T)`
    - `claim_for(&self, account: Option<&str>)`
    - `clear(&self)`
  - Free functions of the same names that act on the process-global store, plus `cache::init(dir: PathBuf)` and `cache::now_ms() -> u64`.
  - `v2_lib::cache::keys`:
    - `TAGS_TTL_MS: u64`, `SUITE_TREE_TTL: Duration`
    - `tags(org, project) -> String`
    - `assigned_seen(org, project) -> String`
    - `suite(base_url, org, project, pbi_id: i32) -> String`
    - `suite_tree(base_url, org, project) -> String`

- [ ] **Step 0: Branch**

```bash
git checkout -b refactor/unified-cache
```

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/tests/cache.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests and confirm they fail**

```bash
cd src-tauri && cargo test --test cache
```

Expected: a compile error, ``unresolved import `v2_lib::cache` ``.

- [ ] **Step 3: Write `keys.rs`**

Create `src-tauri/src/cache/keys.rs`:

```rust
//! Every key the Rust cache holds, and how long each kind stays fresh -
//! in one file so two features can never collide on a key without it
//! being visible here.
//!
//! Each key starts with its kind (`tags:`, `suite:` ...) so kinds cannot
//! overlap whatever an org or project happens to be called.

use std::time::Duration;

/// Tags change when someone types a new one - slow enough that a stale
/// read is harmless, fast enough that a week would annoy.
pub const TAGS_TTL_MS: u64 = 6 * 60 * 60 * 1000;

/// How long the AI bridge reuses a scanned plan tree. A project can hold
/// hundreds of plans and the scan is one request per plan; an assistant
/// that lists suites and then reads three of them must not pay for the
/// scan three times.
pub const SUITE_TREE_TTL: Duration = Duration::from_secs(10 * 60);

/// A project's tag names, shared by the UI and the AI bridge.
pub fn tags(org: &str, project: &str) -> String {
    format!("tags:{org}/{project}")
}

/// Work-item ids already announced as newly assigned (assigned_watch.rs).
pub fn assigned_seen(org: &str, project: &str) -> String {
    format!("assigned-seen:{org}/{project}")
}

/// A PBI's resolved requirement suite, shared by the upload, Run Tests and
/// the AI bridge. The client's base_url is in the key so parallel tests on
/// different mock servers cannot poison each other.
pub fn suite(base_url: &str, org: &str, project: &str, pbi_id: i32) -> String {
    format!("suite:{base_url}|{org}|{project}|{pbi_id}")
}

/// The AI bridge's scanned plans-and-suites tree (session tier only).
pub fn suite_tree(base_url: &str, org: &str, project: &str) -> String {
    format!("suite-tree:{base_url}|{org}|{project}")
}
```

- [ ] **Step 4: Write the store**

Create `src-tauri/src/cache/mod.rs`:

```rust
//! The Rust side's one cache. Anything that needs to remember data
//! between calls - or between launches - uses this instead of a map of
//! its own (tests/cache.rs fails on a private cache static).
//!
//! It lives below both consumers on purpose: the UI's commands and the AI
//! bridge read the SAME entries, so whoever asks first is the only one
//! who ever pays for the request. The webview has its own cache
//! (src/lib/cache.ts) for what only the screens need; the two are
//! separate processes and share nothing.
//!
//! Two tiers:
//! - durable (`get`/`fresh`/`put`/`update`/`forget`): serde values in one
//!   small JSON file, held in memory so repeat reads never touch the disk;
//! - session (`session_fresh`/`session_put`): values kept exactly as they
//!   are, in memory only, for data too big to write on every change or
//!   with `#[serde(skip)]` fields a round trip would lose.
//!
//! Every key and TTL lives in `keys.rs`. Like the webview cache, it drops
//! everything when a different account signs in (`claim_for`).

pub mod keys;

use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

const FILE: &str = "cache.json";

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    value: serde_json::Value,
    /// Unix epoch milliseconds of the last successful fetch.
    at_ms: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Disk {
    /// Non-reversible tag of the account the entries belong to.
    owner: Option<String>,
    entries: HashMap<String, Entry>,
}

#[derive(Default)]
struct Inner {
    disk: Disk,
    session: HashMap<String, (Instant, Box<dyn Any + Send>)>,
}

pub struct Store {
    file: Option<PathBuf>,
    inner: Mutex<Inner>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// FNV-1a: the identity check never needs the address itself on disk.
/// Collisions only cost a needless wipe.
fn owner_tag(account: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in account.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

impl Store {
    /// Open the cache in `dir`, or memory-only with `None`. A corrupt or
    /// absent file simply starts empty - never an error.
    pub fn open(dir: Option<&Path>) -> Store {
        let store = Store {
            file: dir.map(|d| d.join(FILE)),
            inner: Mutex::new(Inner::default()),
        };
        let Some(dir) = dir else { return store };
        if let Some(disk) = read_json::<Disk>(&dir.join(FILE)) {
            store.lock().disk = disk;
        } else if let Some(disk) = legacy::read(dir) {
            let mut inner = store.lock();
            inner.disk = disk;
            if store.persist(&inner.disk) {
                legacy::remove(dir);
            }
        }
        store
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic elsewhere while holding the lock must not take the cache
        // down with it: the map is still structurally sound.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn persist(&self, disk: &Disk) -> bool {
        let Some(path) = &self.file else { return false };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string(disk) {
            Ok(s) => std::fs::write(path, s).is_ok(),
            Err(_) => false,
        }
    }

    /// Whatever is cached, however old.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let inner = self.lock();
        serde_json::from_value(inner.disk.entries.get(key)?.value.clone()).ok()
    }

    /// The cached value if it is younger than `ttl_ms`.
    pub fn fresh<T: DeserializeOwned>(&self, key: &str, ttl_ms: u64) -> Option<T> {
        let inner = self.lock();
        let entry = inner.disk.entries.get(key)?;
        if now_ms().saturating_sub(entry.at_ms) >= ttl_ms {
            return None;
        }
        serde_json::from_value(entry.value.clone()).ok()
    }

    /// Store a freshly fetched value, replacing whatever was there.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) {
        let Ok(value) = serde_json::to_value(value) else { return };
        let mut inner = self.lock();
        inner.disk.entries.insert(key.to_string(), Entry { value, at_ms: now_ms() });
        self.persist(&inner.disk);
    }

    /// Change a cached value in place. `f` returns whether it changed
    /// anything. Leaves the entry's age alone - new local knowledge is not
    /// a refresh - and no-ops on a cold key, so a partial value is never
    /// mistaken for a fetched one.
    pub fn update<T: Serialize + DeserializeOwned>(&self, key: &str, f: impl FnOnce(&mut T) -> bool) {
        let mut inner = self.lock();
        let Some(entry) = inner.disk.entries.get_mut(key) else { return };
        let Ok(mut value) = serde_json::from_value::<T>(entry.value.clone()) else { return };
        if !f(&mut value) {
            return;
        }
        let Ok(json) = serde_json::to_value(&value) else { return };
        entry.value = json;
        self.persist(&inner.disk);
    }

    /// Drop one key from both tiers.
    pub fn forget(&self, key: &str) {
        let mut inner = self.lock();
        inner.session.remove(key);
        if inner.disk.entries.remove(key).is_some() {
            self.persist(&inner.disk);
        }
    }

    /// A session value younger than `ttl`, as it was stored.
    pub fn session_fresh<T: Clone + 'static>(&self, key: &str, ttl: Duration) -> Option<T> {
        let inner = self.lock();
        let (at, value) = inner.session.get(key)?;
        if at.elapsed() >= ttl {
            return None;
        }
        // `**value` is the `dyn Any`: downcasting the Box itself would ask
        // "is this a Box?" and always miss.
        (**value).downcast_ref::<T>().cloned()
    }

    /// Keep a value in memory for this run only.
    pub fn session_put<T: Send + 'static>(&self, key: &str, value: T) {
        self.lock()
            .session
            .insert(key.to_string(), (Instant::now(), Box::new(value)));
    }

    /// Hand the cache to the signed-in account. A different account than
    /// the recorded owner drops everything; an unowned cache (first launch
    /// with this guard, carrying migrated data) is adopted as is. `None`
    /// - no account known yet - changes nothing.
    pub fn claim_for(&self, account: Option<&str>) {
        let Some(account) = account else { return };
        let tag = owner_tag(account);
        let mut inner = self.lock();
        // Compare first, into a plain bool, so no borrow of the owner is
        // alive while the entries are cleared.
        match inner.disk.owner.as_deref().map(|owner| owner == tag) {
            Some(true) => return,
            Some(false) => {
                inner.disk.entries.clear();
                inner.session.clear();
            }
            None => {}
        }
        inner.disk.owner = Some(tag);
        self.persist(&inner.disk);
    }

    /// Drop every entry in both tiers (the owner is kept).
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.disk.entries.clear();
        inner.session.clear();
        self.persist(&inner.disk);
    }
}

/// The two single-purpose files this module replaced. Read once, folded
/// into `cache.json`, then removed - so nobody's tags refetch or suites
/// rescan just because the app updated.
mod legacy {
    use std::collections::HashMap;
    use std::path::Path;

    use super::{keys, now_ms, read_json, Disk, Entry};

    const REFERENCE: &str = "reference-cache.json";
    const SUITES: &str = "suite-cache.json";

    #[derive(serde::Deserialize)]
    struct OldEntry {
        values: Vec<String>,
        at_ms: u64,
    }

    pub fn read(dir: &Path) -> Option<Disk> {
        let reference = read_json::<HashMap<String, OldEntry>>(&dir.join(REFERENCE));
        let suites = read_json::<HashMap<String, serde_json::Value>>(&dir.join(SUITES));
        if reference.is_none() && suites.is_none() {
            return None;
        }
        let mut disk = Disk::default();
        for (old, entry) in reference.unwrap_or_default() {
            // "org/project/tags" and "org/project/assigned-seen"; org and
            // project names cannot contain '/'.
            let key = if let Some(scope) = old.strip_suffix("/tags") {
                scope.split_once('/').map(|(o, p)| keys::tags(o, p))
            } else if let Some(scope) = old.strip_suffix("/assigned-seen") {
                scope.split_once('/').map(|(o, p)| keys::assigned_seen(o, p))
            } else {
                None
            };
            let (Some(key), Ok(value)) = (key, serde_json::to_value(entry.values)) else { continue };
            disk.entries.insert(key, Entry { value, at_ms: entry.at_ms });
        }
        let now = now_ms();
        for (old, value) in suites.unwrap_or_default() {
            // "base_url|org|project|pbi"
            let parts: Vec<&str> = old.split('|').collect();
            let [base, org, project, pbi] = parts.as_slice() else { continue };
            let Ok(pbi) = pbi.parse::<i32>() else { continue };
            disk.entries.insert(keys::suite(base, org, project, pbi), Entry { value, at_ms: now });
        }
        Some(disk)
    }

    pub fn remove(dir: &Path) {
        let _ = std::fs::remove_file(dir.join(REFERENCE));
        let _ = std::fs::remove_file(dir.join(SUITES));
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static GLOBAL: OnceLock<Store> = OnceLock::new();

/// Point the app's cache at its directory. Called once during setup,
/// before anything reads it; later calls are ignored. Without it (the
/// test binaries) the global cache is memory-only.
pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn global() -> &'static Store {
    GLOBAL.get_or_init(|| Store::open(DIR.get().map(PathBuf::as_path)))
}

pub fn get<T: DeserializeOwned>(key: &str) -> Option<T> {
    global().get(key)
}

pub fn fresh<T: DeserializeOwned>(key: &str, ttl_ms: u64) -> Option<T> {
    global().fresh(key, ttl_ms)
}

pub fn put<T: Serialize>(key: &str, value: &T) {
    global().put(key, value)
}

pub fn update<T: Serialize + DeserializeOwned>(key: &str, f: impl FnOnce(&mut T) -> bool) {
    global().update(key, f)
}

pub fn forget(key: &str) {
    global().forget(key)
}

pub fn session_fresh<T: Clone + 'static>(key: &str, ttl: Duration) -> Option<T> {
    global().session_fresh(key, ttl)
}

pub fn session_put<T: Send + 'static>(key: &str, value: T) {
    global().session_put(key, value)
}

pub fn claim_for(account: Option<&str>) {
    global().claim_for(account)
}

pub fn clear() {
    global().clear()
}
```

In `src-tauri/src/lib.rs`, add the module after `pub mod backup;`:

```rust
pub mod backup;
pub mod cache;
pub mod capture;
```

- [ ] **Step 5: Run the tests and confirm they pass**

```bash
cd src-tauri && cargo test --test cache
```

Expected: all 12 tests pass, with no warnings from `src/cache`. (`refcache` still exists and is still used; Task 2 removes it.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/cache src-tauri/src/lib.rs src-tauri/tests/cache.rs
git commit -F - <<'EOF'
refactor(v2): one generic Rust cache store with an account guard

Typed durable and session tiers, every key and TTL in cache/keys.rs,
a one-time migration of the old tag and suite files, and the same
"different account signs in, drop everything" rule the webview has.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

---

### Task 2: Move every Rust cache onto the store

**Files:**
- Modify: `src-tauri/tests/cache.rs` (add the guard test and the tag-merge test)
- Modify: `src-tauri/src/lib.rs:33` and `:295-301`
- Modify: `src-tauri/src/commands/auth.rs:29-40`
- Modify: `src-tauri/src/commands/discovery.rs:55-91`
- Modify: `src-tauri/src/commands/queue.rs:850-855`
- Modify: `src-tauri/src/ai_bridge.rs:1235-1251`, `:1380`, `:1680-1718`
- Modify: `src-tauri/src/assigned_watch.rs:28-32`, `:115-119`
- Modify: `src-tauri/src/ado_testplan/mod.rs:250-318`
- Modify: `src-tauri/tests/ado_testplan.rs:686-712` (delete one test)
- Modify: `src-tauri/tests/ai_bridge.rs:1356-1367`
- Delete: `src-tauri/src/refcache.rs`, `src-tauri/tests/refcache.rs`

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces:
  - `v2_lib::commands::discovery::add_new_tags(tags: &mut Vec<String>, extra: &[String]) -> bool`
  - Unchanged public signatures (their callers don't change): `ado_testplan::{cached_suite, remember_suite, forget_suite}`.
  - Removed: `v2_lib::refcache` (whole module) and `ado_testplan::init_suite_cache`.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/cache.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests and confirm they fail**

```bash
cd src-tauri && cargo test --test cache
```

Expected: a compile error, ``cannot find function `add_new_tags` in module `discovery` ``. (Once that exists, the guard test fails and lists `ai_bridge.rs:1691`, `ado_testplan/mod.rs:262` and `refcache.rs:24`.)

- [ ] **Step 3: Tags → the cache (`discovery.rs`, `queue.rs`, `ai_bridge.rs`)**

In `src-tauri/src/commands/discovery.rs`, change the doc line `/// assistant asking for tags costs nothing extra (see refcache.rs).` to `/// assistant asking for tags costs nothing extra (see cache/mod.rs).`

Then replace the body of `list_project_tags` with:

```rust
    let key = crate::cache::keys::tags(&organization, &project);
    if let Some(v) = crate::cache::fresh::<Vec<String>>(&key, crate::cache::keys::TAGS_TTL_MS) {
        return Ok(v);
    }
    if let Some(stale) = crate::cache::get::<Vec<String>>(&key) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Best effort: a failed background refresh just leaves the
            // stale list in place until the next attempt.
            if let Ok(token) = get_fresh_token(&app).await {
                if let Ok(fresh) = ado::AdoClient::new(token)
                    .get_tags(&organization, &project)
                    .await
                {
                    crate::cache::put(&key, &fresh);
                }
            }
        });
        return Ok(stale);
    }
    let token = get_fresh_token(&app).await?;
    let tags = ado::AdoClient::new(token).get_tags(&organization, &project).await?;
    crate::cache::put(&key, &tags);
    Ok(tags)
}

/// Fold tags the app just learned about locally - carried by test cases it
/// created, so they provably exist now - into a cached tag list.
/// Case-insensitively deduplicated and sorted; returns whether anything was
/// added. Applied through `cache::update`, which leaves the entry's age
/// alone and never seeds a cold key with a partial list.
pub fn add_new_tags(tags: &mut Vec<String>, extra: &[String]) -> bool {
    let mut added = false;
    for v in extra {
        let v = v.trim();
        if v.is_empty() || tags.iter().any(|e| e.eq_ignore_ascii_case(v)) {
            continue;
        }
        tags.push(v.to_string());
        added = true;
    }
    if added {
        tags.sort_by_key(|v| v.to_lowercase());
    }
    added
}
```

(The closing `}` of `list_project_tags` is the first `}` shown above. Replace from `let key = crate::refcache::tags_key` through that function's closing brace.)

In `src-tauri/src/commands/queue.rs`, replace:

```rust
        crate::refcache::merge(
            &crate::refcache::tags_key(&organization, &project),
            &created,
        );
```

with:

```rust
        crate::cache::update::<Vec<String>>(
            &crate::cache::keys::tags(&organization, &project),
            |tags| crate::commands::discovery::add_new_tags(tags, &created),
        );
```

In `src-tauri/src/ai_bridge.rs`, in the doc comment above `async fn tags`, change `(refcache.rs)` to `(cache/mod.rs)`. Then replace:

```rust
    let key = crate::refcache::tags_key(&ctx.org, &ctx.project);
    let (values, source) = match crate::refcache::any(&key) {
```

with:

```rust
    let key = crate::cache::keys::tags(&ctx.org, &ctx.project);
    let (values, source) = match crate::cache::get::<Vec<String>>(&key) {
```

In the same function, change `crate::refcache::put(&key, &v);` to `crate::cache::put(&key, &v);`. Then replace:

```rust
    let tag_lines = match crate::refcache::any(&crate::refcache::tags_key(&ctx.org, &ctx.project)) {
```

with:

```rust
    let tag_lines = match crate::cache::get::<Vec<String>>(&crate::cache::keys::tags(&ctx.org, &ctx.project)) {
```

- [ ] **Step 4: Suite tree → the session tier (`ai_bridge.rs`)**

Delete the doc comment, `const SUITE_TREE_TTL`, and `fn suite_tree_cache()` (from `/// How long a scanned plan tree is reused` through the closing `}` of `suite_tree_cache`). Keep `type SuiteTree = Vec<crate::ado_testplan::PlanWithSuites>;`.

Replace `async fn suite_tree` with:

```rust
/// The plans and suites the Test Suites tab shows, cached in memory per org
/// and project for `cache::keys::SUITE_TREE_TTL`; `refresh=true` reads them
/// again. Session tier: the tree is large, and TestPlan's `#[serde(skip)]`
/// fields would not survive a trip through the disk.
async fn suite_tree(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    refresh: bool,
) -> Result<SuiteTree, crate::ado::AdoError> {
    let key = crate::cache::keys::suite_tree(&client.base_url, &ctx.org, &ctx.project);
    if !refresh {
        if let Some(tree) = crate::cache::session_fresh::<SuiteTree>(&key, crate::cache::keys::SUITE_TREE_TTL) {
            return Ok(tree);
        }
    }
    let tree = client.list_plans_with_suites(&ctx.org, &ctx.project).await?;
    crate::cache::session_put(&key, tree.clone());
    Ok(tree)
}
```

- [ ] **Step 5: Resolved suites → the durable tier (`ado_testplan/mod.rs`)**

Keep the first paragraph of the doc comment that starts above `static SUITE_DIR` (why resolving is slow and why the ids are cached). Delete these, from `static SUITE_DIR` through the end of `forget_suite`:
- `static SUITE_DIR`, `static SUITE_MEM` and `const SUITE_CACHE_FILE`
- `init_suite_cache`, `suite_file`, `suite_cache`, `persist_suites` and `suite_key`
- the old bodies of the three public functions

Put this in their place, directly under the kept doc paragraph:

```rust
pub fn cached_suite(base_url: &str, org: &str, project: &str, pbi_id: i32) -> Option<EnsuredSuite> {
    crate::cache::get(&crate::cache::keys::suite(base_url, org, project, pbi_id))
}

pub fn remember_suite(base_url: &str, org: &str, project: &str, pbi_id: i32, suite: &EnsuredSuite) {
    crate::cache::put(&crate::cache::keys::suite(base_url, org, project, pbi_id), suite);
}

pub fn forget_suite(base_url: &str, org: &str, project: &str, pbi_id: i32) {
    crate::cache::forget(&crate::cache::keys::suite(base_url, org, project, pbi_id));
}
```

Then edit the kept doc paragraph: change "written to disk beside the tag cache" to "kept in the app's cache (cache/mod.rs), on disk". Also delete its last sentence about base_url being in the key; `keys::suite` documents that now.

- [ ] **Step 6: Assigned baseline → the cache (`assigned_watch.rs`)**

Delete `fn seen_key` and its two-line doc comment (`/// Ids already announced, per org/project. ...`). In `check_once`, replace:

```rust
    let key = seen_key(org, project);
    let (fresh, next) = newly_assigned(&current, crate::refcache::any(&key));
    crate::refcache::put(&key, &next);
```

with:

```rust
    // The announced ids are kept in the app's cache, on disk, so a restart
    // doesn't re-announce everything.
    let key = crate::cache::keys::assigned_seen(org, project);
    let (fresh, next) = newly_assigned(&current, crate::cache::get::<Vec<String>>(&key));
    crate::cache::put(&key, &next);
```

- [ ] **Step 7: Wire up `init` and the account claim (`lib.rs`, `auth.rs`); delete refcache**

In `src-tauri/src/lib.rs`:
- Delete the line `pub mod refcache;`.
- Replace:

```rust
            // Reference data (project tags) cached on disk and shared by the
            // UI and the AI bridge - see refcache.rs.
            if let Ok(dir) = app.path().app_data_dir() {
                refcache::init(dir.clone());
                // Resolved requirement suites, shared by the upload, Run
                // Tests and the AI bridge, and kept across restarts.
                ado_testplan::init_suite_cache(dir.clone());
```

with:

```rust
            // The app's one Rust-side cache (project tags, resolved suites,
            // the assigned-items baseline), on disk and shared by the UI and
            // the AI bridge - see cache/mod.rs.
            if let Ok(dir) = app.path().app_data_dir() {
                cache::init(dir.clone());
```

In `src-tauri/src/commands/auth.rs`, in `sign_in`, replace:

```rust
    crate::applog::info("Signed in to Azure DevOps");
    let state = app.state::<Mutex<auth::AuthState>>();
```

with:

```rust
    crate::applog::info("Signed in to Azure DevOps");
    // Before the tokens are visible to anything that reads the cache: a
    // different account than last time must not be served the previous
    // one's tags or suite ids (the webview's cache does the same).
    crate::cache::claim_for(tokens.account.as_deref());
    let state = app.state::<Mutex<auth::AuthState>>();
```

Delete the old module and its tests:

```bash
git rm src-tauri/src/refcache.rs src-tauri/tests/refcache.rs
```

- [ ] **Step 8: Update the remaining tests**

In `src-tauri/tests/ado_testplan.rs`, delete the whole `the_suite_cache_is_written_to_disk` test and its doc comment (`/// The cache survives a restart: ...` through the test's closing `}`). Persistence is covered by `tests/cache.rs::values_survive_a_restart`. Keep `the_suite_cache_is_shared_keyed_and_forgettable`; it still passes unchanged through the wrappers.

In `src-tauri/tests/ai_bridge.rs`, in `a_query_less_get_tags_is_capped_and_a_query_still_searches_everything`, replace:

```rust
    // other test's way (the refcache is process-global).
```
```rust
    let key = v2_lib::refcache::tags_key("cap-org", "CapProj");
    let values: Vec<String> = (0..350).map(|i| format!("tag-{i:03}")).collect();
    v2_lib::refcache::put(&key, &values);
```

with:

```rust
    // other test's way (the cache is process-global).
```
```rust
    let key = v2_lib::cache::keys::tags("cap-org", "CapProj");
    let values: Vec<String> = (0..350).map(|i| format!("tag-{i:03}")).collect();
    v2_lib::cache::put(&key, &values);
```

- [ ] **Step 9: Check that nothing still names the old caches**

Use the Grep tool (not bash grep) for `refcache|init_suite_cache|suite_tree_cache|SUITE_MEM|seen_key` in `src-tauri/`.

Expected: no matches.

- [ ] **Step 10: Run the targeted suites, then the whole Rust suite (one at a time)**

```bash
cd src-tauri && cargo test --test cache
```

Expected: 14 passed.

```bash
cd src-tauri && cargo test --test ado_testplan --test ai_bridge
```

Expected: all pass, including `run_failures_re_resolves_a_cached_suite_that_was_deleted`.

```bash
cd src-tauri && cargo test --tests
```

Expected: every suite passes, with no `unused import` warnings in the touched files. Afterwards run `git status`. `src/bindings.ts` must be unchanged; if it shows as modified, see Global Constraints.

- [ ] **Step 11: Commit**

```bash
git add -A src-tauri
git commit -F - <<'EOF'
refactor(v2): tags, suites, suite trees and the assigned baseline share one cache

refcache.rs, the resolved-suite map and the AI bridge's suite-tree map
are gone; all four read and write crate::cache. Signing in as a
different account now drops the Rust cache too. A test fails if a
module grows a private cache static again.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

---

### Task 3: The webview cache module

**Files:**
- Create: `src/lib/cache.ts`
- Create: `src/lib/cache.test.ts`
- Delete: `src/lib/localCache.ts`, `src/lib/localCache.test.ts`, `src/lib/persistentQuery.ts`, `src/lib/persistentQuery.test.ts`
- Modify (import paths only): `src/App.tsx:41,44`, `src/components/CommandPalette.tsx:9`, `src/components/CommentsPanel.tsx:9`, `src/components/ContextBar.tsx:10`, `src/components/WorkItemDrawer.tsx:12`, `src/screens/CreateWorkItem.tsx:19`, `src/screens/ManageCases/index.tsx:7`, `src/screens/PrPanel.tsx:34`, `src/screens/RunPanel/index.tsx:16`, `src/screens/Suites.tsx:14`, `src/screens/WorkBoard.tsx:18`

**Interfaces:**
- Produces (used by Task 4):
  - `claimCacheFor(account: string | null): void`
  - `suspendCache(value: boolean): void`
  - `cacheEntry<T>(key: string, maxAgeMs: number): { data: T; at: number } | null`
  - `cacheRead<T>(key: string, maxAgeMs: number): T | null`
  - `cacheWrite<T>(key: string, data: T): void`
  - `cached<T>(key, maxAgeMs, fetcher)`, temporarily; Task 4 removes it.
  - `persistentQuery<T>({ key, fetcher, ttlMs, staleMs, store? })`, where `store?: (data: T) => boolean`
  - `CACHE.{structure, outcomes, reference, finished}`, each `{ ttlMs, staleMs }`
  - `cacheKeys` with these builders:
    - `orgs()`, `projects(org)`, `members(org, project)`
    - `workItemDetail(org, project, id)`, `workItemComments(org, project, id)`
    - `plansSuites(org, project)`, `runHistory(org, project, planId)`, `points(org, project, planId, suiteId)`
    - `boardPrs(org, project)`, `prPipeline(org, project, prId, mergeCommit)`

- [ ] **Step 1: Write the failing tests**

Create `src/lib/cache.test.ts`:

```ts
import { afterEach, expect, test, vi } from "vitest";
import {
  CACHE,
  cacheKeys,
  cacheRead,
  cacheWrite,
  cached,
  claimCacheFor,
  persistentQuery,
  suspendCache,
} from "./cache";

afterEach(() => {
  suspendCache(false);
  localStorage.clear();
});

test("round-trips within the TTL and expires after it", () => {
  cacheWrite("k", { a: 1 });
  expect(cacheRead<{ a: number }>("k", 60_000)).toEqual({ a: 1 });
  // An entry older than the TTL reads as a miss.
  const raw = JSON.parse(localStorage.getItem("tcm-v2-cache:k")!);
  raw.at = Date.now() - 120_000;
  localStorage.setItem("tcm-v2-cache:k", JSON.stringify(raw));
  expect(cacheRead("k", 60_000)).toBeNull();
});

test("cached() fetches once, then serves from storage", async () => {
  let calls = 0;
  const fetcher = async () => {
    calls += 1;
    return ["x"];
  };
  expect(await cached("list", 60_000, fetcher)).toEqual(["x"]);
  expect(await cached("list", 60_000, fetcher)).toEqual(["x"]);
  expect(calls).toBe(1);
});

test("demo mode never reads or writes the cache", () => {
  cacheWrite("real", "data");
  localStorage.setItem("tcm-v2-dev-demo", "on");
  expect(cacheRead("real", 60_000)).toBeNull(); // real data hidden from demo
  cacheWrite("demo-key", "demo-data");
  localStorage.setItem("tcm-v2-dev-demo", "off");
  expect(cacheRead("demo-key", 60_000)).toBeNull(); // demo data never stored
  expect(cacheRead("real", 60_000)).toBe("data"); // real data intact
});

test("unreadable entries are dropped, not served", () => {
  localStorage.setItem("tcm-v2-cache:bad", "{not json");
  expect(cacheRead("bad", 60_000)).toBeNull();
});

test("a suspended cache neither reads nor writes", () => {
  cacheWrite("tour-check", { a: 1 });
  expect(cacheRead("tour-check", 60_000)).toEqual({ a: 1 });

  suspendCache(true);
  expect(cacheRead("tour-check", 60_000)).toBeNull();
  cacheWrite("tour-check", { a: 2 });

  suspendCache(false);
  // The write while suspended was dropped - the earlier value survives.
  expect(cacheRead("tour-check", 60_000)).toEqual({ a: 1 });
});

/** Cache keys carry org and project, which is not the same as carrying the
 * PERSON. Two accounts on one Windows profile used to read each other's
 * plans, suites and outcomes - fetched with a token the second one never
 * held, and painted instantly from the seed before a request could have
 * been refused. */
test("signing in as someone else drops the previous account's cache", () => {
  claimCacheFor("first@example.com");
  cacheWrite("plans-suites:acme/Payments", ["Plan A"]);
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toEqual(["Plan A"]);

  // Same person again, however many times: their cache survives.
  claimCacheFor("first@example.com");
  claimCacheFor("first@example.com");
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toEqual(["Plan A"]);

  // Someone else: gone, even though org and project are identical.
  claimCacheFor("second@example.com");
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toBeNull();

  // Signed out (no account yet) must not wipe what the signed-in user has.
  cacheWrite("plans-suites:acme/Payments", ["Plan B"]);
  claimCacheFor(null);
  expect(cacheRead("plans-suites:acme/Payments", CACHE.structure.ttlMs)).toEqual(["Plan B"]);

  // The address itself is never written to disk.
  expect(JSON.stringify(localStorage)).not.toContain("example.com");
});

/** Only this module's own entries are its to drop. */
test("claiming the cache leaves everything else in storage alone", () => {
  localStorage.setItem("tcm-v2-draft", "the user's queue");
  localStorage.setItem("tcm-v2-theme", "dark");
  claimCacheFor("first@example.com");
  cacheWrite("points:acme/Payments/1/2", [1, 2, 3]);
  claimCacheFor("second@example.com");
  expect(localStorage.getItem("tcm-v2-draft")).toBe("the user's queue");
  expect(localStorage.getItem("tcm-v2-theme")).toBe("dark");
});

test("a fetch writes the result to disk for the next launch", async () => {
  const opts = persistentQuery({
    key: "k",
    fetcher: async () => ["a", "b"],
    ...CACHE.structure,
  });
  expect(opts.initialData()).toBeUndefined(); // cold: nothing seeded
  await opts.queryFn();
  expect(cacheRead<string[]>("k", CACHE.structure.ttlMs)).toEqual(["a", "b"]);
});

test("a stored entry seeds initialData WITH its real age", () => {
  cacheWrite("k", ["seeded"]);
  const opts = persistentQuery({
    key: "k",
    fetcher: async () => ["fresh"],
    ...CACHE.structure,
  });
  expect(opts.initialData()).toEqual(["seeded"]);
  // The age is the write time, not "now" - otherwise React Query would
  // treat a week-old seed as freshly fetched and never revalidate.
  const at = opts.initialDataUpdatedAt();
  expect(at).toBeTypeOf("number");
  expect(Math.abs(Date.now() - at!)).toBeLessThan(5_000);
});

test("an entry older than the TTL is not served at all", () => {
  cacheWrite("k", ["ancient"]);
  const raw = JSON.parse(localStorage.getItem("tcm-v2-cache:k")!);
  raw.at = Date.now() - (CACHE.structure.ttlMs + 60_000);
  localStorage.setItem("tcm-v2-cache:k", JSON.stringify(raw));

  const opts = persistentQuery({ key: "k", fetcher: async () => [], ...CACHE.structure });
  expect(opts.initialData()).toBeUndefined();
});

/** Work-item details carry inline images as data: URIs; one multi-megabyte
 * write would trip the quota handler, which clears the whole cache. */
test("a store predicate keeps oversized results off the disk but still returns them", async () => {
  const big = "x".repeat(50);
  const opts = persistentQuery({
    key: "k",
    fetcher: async () => big,
    ...CACHE.outcomes,
    store: (d) => d.length <= 10,
  });
  expect(await opts.queryFn()).toBe(big);
  expect(localStorage.getItem("tcm-v2-cache:k")).toBeNull();

  const small = persistentQuery({
    key: "k2",
    fetcher: async () => "tiny",
    ...CACHE.outcomes,
    store: (d) => d.length <= 10,
  });
  await small.queryFn();
  expect(cacheRead("k2", CACHE.outcomes.ttlMs)).toBe("tiny");
});

test("shelf lives say what the data does", () => {
  // Structure should not refetch on every visit; run results should.
  expect(CACHE.structure.staleMs).toBeGreaterThan(60 * 60_000);
  expect(CACHE.outcomes.staleMs).toBe(0);
  // Org/project lists and members: a day from disk with no request at all.
  expect(CACHE.reference).toEqual({ ttlMs: 24 * 60 * 60_000, staleMs: 24 * 60 * 60_000 });
  // Pipelines of a finished PR never change: a month, never stale.
  expect(CACHE.finished).toEqual({ ttlMs: 30 * 24 * 60 * 60_000, staleMs: Infinity });
});

/** These strings are what earlier versions wrote. Changing one silently
 * throws away every user's cache for that data on upgrade. */
test("cache keys are the strings earlier versions stored", () => {
  expect(cacheKeys.orgs()).toBe("orgs");
  expect(cacheKeys.projects("acme")).toBe("projects:acme");
  expect(cacheKeys.members("acme", "Web")).toBe("members:acme/Web");
  expect(cacheKeys.workItemDetail("acme", "Web", 2003)).toBe("wi-detail:acme/Web/2003");
  expect(cacheKeys.workItemComments("acme", "Web", 2003)).toBe("wi-comments:acme/Web/2003");
  expect(cacheKeys.plansSuites("acme", "Web")).toBe("plans-suites:acme/Web");
  expect(cacheKeys.runHistory("acme", "Web", 7)).toBe("run-history:acme/Web/7");
  expect(cacheKeys.points("acme", "Web", 7, 71)).toBe("points:acme/Web/7/71");
  expect(cacheKeys.boardPrs("acme", "Web")).toBe("board-prs:acme/Web");
  expect(cacheKeys.prPipeline("acme", "Web", 42, "abc")).toBe("pipe:acme/Web:42:abc");
});

test("demo mode neither seeds nor stores", async () => {
  localStorage.setItem("tcm-v2-dev-demo", "on");
  const fetcher = vi.fn(async () => ["real"]);
  const opts = persistentQuery({ key: "k", fetcher, ...CACHE.structure });
  await opts.queryFn();
  expect(opts.initialData()).toBeUndefined();
  expect(localStorage.getItem("tcm-v2-cache:k")).toBeNull();
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

```bash
npx vitest run src/lib/cache.test.ts
```

Expected: FAIL, `Failed to resolve import "./cache"`.

- [ ] **Step 3: Write `cache.ts`**

Create `src/lib/cache.ts`:

```ts
/**
 * The webview's one cache: persistent storage over localStorage, plus the
 * React Query options that seed a query from it.
 *
 * Azure DevOps rate-limits per user, so every read the app can answer from
 * disk is budget handed back. React Query's own cache is memory-only - every
 * launch (and every dev reload) starts empty - so this holds the slow-moving
 * reads (org/project lists, team members, plan trees) and the immutable ones
 * (pipeline runs of finished PRs) across restarts.
 *
 * How to use it:
 * - A query whose data should survive a restart: spread
 *   `persistentQuery({ key: cacheKeys.x(...), fetcher, ...CACHE.preset })`
 *   into `useQuery`. That is the whole integration.
 * - Merge logic a plain query can't express (PrPanel's finished pipelines):
 *   `cacheRead` / `cacheWrite`, still with a `cacheKeys` key and a `CACHE`
 *   shelf life.
 * - A new key goes in `cacheKeys`, a new shelf life in `CACHE`. cache.test.ts
 *   fails on a hand-written key, a raw TTL, or a second implementation.
 *
 * The Rust backend has its own cache (src-tauri/src/cache) for data the AI
 * bridge reads too; the two are separate processes and share nothing.
 *
 * Disabled entirely in demo mode: demo data must never be served to a real
 * session, and vice versa. Bounded to MAX_ENTRIES; oldest entries fall out
 * first (localStorage is ~5 MB and shared with everything else the app
 * persists).
 */

const PREFIX = "tcm-v2-cache:";
const OWNER_KEY = "tcm-v2-cache-owner";
const MAX_ENTRIES = 150;

const HOUR = 60 * 60_000;
const DAY = 24 * HOUR;

type Entry<T> = { at: number; data: T };

/** Common shelf lives, named so call sites read as intent. Pick `staleMs`
 * by how much the data actually moves. */
export const CACHE = {
  /** Plans/suites structure: refetch at most every 6h, keep for a week. */
  structure: { ttlMs: 7 * DAY, staleMs: 6 * HOUR },
  /** Run outcomes, comments, item details: seed instantly, always revalidate. */
  outcomes: { ttlMs: 7 * DAY, staleMs: 0 },
  /** Org/project lists, team members: a day from disk with no request, so
   * most app starts cost nothing here. */
  reference: { ttlMs: DAY, staleMs: DAY },
  /** Pipelines of a finished PR: they never change again. */
  finished: { ttlMs: 30 * DAY, staleMs: Infinity },
} as const;

/** Every key the webview caches under. The strings are what earlier
 * versions stored - changing one throws that data away for every user. */
export const cacheKeys = {
  orgs: () => "orgs",
  projects: (org: string) => `projects:${org}`,
  members: (org: string, project: string) => `members:${org}/${project}`,
  workItemDetail: (org: string, project: string, id: number) => `wi-detail:${org}/${project}/${id}`,
  workItemComments: (org: string, project: string, id: number) =>
    `wi-comments:${org}/${project}/${id}`,
  plansSuites: (org: string, project: string) => `plans-suites:${org}/${project}`,
  runHistory: (org: string, project: string, planId: number | undefined) =>
    `run-history:${org}/${project}/${planId}`,
  points: (org: string, project: string, planId: number | undefined, suiteId: number | undefined) =>
    `points:${org}/${project}/${planId}/${suiteId}`,
  boardPrs: (org: string, project: string) => `board-prs:${org}/${project}`,
  prPipeline: (org: string, project: string, prId: number, mergeCommit: string) =>
    `pipe:${org}/${project}:${prId}:${mergeCommit}`,
};

/** Non-reversible tag for an account, so the identity check never needs the
 * address itself written to disk. Collisions only cost a needless wipe. */
function tag(account: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < account.length; i++) {
    h = ((h ^ account.charCodeAt(i)) * 0x01000193) >>> 0;
  }
  return h.toString(36);
}

/**
 * Hand the cache to the signed-in account, discarding anything the previous
 * one left behind.
 *
 * Every key here is scoped by org and project, which is not the same as
 * being scoped by PERSON. Two accounts on one Windows profile - someone
 * signing out and back in as a service or test account - saw each other's
 * plan and suite trees for up to the structure TTL, painted instantly from
 * `initialData` before any request could have been refused. The data was
 * fetched with a token the second account never held.
 */
export function claimCacheFor(account: string | null): void {
  if (!account) return;
  try {
    const now = tag(account);
    if (localStorage.getItem(OWNER_KEY) === now) return;
    for (const k of Object.keys(localStorage)) {
      if (k.startsWith(PREFIX)) localStorage.removeItem(k);
    }
    localStorage.setItem(OWNER_KEY, now);
  } catch {
    // storage unavailable - nothing was cached to leak
  }
}

function demoMode(): boolean {
  try {
    return localStorage.getItem("tcm-v2-dev-demo") === "on";
  } catch {
    return false;
  }
}

/** The guided tour shows sample data: while it runs nothing may be read
 * from disk (real data would appear inside the tour) and nothing written
 * to it (sample data would outlive the tour). */
let suspended = false;

export function suspendCache(value: boolean): void {
  suspended = value;
}

function off(): boolean {
  return suspended || demoMode();
}

/** The entry WITH its age, for callers that need to tell React Query how
 * old the seed is (so it can decide whether to revalidate). */
export function cacheEntry<T>(key: string, maxAgeMs: number): { data: T; at: number } | null {
  if (off()) return null;
  try {
    const raw = localStorage.getItem(PREFIX + key);
    if (!raw) return null;
    const entry = JSON.parse(raw) as Entry<T>;
    if (typeof entry?.at !== "number") return null;
    if (Date.now() - entry.at > maxAgeMs) return null;
    return entry;
  } catch {
    return null;
  }
}

export function cacheRead<T>(key: string, maxAgeMs: number): T | null {
  return cacheEntry<T>(key, maxAgeMs)?.data ?? null;
}

export function cacheWrite<T>(key: string, data: T): void {
  if (off()) return;
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify({ at: Date.now(), data }));
    prune();
  } catch {
    // Quota or unavailable - drop everything we own and carry on; a cache
    // that cannot write must never break the feature it accelerates.
    try {
      for (const k of Object.keys(localStorage)) {
        if (k.startsWith(PREFIX)) localStorage.removeItem(k);
      }
    } catch {
      // storage fully unavailable
    }
  }
}

/** Serve from cache when fresh enough, else fetch and remember. */
export async function cached<T>(
  key: string,
  maxAgeMs: number,
  fetcher: () => Promise<T>,
): Promise<T> {
  const hit = cacheRead<T>(key, maxAgeMs);
  if (hit !== null) return hit;
  const data = await fetcher();
  cacheWrite(key, data);
  return data;
}

/**
 * React Query options backed by the cache, so a query survives an app
 * restart instead of re-hitting Azure DevOps.
 *
 * Seeding `initialData` from disk - WITH its real age via
 * `initialDataUpdatedAt` - lets React Query paint instantly and then decide
 * for itself whether the seed is stale enough to revalidate in the
 * background.
 */
export function persistentQuery<T>(opts: {
  /** From `cacheKeys` - must encode every scope the data depends on. */
  key: string;
  fetcher: () => Promise<T>;
  /** How long a seed may be served at all. */
  ttlMs: number;
  /** How long a seed is considered fresh (no background refetch). */
  staleMs: number;
  /** Whether a fetched result may be written; it is returned either way. */
  store?: (data: T) => boolean;
}) {
  const { key, fetcher, ttlMs, staleMs, store } = opts;
  return {
    queryFn: async () => {
      const data = await fetcher();
      if (!store || store(data)) cacheWrite(key, data);
      return data;
    },
    initialData: () => cacheEntry<T>(key, ttlMs)?.data,
    // Without the real timestamp React Query would treat the seed as
    // fetched "now" and never refresh it.
    initialDataUpdatedAt: () => cacheEntry<T>(key, ttlMs)?.at,
    staleTime: staleMs,
  };
}

function prune(): void {
  const keys: { k: string; at: number }[] = [];
  for (const k of Object.keys(localStorage)) {
    if (!k.startsWith(PREFIX)) continue;
    try {
      keys.push({ k, at: (JSON.parse(localStorage.getItem(k) ?? "") as Entry<unknown>).at ?? 0 });
    } catch {
      localStorage.removeItem(k); // unreadable entry - not a cache anymore
    }
  }
  if (keys.length <= MAX_ENTRIES) return;
  keys.sort((a, b) => a.at - b.at);
  for (const { k } of keys.slice(0, keys.length - MAX_ENTRIES)) {
    localStorage.removeItem(k);
  }
}
```

- [ ] **Step 4: Run the new tests and confirm they pass**

```bash
npx vitest run src/lib/cache.test.ts
```

Expected: 14 passed.

- [ ] **Step 5: Delete the old modules and repoint every import**

```bash
git rm src/lib/localCache.ts src/lib/localCache.test.ts src/lib/persistentQuery.ts src/lib/persistentQuery.test.ts
```

In each file below, change only the module path of the import (the imported names stay the same):

| File | From | To |
|---|---|---|
| `src/components/CommandPalette.tsx` | `"../lib/localCache"` | `"../lib/cache"` |
| `src/components/CommentsPanel.tsx` | `"../lib/persistentQuery"` | `"../lib/cache"` |
| `src/components/ContextBar.tsx` | `"../lib/localCache"` | `"../lib/cache"` |
| `src/components/WorkItemDrawer.tsx` | `"../lib/localCache"` | `"../lib/cache"` |
| `src/screens/CreateWorkItem.tsx` | `"../lib/localCache"` | `"../lib/cache"` |
| `src/screens/ManageCases/index.tsx` | `"../../lib/persistentQuery"` | `"../../lib/cache"` |
| `src/screens/PrPanel.tsx` | `"../lib/localCache"` | `"../lib/cache"` |
| `src/screens/RunPanel/index.tsx` | `"../../lib/persistentQuery"` | `"../../lib/cache"` |
| `src/screens/Suites.tsx` | `"../lib/persistentQuery"` | `"../lib/cache"` |
| `src/screens/WorkBoard.tsx` | `"../lib/persistentQuery"` | `"../lib/cache"` |

In `src/App.tsx`, delete line 44 (`import { CACHE, persistentQuery } from "./lib/persistentQuery";`). Replace line 41 with:

```ts
import { CACHE, cacheEntry, claimCacheFor, persistentQuery, suspendCache } from "./lib/cache";
```

- [ ] **Step 6: Typecheck and run the full frontend suite**

```bash
npx tsc --noEmit
```

Expected: no errors. (Anything still importing `localCache` or `persistentQuery` fails here by name.)

```bash
npx vitest run
```

Expected: all files pass. `PrPanel.test.tsx` and `WorkItemDrawer.test.tsx` read `tcm-v2-cache:` keys directly, and they still pass because nothing about storage changed. If `App.test.tsx` fails once and passes on a re-run, that is its documented load flake; two different failures are not.

- [ ] **Step 7: Commit**

```bash
git add -A src
git commit -F - <<'EOF'
refactor(v2): the webview cache is one module, lib/cache.ts

localCache and persistentQuery merge into one file that also names
every cache key (unchanged strings, so nothing already cached is lost)
and more shelf-life presets. persistentQuery gains a store predicate.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

---

### Task 4: Every webview call site through the one API, plus the guard

**Files:**
- Modify: `src/lib/cache.test.ts` (add the guard; drop the `cached()` test)
- Modify: `src/lib/cache.ts` (remove `cached`)
- Modify: `src/components/ContextBar.tsx:61-75`, `src/components/CommandPalette.tsx:40-48`, `src/screens/CreateWorkItem.tsx:80-90`, `src/components/WorkItemDrawer.tsx:101-130`, `src/screens/PrPanel.tsx:428-457`, `src/screens/WorkBoard.tsx:295-300`, `src/App.tsx:718,798-801`, `src/components/CommentsPanel.tsx:106`, `src/screens/ManageCases/index.tsx:29`, `src/screens/RunPanel/index.tsx:150,169`, `src/screens/Suites.tsx:59,140`

**Interfaces:**
- Consumes: `persistentQuery`, `CACHE`, `cacheKeys`, `cacheEntry`, `cacheRead` and `cacheWrite` from Task 3.
- Produces: `cached` is removed from `src/lib/cache.ts`. Nothing else consumes this task.

- [ ] **Step 1: Write the failing guard test**

In `src/lib/cache.test.ts`, change the imports:

```ts
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test, vi } from "vitest";
import {
  CACHE,
  cacheKeys,
  cacheRead,
  cacheWrite,
  claimCacheFor,
  persistentQuery,
  suspendCache,
} from "./cache";
```

Delete the whole `test("cached() fetches once, then serves from storage", ...)` block. Append:

```ts
/**
 * One cache means one. A screen that needs cached data uses lib/cache.ts;
 * it does not reach into localStorage, seed a query by hand, invent a key
 * string, or pick a shelf life of its own. Fix a failure by using
 * persistentQuery / cacheKeys / CACHE - add the key or preset there if it
 * is new.
 */
describe("one cache", () => {
  // import.meta.url, not __dirname: this file is ESM under vitest.
  const SRC = dirname(dirname(fileURLToPath(import.meta.url)));
  const files: { file: string; text: string }[] = [];
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        walk(p);
        continue;
      }
      if (!/\.(tsx|ts)$/.test(e.name) || /\.test\.(tsx|ts)$/.test(e.name)) continue;
      if (e.name === "bindings.ts") continue; // generated
      const file = relative(SRC, p).replace(/\\/g, "/");
      if (file === "lib/cache.ts") continue;
      files.push({ file, text: readFileSync(p, "utf8") });
    }
  };
  walk(SRC);

  const offenders = (pattern: RegExp) => files.filter((f) => pattern.test(f.text)).map((f) => f.file);

  test("only lib/cache.ts touches the cache's storage or seeds a query from disk", () => {
    expect(offenders(/tcm-v2-cache|initialDataUpdatedAt/)).toEqual([]);
  });

  test("cache keys come from cacheKeys, never a hand-written string", () => {
    // A literal passed straight in, or any string opening with one of the
    // prefixes cacheKeys owns (React Query keys are arrays - no colon).
    expect(
      offenders(
        /cache(?:Read|Write|Entry)(?:<[^>]*>)?\(\s*[`"']|persistentQuery\(\{\s*key:\s*[`"']|[`"'](?:projects|members|wi-detail|wi-comments|plans-suites|run-history|points|board-prs|pipe):/,
      ),
    ).toEqual([]);
  });

  test("shelf lives come from CACHE, never a number at the call site", () => {
    expect(offenders(/ttlMs:\s*\d/)).toEqual([]);
  });
});
```

- [ ] **Step 2: Run the guard and confirm it fails**

```bash
npx vitest run src/lib/cache.test.ts
```

Expected: FAIL, with these offenders:
- **Storage test:** `components/WorkItemDrawer.tsx`
- **Keys test:** all 11 call-site files: `App.tsx`, `components/CommandPalette.tsx`, `components/CommentsPanel.tsx`, `components/ContextBar.tsx`, `components/WorkItemDrawer.tsx`, `screens/CreateWorkItem.tsx`, `screens/ManageCases/index.tsx`, `screens/PrPanel.tsx`, `screens/RunPanel/index.tsx`, `screens/Suites.tsx`, `screens/WorkBoard.tsx`
- **TTL test:** `screens/WorkBoard.tsx`

Any *other* file listed here is a cache the survey missed. Migrate it the same way before continuing.

- [ ] **Step 3: Orgs, projects and members onto `persistentQuery`**

`src/components/ContextBar.tsx`: change the import to `import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";`. Then replace the `orgs` and `projects` queries:

```tsx
  // Org/project lists barely change - served from the local cache for a
  // day, so most app starts cost zero ADO requests here.
  const orgs = useQuery({
    queryKey: ["orgs"],
    ...persistentQuery({
      key: cacheKeys.orgs(),
      fetcher: () => unwrap(commands.listOrgs()),
      ...CACHE.reference,
    }),
  });

  const projects = useQuery({
    queryKey: ["projects", org],
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: Boolean(org),
  });
```

`src/components/CommandPalette.tsx`: change the import to `import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";`. Then replace the `projects` query:

```tsx
  // Same key + cache as ContextBar, so the palette never refetches what
  // the bar already has.
  const projects = useQuery({
    queryKey: ["projects", org],
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: open && Boolean(org),
  });
```

`src/screens/CreateWorkItem.tsx`: change the import to `import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";`. Then replace the `members` query:

```tsx
  const members = useQuery({
    // Same key + cache as the drawer: one members fetch serves both.
    queryKey: ["members", org, project],
    ...persistentQuery({
      key: cacheKeys.members(org, project),
      fetcher: () => unwrap(commands.listTeamMembers(org, project)),
      ...CACHE.reference,
    }),
    enabled: Boolean(org && project),
    retry: false,
  });
```

- [ ] **Step 4: The work-item drawer (size-capped detail, members)**

`src/components/WorkItemDrawer.tsx`: change the import to `import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";`. Then replace everything from the comment `// Seeded from disk so a reopened item paints instantly` through the end of the `members` query with:

```tsx
  // Seeded from disk so a reopened item paints instantly, then
  // revalidates. The write is SIZE-CAPPED: details carry inline images as
  // data: URIs, and one multi-megabyte write would trip localStorage's
  // quota handler, which clears the whole cache to recover - a bad trade
  // for one Bug's screenshots. Oversized items just skip the seed and
  // load as before.
  const detail = useQuery({
    queryKey: ["wi-detail", org, project, itemId],
    ...persistentQuery({
      key: cacheKeys.workItemDetail(org, project, itemId),
      fetcher: () => unwrap(commands.workItemDetail(org, project, itemId)),
      ...CACHE.outcomes,
      store: (d) => JSON.stringify(d).length <= 400_000,
    }),
    retry: false,
  });

  const members = useQuery({
    queryKey: ["members", org, project],
    // v1 cached members for 24h; the local cache carries that across
    // restarts too (big orgs, slow endpoint).
    ...persistentQuery({
      key: cacheKeys.members(org, project),
      fetcher: () => unwrap(commands.listTeamMembers(org, project)),
      ...CACHE.reference,
    }),
  });
```

Keep the `WorkItemDetail` type import; `toDraft` still uses it.

- [ ] **Step 5: PR pipelines, the board, and the remaining keys**

`src/screens/PrPanel.tsx`: change the import to `import { CACHE, cacheKeys, cacheRead, cacheWrite } from "../lib/cache";`. In the `pipeline` query's `queryFn`, replace the two lines:

```tsx
      const key = `pipe:${org}/${project}:${pr.id}:${pr.merge_commit}`;
      if (finalized) {
        const hit = cacheRead<PrBuild[]>(key, 30 * 24 * 60 * 60_000);
```

with:

```tsx
      const key = cacheKeys.prPipeline(org, project, pr.id, pr.merge_commit);
      if (finalized) {
        const hit = cacheRead<PrBuild[]>(key, CACHE.finished.ttlMs);
```

`src/screens/WorkBoard.tsx`: change the import to `import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";`. Then replace:

```tsx
      key: `board-prs:${org}/${project}`,
      fetcher: () => unwrap(commands.boardPrLinks(org, project)),
      ttlMs: 7 * 24 * 60 * 60_000,
      staleMs: 5 * 60_000,
```

with:

```tsx
      key: cacheKeys.boardPrs(org, project),
      fetcher: () => unwrap(commands.boardPrLinks(org, project)),
      // A week on disk like any structure, but PR links move within minutes.
      ...CACHE.structure,
      staleMs: 5 * 60_000,
```

Replace each hand-written key in the remaining files (add `cacheKeys` to each file's existing `../lib/cache` import):

| File | Replace | With |
|---|---|---|
| `src/App.tsx:718` | `` const key = `plans-suites:${org}/${project}`; `` | `const key = cacheKeys.plansSuites(org, project);` |
| `src/App.tsx:798-801` | `` cacheEntry<PlanWithSuites[]>(`plans-suites:${org}/${project}`, CACHE.structure.ttlMs,) `` (spread over lines) | `cacheEntry<PlanWithSuites[]>(cacheKeys.plansSuites(org, project), CACHE.structure.ttlMs)` |
| `src/components/CommentsPanel.tsx:106` | `` key: `wi-comments:${org}/${project}/${itemId}`, `` | `key: cacheKeys.workItemComments(org, project, itemId),` |
| `src/screens/ManageCases/index.tsx:29` | `` key: `plans-suites:${org}/${project}`, `` | `key: cacheKeys.plansSuites(org, project),` |
| `src/screens/RunPanel/index.tsx:150` | `` key: `run-history:${org}/${project}/${suite.data?.plan_id}`, `` | `key: cacheKeys.runHistory(org, project, suite.data?.plan_id),` |
| `src/screens/RunPanel/index.tsx:169` | `` key: `points:${org}/${project}/${suite.data?.plan_id}/${suite.data?.suite_id}`, `` | `key: cacheKeys.points(org, project, suite.data?.plan_id, suite.data?.suite_id),` |
| `src/screens/Suites.tsx:59` | `` key: `points:${org}/${project}/${planId}/${suite.id}`, `` | `key: cacheKeys.points(org, project, planId, suite.id),` |
| `src/screens/Suites.tsx:140` | `` key: `plans-suites:${org}/${project}`, `` | `key: cacheKeys.plansSuites(org, project),` |

The `App.tsx` import becomes:

```ts
import { CACHE, cacheEntry, cacheKeys, claimCacheFor, persistentQuery, suspendCache } from "./lib/cache";
```

- [ ] **Step 6: Remove `cached`**

In `src/lib/cache.ts`, delete the whole `/** Serve from cache when fresh enough, else fetch and remember. */ export async function cached<T>(...) { ... }` block.

- [ ] **Step 7: Run the guard, typecheck, and the full suite (one at a time)**

```bash
npx vitest run src/lib/cache.test.ts
```

Expected: 16 passed. The `cached()` test is gone and the three guard tests pass.

```bash
npx tsc --noEmit
```

Expected: no errors.

```bash
npx vitest run
```

Expected: every file passes. If a ContextBar, CommandPalette, CreateWorkItem or WorkItemDrawer test now fails, read it before touching it:
- A test that counted `listOrgs`, `listProjects` or `listTeamMembers` calls may now see fewer, because a fresh seed no longer calls the fetcher at all. Fewer requests is the intended behaviour, so update that expectation to match.
- Any other failure is a real regression. Fix the code, not the test.

- [ ] **Step 8: Commit**

```bash
git add -A src
git commit -F - <<'EOF'
refactor(v2): every cached webview read goes through lib/cache.ts

Orgs, projects and members use persistentQuery (and now paint from
disk instantly), the work-item drawer uses the store predicate for its
size cap, and every key and shelf life comes from cacheKeys and CACHE.
A test fails on a hand-written key, a raw TTL, or direct storage use.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
git log -1 --stat
```

---

### Task 5: Document the convention and run the release gates

**Files:**
- Modify: `CLAUDE.md` (the "Important conventions" section)

**Interfaces:**
- Consumes: the finished state of Tasks 1-4. Produces nothing for other tasks.

- [ ] **Step 1: Add the convention**

In `CLAUDE.md`, under `## Important conventions`, add this bullet after the **Theming** bullet:

```markdown
- **Caching has exactly one implementation per side** - never add another.
  Webview: `src/lib/cache.ts`. Spread `persistentQuery({ key:
  cacheKeys.x(...), fetcher, ...CACHE.preset })` into `useQuery`; reach for
  `cacheRead`/`cacheWrite` only for merge logic a query can't express. New
  keys go in `cacheKeys` (existing strings are what users already have on
  disk - never change one), new shelf lives in `CACHE`. Rust:
  `src-tauri/src/cache/` - `cache::get/fresh/put/update/forget` for data
  that survives a restart, `session_fresh/session_put` for memory-only
  values; every key and TTL in `cache/keys.rs`. Both wipe themselves when a
  different account signs in. `src/lib/cache.test.ts` and
  `tests/cache.rs` fail on a private cache.
```

- [ ] **Step 2: Run the release gates exactly as the release script does (one at a time)**

```bash
cd src-tauri && cargo test --tests
```

Expected: every suite passes.

```bash
npx vitest run
```

Expected: every file passes.

```bash
npm run build
```

Expected: tsc and vite both succeed.

Then run `git status`. Only `CLAUDE.md` should be modified. If `src/bindings.ts` or `src-tauri/Cargo.toml` show as modified with identical content, see Global Constraints.

- [ ] **Step 3: Commit and hand back**

```bash
git add CLAUDE.md
git commit -F - <<'EOF'
docs(v2): where caching lives and how to use it

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
git log --oneline main..HEAD
```

Expected: five commits on `refactor/unified-cache`. Stop here and report to the user. Merging into `main`, pushing, and any release are the user's call.
