//! Finding the environment's database server, knocking on it, and the
//! unattended run's wait for it to answer again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use v2_lib::autorun::network::{
    answers, db_target, target_of, wait_until, waits, WaitEnd, CONNECT_TIMEOUT,
};
use v2_lib::db::credentials::{add_custom, WithList};
use v2_lib::db::{DbCredentialsForm, MemoryStore};

fn t(s: &str, p: Option<u16>) -> Option<(String, u16)> {
    target_of(s, p)
}

#[test]
fn target_of_finds_host_and_port() {
    assert_eq!(t("db1", None), Some(("db1".into(), 1433)));
    assert_eq!(t("db1", Some(1444)), Some(("db1".into(), 1444)));
    assert_eq!(t("db1,1500", None), Some(("db1".into(), 1500)));
    assert_eq!(t("tcp:db1,1500", None), Some(("db1".into(), 1500)));
    assert_eq!(t("tcp:db1", None), Some(("db1".into(), 1433)));
    assert_eq!(t("db1\\SQL2019", None), None);
    assert_eq!(t("db1\\SQL2019", Some(1600)), Some(("db1".into(), 1600)));
    assert_eq!(t("", None), None);
    assert_eq!(t("  ", Some(1)), None);
    assert_eq!(t("  db1  ", None), Some(("db1".into(), 1433)));
    assert_eq!(t("  tcp:db1,1500 ", None), Some(("db1".into(), 1500)));
}

#[test]
fn db_target_resolves_a_saved_database_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") };
    let form = DbCredentialsForm {
        server: "own-host".into(),
        port: Some(1555),
        database: "own-db".into(),
        user: "me".into(),
        password: Some("pw".into()),
        trust_cert: false,
    };
    let id = add_custom(&store, "Staging", &form).unwrap().id;
    assert_eq!(db_target(&store, &id), Some(("own-host".into(), 1555)));
    assert_eq!(db_target(&store, "no-such-db"), None);
    assert_eq!(db_target(&store, ""), None);
}

#[tokio::test]
async fn answers_is_true_for_a_listener_and_false_once_it_is_gone() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(answers("127.0.0.1", port, CONNECT_TIMEOUT).await);
    drop(l);
    let started = Instant::now();
    assert!(!answers("127.0.0.1", port, CONNECT_TIMEOUT).await);
    assert!(started.elapsed() < CONNECT_TIMEOUT + Duration::from_secs(1));
}

const MS: Duration = Duration::from_millis(10);

#[tokio::test]
async fn wait_resumes_when_the_check_turns_true() {
    let stop = AtomicBool::new(false);
    let cancel = AtomicBool::new(false);
    let calls = std::sync::atomic::AtomicU32::new(0);
    let end = wait_until(
        || {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            async move { n >= 3 }
        },
        &stop,
        &cancel,
        MS,
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(end, WaitEnd::Resumed);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(end.outcome(), "resumed");
}

#[tokio::test]
async fn wait_stops_on_stop_or_cancel_quickly() {
    for use_cancel in [false, true] {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = if use_cancel { cancel.clone() } else { stop.clone() };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let end = wait_until(
            || async { false },
            &stop,
            &cancel,
            Duration::from_secs(60),
            Duration::from_secs(600),
        )
        .await;
        assert_eq!(end, WaitEnd::Stopped);
        assert_eq!(end.outcome(), "stopped");
        assert!(started.elapsed() < Duration::from_millis(200));
    }
}

#[tokio::test]
async fn wait_gives_up_after_the_limit() {
    let stop = AtomicBool::new(false);
    let cancel = AtomicBool::new(false);
    let end = wait_until(|| async { false }, &stop, &cancel, MS, Duration::from_millis(80)).await;
    assert_eq!(end, WaitEnd::GaveUp);
    assert_eq!(end.outcome(), "gave_up");
}

#[test]
fn the_registry_holds_one_wait() {
    let _g = crate::serial::network_waits();
    let w = waits();
    assert!(!w.stop_for_exit());
    assert!(w.current().is_none());
    let flag = w.begin("run-a", 42, Duration::from_secs(1800));
    let view = w.current().unwrap();
    assert_eq!(view.run_id, "run-a");
    assert_eq!(view.before_case_id, 42);
    assert!(view.give_up_in_s <= 1800);
    assert!(!w.stop("run-b"));
    assert!(!flag.load(Ordering::SeqCst));
    assert!(w.stop("run-a"));
    assert!(flag.load(Ordering::SeqCst));
    w.end("run-a");
    assert!(w.current().is_none());

    let flag = w.begin("run-c", 7, Duration::from_secs(60));
    assert!(w.stop_for_exit());
    assert!(flag.load(Ordering::SeqCst));
    w.end("run-c");
    assert!(w.current().is_none());
    assert!(!w.stop_for_exit());
}

#[test]
fn target_of_never_guesses_a_port() {
    assert_eq!(t("host,abc", None), None);
    assert_eq!(t("host,99999", None), None);
    assert_eq!(t("host,0", None), None);
    assert_eq!(t("host", Some(0)), None);
    assert_eq!(t(r"host\inst,1700", None), Some(("host".into(), 1700)));
    assert_eq!(t(r"host\inst", Some(1701)), Some(("host".into(), 1701)));
}

#[test]
fn target_of_takes_only_plain_tcp_hosts() {
    assert_eq!(t("TCP:db1", None), Some(("db1".into(), 1433)));
    assert_eq!(t("Tcp:db1,1500", None), Some(("db1".into(), 1500)));
    assert_eq!(t("[::1]", None), Some(("::1".into(), 1433)));
    assert_eq!(t("tcp:[fe80::1],1500", None), Some(("fe80::1".into(), 1500)));
    assert_eq!(t("10.0.0.5,1434", None), Some(("10.0.0.5".into(), 1434)));
    assert_eq!(t("[::1],abc", None), None);
    assert_eq!(t("[nonsense]", None), None);
    for bad in ["np:db1", "lpc:db1", r"NP:\\db1\pipe", ".", "(local)", "db 1", "db1;x", "a/b", "tcp:", "tcp:."] {
        assert_eq!(t(bad, None), None, "{bad}");
    }
}

#[tokio::test]
async fn a_stop_during_a_slow_check_is_seen_at_once() {
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = AtomicBool::new(false);
    let s = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        s.store(true, Ordering::SeqCst);
    });
    let started = Instant::now();
    let end = wait_until(
        || async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            false
        },
        &stop,
        &cancel,
        MS,
        Duration::from_secs(60),
    )
    .await;
    assert_eq!(end, WaitEnd::Stopped);
    assert!(started.elapsed() < Duration::from_millis(250));
}
