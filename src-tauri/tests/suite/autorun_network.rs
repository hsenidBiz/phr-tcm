//! Finding the environment's database server, knocking on it, and the
//! unattended run's wait for it to answer again.



use std::time::{Duration, Instant};
use v2_lib::autorun::network::{
    answers, db_target, target_of, CONNECT_TIMEOUT,
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
