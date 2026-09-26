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
    // Compared with assert!, never assert_eq!: a failing assert_eq! prints
    // both sides, and one side here is a shipped credential.
    let is_shipped = |s: &MemoryStore| resolve(s, first.id).unwrap().as_deref() == Some(first.connection_string);
    assert!(is_shipped(&s), "{}: resolved to something other than the shipped string", first.id);
    save(&s, first.id, &form("someone", Some("pw1"))).unwrap();
    let got = resolve(&s, first.id).unwrap().unwrap();
    assert!(got.contains("User Id=someone") && got.contains("Password=pw1"));
    reset(&s, first.id).unwrap();
    assert!(is_shipped(&s), "{}: reset did not go back to the shipped string", first.id);
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
    assert!(c.password == shipped_pw, "{}: a blank password did not keep the shipped one", first.id);
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
    assert!(c.server == shipped.server, "{}: the form moved the server", first.id);
    assert!(c.database == shipped.database, "{}: the form moved the database", first.id);
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

fn own(server: &str, port: Option<u16>, database: &str, user: &str, password: Option<&str>) -> DbCredentialsForm {
    DbCredentialsForm { server: server.into(), port, database: database.into(), user: user.into(), password: password.map(Into::into), trust_cert: false }
}

fn customised(s: &MemoryStore, id: &str) -> bool {
    databases(s).into_iter().find(|d| d.id == id).unwrap().customised
}

/// Saving what the shipped login already is must not freeze it: a stored
/// copy would outlive the day the shipped password is rotated, and the
/// database would show as changed when nothing was.
#[test]
fn saving_the_shipped_login_unchanged_keeps_no_override() {
    let s = MemoryStore::default();
    let p = DB_PRESETS[0];
    let shipped = v2_lib::db::parse_connection(p.connection_string).unwrap();

    // Blank password on a fresh machine: the shipped one, so the shipped string.
    save(&s, p.id, &form(&shipped.user, None)).unwrap();
    assert!(!customised(&s, p.id), "{}: saving the shipped login stored an override", p.id);
    assert!(s.get(&format!("tcm-v2/db/{}", p.id)).unwrap().is_none(), "{}: an entry was written", p.id);

    // Typed back by hand over an existing override: the override goes.
    save(&s, p.id, &form("someone", Some("pw1"))).unwrap();
    assert!(customised(&s, p.id));
    save(&s, p.id, &form(&shipped.user, Some(&shipped.password))).unwrap();
    assert!(!customised(&s, p.id), "{}: typing the shipped login back kept the override", p.id);
    assert!(resolve(&s, p.id).unwrap().as_deref() == Some(p.connection_string), "{}: did not go back to the shipped string", p.id);
}

/// A blank password means "the one already saved" - for the same server.
/// Carrying it to a different server or database would hand one server's
/// password to another.
#[test]
fn own_needs_the_password_again_when_it_points_somewhere_new() {
    let s = MemoryStore::default();
    save(&s, OWN_ID, &own("db.local", Some(1444), "Hr", "me", Some("pw"))).unwrap();
    for moved in [
        own("db.other", Some(1444), "Hr", "me", None),
        own("db.local", Some(1500), "Hr", "me", None),
        own("db.local", None, "Hr", "me", None),
        own("db.local", Some(1444), "Payroll", "me", None),
    ] {
        assert_eq!(save(&s, OWN_ID, &moved).unwrap_err(), "Enter the password for the new server or database.", "{moved:?}");
    }
    // Same place, another user: the saved password still applies.
    save(&s, OWN_ID, &own("DB.local ", Some(1444), "Hr", "someone", None)).unwrap();
    let c = v2_lib::db::parse_connection(&resolve(&s, OWN_ID).unwrap().unwrap()).unwrap();
    assert_eq!((c.user.as_str(), c.password.as_str()), ("someone", "pw"));
}

/// Values are trimmed when a connection string is read back, so a password
/// with a space at either end could never sign in.
#[test]
fn a_password_with_a_space_at_either_end_is_refused() {
    let s = MemoryStore::default();
    for pw in [" pw", "pw ", "\tpw"] {
        assert_eq!(save(&s, OWN_ID, &own("h", None, "d", "u", Some(pw))).unwrap_err(), "The password can't start or end with a space.");
        assert_eq!(save(&s, DB_PRESETS[0].id, &form("u", Some(pw))).unwrap_err(), "The password can't start or end with a space.");
    }
    // A space inside is a character like any other.
    save(&s, OWN_ID, &own("h", None, "d", "u", Some("p w"))).unwrap();
}

#[test]
fn a_port_after_the_server_name_and_in_the_port_box_is_refused() {
    let s = MemoryStore::default();
    let err = save(&s, OWN_ID, &own("h,1433", Some(1433), "d", "u", Some("pw"))).unwrap_err();
    assert_eq!(err, "Put the port in the Port box, not after the server name.");
    // Written only after the name, with the box empty, it still works.
    save(&s, OWN_ID, &own("h,1433", None, "d", "u", Some("pw"))).unwrap();
    assert!(resolve(&s, OWN_ID).unwrap().unwrap().starts_with("Server=h,1433;"));
}
