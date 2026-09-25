//! Where each database's login lives, and what a database id resolves to.
//! Everything here runs against `MemoryStore`: Windows Credential Manager is
//! the machine's real store and a test must not write to it.

use v2_lib::db::credentials::*;
use v2_lib::db_defaults::DB_PRESETS;

fn form(user: &str, password: Option<&str>) -> DbCredentialsForm {
    DbCredentialsForm { server: String::new(), port: None, database: String::new(), user: user.into(), password: password.map(Into::into), trust_cert: false }
}

#[test]
fn a_shipped_database_resolves_to_its_shipped_string_until_overridden() {
    let s = MemoryStore::default();
    let first = DB_PRESETS[0];
    assert_eq!(resolve(&s, first.id).unwrap().as_deref(), Some(first.connection_string));
    save(&s, first.id, &form("someone", Some("pw1"))).unwrap();
    let got = resolve(&s, first.id).unwrap().unwrap();
    assert!(got.contains("User Id=someone") && got.contains("Password=pw1"));
    reset(&s, first.id).unwrap();
    assert_eq!(resolve(&s, first.id).unwrap().as_deref(), Some(first.connection_string));
}

#[test]
fn own_resolves_to_nothing_until_saved() {
    let s = MemoryStore::default();
    assert_eq!(resolve(&s, OWN_ID).unwrap(), None);
    let f = DbCredentialsForm { server: "db.local".into(), port: Some(1444), database: "Hr".into(), user: "me".into(), password: Some("pw".into()), trust_cert: true };
    save(&s, OWN_ID, &f).unwrap();
    assert_eq!(resolve(&s, OWN_ID).unwrap().unwrap(), "Server=db.local,1444;Database=Hr;User Id=me;Password=pw;TrustServerCertificate=True");
}

#[test]
fn a_blank_password_keeps_the_current_one_including_the_shipped_one() {
    let s = MemoryStore::default();
    let first = DB_PRESETS[0];
    let shipped_pw = v2_lib::db::parse_connection(first.connection_string).unwrap().password;
    save(&s, first.id, &form("other", None)).unwrap();
    let c = v2_lib::db::parse_connection(&resolve(&s, first.id).unwrap().unwrap()).unwrap();
    assert_eq!(c.user, "other");
    assert_eq!(c.password, shipped_pw);
}

#[test]
fn the_public_view_never_carries_the_password() {
    let s = MemoryStore::default();
    save(&s, OWN_ID, &DbCredentialsForm { server: "h".into(), port: None, database: "d".into(), user: "u".into(), password: Some("S3cret-XYZ".into()), trust_cert: false }).unwrap();
    let json = serde_json::to_string(&databases(&s)).unwrap();
    assert!(!json.to_lowercase().contains("\"password\""));
    assert!(!json.contains("S3cret-XYZ"));
    for p in DB_PRESETS {
        let pw = v2_lib::db::parse_connection(p.connection_string).unwrap().password;
        assert!(!json.contains(&pw), "shipped password leaked for {}", p.id);
    }
    let own = databases(&s).into_iter().find(|d| d.id == OWN_ID).unwrap();
    assert!(own.has_password && own.customised && !own.shipped);
}

#[test]
fn a_semicolon_in_any_field_is_refused() {
    let s = MemoryStore::default();
    let err = save(&s, OWN_ID, &DbCredentialsForm { server: "h".into(), port: None, database: "d".into(), user: "u".into(), password: Some("a;b".into()), trust_cert: false }).unwrap_err();
    assert_eq!(err, "The login can't contain a semicolon.");
}

#[test]
fn a_shipped_form_cannot_move_the_server() {
    let s = MemoryStore::default();
    let first = DB_PRESETS[0];
    let f = DbCredentialsForm { server: "evil".into(), port: Some(1), database: "x".into(), user: "u".into(), password: Some("p".into()), trust_cert: true };
    save(&s, first.id, &f).unwrap();
    let c = v2_lib::db::parse_connection(&resolve(&s, first.id).unwrap().unwrap()).unwrap();
    let shipped = v2_lib::db::parse_connection(first.connection_string).unwrap();
    assert_eq!(c.server, shipped.server);
    assert_eq!(c.database, shipped.database);
}

#[test]
fn forget_all_removes_every_override() {
    let s = MemoryStore::default();
    save(&s, OWN_ID, &DbCredentialsForm { server: "h".into(), port: None, database: "d".into(), user: "u".into(), password: Some("p".into()), trust_cert: false }).unwrap();
    save(&s, DB_PRESETS[0].id, &form("x", Some("y"))).unwrap();
    forget_all(&s).unwrap();
    assert_eq!(resolve(&s, OWN_ID).unwrap(), None);
    assert!(databases(&s).iter().all(|d| !d.customised));
}

#[test]
fn find_shipped_ignores_order_case_and_spacing() {
    let p = DB_PRESETS[1];
    let mut parts: Vec<&str> = p.connection_string.split(';').filter(|x| !x.trim().is_empty()).collect();
    parts.reverse();
    let shuffled = parts.join(" ; ").replace("User Id", "USER ID");
    assert_eq!(find_shipped(&shuffled), Some(p.id));
    assert_eq!(find_shipped("Server=elsewhere;Database=x"), None);
}

#[test]
fn the_form_never_prints_its_password() {
    let f = form("u", Some("S3cret-XYZ"));
    let shown = format!("{f:?}");
    assert!(!shown.contains("S3cret-XYZ"));
    assert!(shown.contains("(hidden)"));
}

#[test]
fn an_unknown_id_is_refused_and_own_has_nothing_to_reset_to() {
    let s = MemoryStore::default();
    assert_eq!(resolve(&s, "prod").unwrap_err(), "That database is not one the app knows.");
    assert!(save(&s, "prod", &form("u", Some("p"))).is_err());
    assert_eq!(reset(&s, OWN_ID).unwrap_err(), "Only a shipped database has a default to go back to.");
}
