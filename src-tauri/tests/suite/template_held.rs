//! The held signed-in browser store (`api_templates::held`): one per
//! (environment, account), handed back only to a run with the same sign-in
//! and only while nothing else has taken the account since; and the lease
//! generation (`autorun::lease::generation`) that tells it so.
//!
//! The store and the lease table are process-wide, so every test here takes
//! `serial::held_browsers` and then `serial::account_leases`, in that order.

use crate::common;
use std::time::{Duration, Instant};
use v2_lib::api_templates::held::{self, HeldEntry, Taken, HELD_IDLE};
use v2_lib::api_templates::runner::Session;
use v2_lib::autorun::lease::{self, Holder};

/// Stands in for a browser: the store never drives one, only keeps it.
#[derive(Debug, PartialEq)]
struct Tag(u32);

const ENV: &str = "env-held";

fn session() -> Session {
    Session::new(common::recipe(), common::account(), "https://hr.example.internal".into(), Vec::new())
}

fn fingerprint() -> u64 {
    held::fingerprint(&common::recipe(), &common::account())
}

fn entry(tag: u32, env: &str, key: &str) -> HeldEntry<Tag> {
    HeldEntry {
        driver: Tag(tag),
        session: session(),
        fingerprint: fingerprint(),
        generation: lease::generation(env, key),
        page: Some("/Home/Index".into()),
    }
}

fn reused(t: Taken<Tag>) -> Option<HeldEntry<Tag>> {
    match t {
        Taken::Reuse(e) => Some(e),
        _ => None,
    }
}

#[test]
fn take_after_put_returns_the_entry_once() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.once";
    assert!(held::put(ENV, key, entry(1, ENV, key)).is_none());

    let got = reused(held::take::<Tag>(ENV, key, fingerprint())).expect("the held browser was not handed back");
    assert_eq!(got.driver, Tag(1));
    assert_eq!(got.page.as_deref(), Some("/Home/Index"));
    assert!(matches!(held::take::<Tag>(ENV, key, fingerprint()), Taken::Nothing), "handed back twice");
}

#[test]
fn a_case_taking_the_account_makes_the_held_browser_give_way() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.given-way";
    held::put(ENV, key, entry(2, ENV, key));
    let before = lease::generation(ENV, key);
    drop(lease::try_acquire(ENV, key, Holder::Case { run: "run-1".into() }).unwrap());
    assert_eq!(lease::generation(ENV, key), before + 1);

    match held::take::<Tag>(ENV, key, fingerprint()) {
        Taken::Close(e) => assert_eq!(e.driver, Tag(2)),
        _ => panic!("a browser that gave way was not handed back for closing"),
    }
    assert!(matches!(held::take::<Tag>(ENV, key, fingerprint()), Taken::Nothing), "a stale entry stayed");
}

#[test]
fn the_supervised_browser_and_setup_also_bump_the_generation() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.others";
    assert_eq!(lease::generation(ENV, key), 0, "a key never taken starts at 0");
    drop(lease::try_acquire(ENV, key, Holder::Browser).unwrap());
    drop(lease::try_acquire(ENV, key, Holder::Setup).unwrap());
    assert_eq!(lease::generation(ENV, key), 2);
    assert_eq!(lease::generation("another-env", key), 0, "a generation is per environment");
}

#[test]
fn a_template_lease_does_not_bump_the_generation() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.template";
    held::put(ENV, key, entry(3, ENV, key));
    let before = lease::generation(ENV, key);
    drop(lease::try_acquire(ENV, key, Holder::Template).unwrap());
    assert_eq!(lease::generation(ENV, key), before);

    let got = reused(held::take::<Tag>(ENV, key, fingerprint())).expect("a template lease made it give way");
    assert_eq!(got.driver, Tag(3));
}

#[test]
fn a_refused_take_does_not_bump_the_generation() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.refused";
    let _template = lease::try_acquire(ENV, key, Holder::Template).unwrap();
    let before = lease::generation(ENV, key);
    assert!(lease::try_acquire(ENV, key, Holder::Browser).is_err());
    assert_eq!(lease::generation(ENV, key), before, "a lease that was never taken took the account");
}

#[test]
fn a_changed_recipe_is_not_reused() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.changed";
    held::put(ENV, key, entry(4, ENV, key));

    let mut recipe = common::recipe();
    recipe.start_url = "https://hr.example.internal/other".into();
    let changed = held::fingerprint(&recipe, &common::account());
    assert_ne!(changed, fingerprint());
    match held::take::<Tag>(ENV, key, changed) {
        Taken::Close(e) => assert_eq!(e.driver, Tag(4)),
        _ => panic!("a browser signed in with another recipe was not handed back for closing"),
    }
    assert!(matches!(held::take::<Tag>(ENV, key, fingerprint()), Taken::Nothing));
}

#[test]
fn a_changed_login_changes_the_fingerprint() {
    let mut account = common::account();
    account.username = "someone-else".into();
    assert_ne!(held::fingerprint(&common::recipe(), &account), fingerprint());
    let mut account = common::account();
    account.key = "another.key".into();
    assert_ne!(held::fingerprint(&common::recipe(), &account), fingerprint());
    assert_eq!(fingerprint(), fingerprint(), "the same sign-in gave two fingerprints");
}

#[test]
fn expired_drains_only_entries_past_their_deadline() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let now = Instant::now();
    let (old, young) = ("held.old", "held.young");
    held::put_at(ENV, old, entry(5, ENV, old), now);
    held::put_at(ENV, young, entry(6, ENV, young), now + Duration::from_secs(60));

    assert!(held::expired_at::<Tag>(now + HELD_IDLE - Duration::from_secs(1)).is_empty(), "drained too early");
    let gone = held::expired_at::<Tag>(now + HELD_IDLE + Duration::from_secs(1));
    assert_eq!(gone.into_iter().map(|e| e.driver).collect::<Vec<_>>(), vec![Tag(5)]);

    let got = reused(held::take::<Tag>(ENV, young, fingerprint())).expect("the younger entry was drained");
    assert_eq!(got.driver, Tag(6));
}

#[test]
fn put_replacing_an_entry_hands_the_old_one_back() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    let key = "held.replace";
    assert!(held::put(ENV, key, entry(7, ENV, key)).is_none());
    let old = held::put(ENV, key, entry(8, ENV, key)).expect("the replaced entry was not handed back");
    assert_eq!(old.driver, Tag(7));
    let got = reused(held::take::<Tag>(ENV, key, fingerprint())).unwrap();
    assert_eq!(got.driver, Tag(8));
}

#[test]
fn drain_all_hands_back_every_entry() {
    let _h = crate::serial::held_browsers();
    let _l = crate::serial::account_leases();
    held::drain_all::<Tag>();
    held::put(ENV, "held.all-1", entry(9, ENV, "held.all-1"));
    held::put("env-other", "held.all-2", entry(10, "env-other", "held.all-2"));
    let mut tags: Vec<u32> = held::drain_all::<Tag>().into_iter().map(|e| e.driver.0).collect();
    tags.sort();
    assert_eq!(tags, vec![9, 10]);
    assert!(held::drain_all::<Tag>().is_empty());
}
