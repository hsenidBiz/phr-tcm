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

// ------------------------------------------------- a person's own databases
//
// The list naming them is a file; these run it in a temp dir beside a
// `MemoryStore`, so neither the real vault nor the real app data is touched.

fn listed(dir: &tempfile::TempDir) -> WithList<MemoryStore> {
    WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") }
}

fn login(user: &str) -> DbCredentialsForm {
    own("db.example", Some(1444), "Hr", user, Some("pw-9"))
}

fn ids(s: &dyn SecretStore) -> Vec<String> {
    databases(s).into_iter().map(|d| d.id).collect()
}

fn list_text(dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(dir.path().join("databases.json")).unwrap_or_default()
}

/// Before any list exists, `own` is on it with the name it always had and
/// the login it already holds - so an environment or a saved choice that
/// names `own` means what it meant. Reading writes nothing.
#[test]
fn the_list_starts_with_own_and_keeps_its_login() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    s.put("tcm-v2/db/own", "Server=old-host;Database=Old;User Id=me;Password=pw").unwrap();
    let own_db = databases(&s).into_iter().find(|d| d.id == OWN_ID).expect("own is listed");
    assert_eq!(own_db.label, "Your own database");
    assert_eq!(own_db.server, "old-host");
    assert!(own_db.has_password && !own_db.shipped);
    assert_eq!(resolve(&s, OWN_ID).unwrap().as_deref(), Some("Server=old-host;Database=Old;User Id=me;Password=pw"));
    assert!(is_known(&s, OWN_ID));
    assert!(!dir.path().join("databases.json").exists(), "a read wrote the list");
}

#[test]
fn shipped_come_first_then_the_own_ones_in_the_order_they_were_added() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let a = add_custom(&s, "Staging", &login("a")).unwrap();
    let b = add_custom(&s, "QA box", &login("b")).unwrap();
    let mut want: Vec<String> = DB_PRESETS.iter().map(|p| p.id.to_string()).collect();
    want.extend([OWN_ID.to_string(), a.id.clone(), b.id.clone()]);
    assert_eq!(ids(&s), want);
    let labels: Vec<String> = databases(&s).into_iter().skip(DB_PRESETS.len()).map(|d| d.label).collect();
    assert_eq!(labels, ["Your own database", "Staging", "QA box"]);
}

#[test]
fn an_added_database_gets_a_fresh_id_its_own_login_and_is_known() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let d = add_custom(&s, "  Staging  ", &login("someone")).unwrap();
    let hex = d.id.strip_prefix("custom-").expect("a custom id");
    assert!(hex.len() == 8 && hex.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)), "{}", d.id);
    assert_eq!(d.label, "Staging");
    assert_eq!((d.user.as_str(), d.server.as_str(), d.port, d.database.as_str()), ("someone", "db.example", Some(1444), "Hr"));
    assert!(d.has_password && !d.shipped);
    assert!(is_known(&s, &d.id));
    assert_eq!(
        resolve(&s, &d.id).unwrap().as_deref(),
        Some("Server=db.example,1444;Database=Hr;User Id=someone;Password=pw-9")
    );
    // The list names it and holds no secret.
    let text = list_text(&dir);
    assert!(text.contains(&d.id) && text.contains("Staging"));
    assert!(!text.contains("pw-9") && !text.contains("db.example"), "{text}");
    // Saving and testing work on it like on `own`.
    save(&s, &d.id, &own("db.example", Some(1444), "Hr", "other", None)).unwrap();
    assert!(resolve(&s, &d.id).unwrap().unwrap().contains("User Id=other;Password=pw-9"));
    assert_eq!(reset(&s, &d.id).unwrap_err(), "Only a shipped database has a default to go back to.");
}

#[test]
fn an_add_needs_a_name_and_a_whole_login_and_adds_nothing_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    assert_eq!(add_custom(&s, "   ", &login("u")).unwrap_err(), "Enter a name for the database.");
    assert_eq!(add_custom(&s, &"x".repeat(81), &login("u")).unwrap_err(), "Keep the name to 80 characters or fewer.");
    assert!(add_custom(&s, &"x".repeat(80), &login("u")).is_ok());
    assert_eq!(add_custom(&s, "Nopw", &own("h", None, "d", "u", None)).unwrap_err(), "Enter a password.");
    assert_eq!(add_custom(&s, "Nouser", &own("h", None, "d", " ", Some("p"))).unwrap_err(), "Enter the user name.");
    assert_eq!(ids(&s).len(), DB_PRESETS.len() + 2, "only the good add landed");
}

#[test]
fn names_are_unique_ignoring_case_shipped_names_included() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let a = add_custom(&s, "Staging", &login("a")).unwrap();
    assert_eq!(add_custom(&s, "STAGING", &login("b")).unwrap_err(), "A database named \"STAGING\" already exists.");
    assert_eq!(
        add_custom(&s, "your own DATABASE", &login("b")).unwrap_err(),
        "A database named \"your own DATABASE\" already exists."
    );
    let shipped = DB_PRESETS[0].label.to_uppercase();
    assert_eq!(add_custom(&s, &shipped, &login("b")).unwrap_err(), format!("A database named \"{shipped}\" already exists."));
    // Renaming to its own name in another case is no clash with itself.
    assert_eq!(rename_custom(&s, &a.id, "staging").unwrap().label, "staging");
    assert!(rename_custom(&s, &a.id, "Your own database").is_err());
}

#[test]
fn a_rename_keeps_the_id_and_the_login() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let a = add_custom(&s, "Staging", &login("a")).unwrap();
    let renamed = rename_custom(&s, &a.id, "Staging 2").unwrap();
    assert_eq!((renamed.id.as_str(), renamed.label.as_str(), renamed.user.as_str()), (a.id.as_str(), "Staging 2", "a"));
    assert_eq!(databases(&s).last().unwrap().label, "Staging 2");
    assert_eq!(rename_custom(&s, OWN_ID, "Mine").unwrap().label, "Mine");
    assert_eq!(rename_custom(&s, DB_PRESETS[0].id, "Mine 2").unwrap_err(), "A shipped database keeps its name.");
    assert_eq!(rename_custom(&s, "custom-00000000", "x").unwrap_err(), "That database is not one the app knows.");
    assert_eq!(rename_custom(&s, &a.id, "").unwrap_err(), "Enter a name for the database.");
}

#[test]
fn a_removal_takes_the_login_and_the_name_and_the_id_is_never_handed_out_again() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let a = add_custom(&s, "Staging", &login("a")).unwrap();
    remove_custom(&s, &a.id).unwrap();
    assert_eq!(s.get(&format!("tcm-v2/db/{}", a.id)).unwrap(), None, "the secret stayed behind");
    assert!(!is_known(&s, &a.id));
    assert!(!ids(&s).contains(&a.id));
    assert_eq!(resolve(&s, &a.id).unwrap_err(), "That database is not one the app knows.");
    assert!(save(&s, &a.id, &login("x")).is_err(), "a removed database took a login");
    // Its name is free again, its id is not.
    let b = add_custom(&s, "Staging", &login("b")).unwrap();
    assert_ne!(b.id, a.id);
    assert!(list_text(&dir).contains(&a.id), "the removed id is not kept as retired");
    // `own` can go too, secret and all.
    s.put("tcm-v2/db/own", "Server=h;Database=d;User Id=u;Password=p").unwrap();
    remove_custom(&s, OWN_ID).unwrap();
    assert_eq!(s.get("tcm-v2/db/own").unwrap(), None);
    assert!(!is_known(&s, OWN_ID));
}

#[test]
fn a_shipped_database_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    for p in DB_PRESETS {
        assert_eq!(remove_custom(&s, p.id).unwrap_err(), "A shipped database can't be removed.");
        assert!(is_known(&s, p.id));
    }
    assert_eq!(remove_custom(&s, "custom-00000000").unwrap_err(), "That database is not one the app knows.");
}

#[test]
fn a_store_with_nowhere_to_keep_the_list_has_own_and_refuses_changes() {
    let s = MemoryStore::default();
    assert!(is_known(&s, OWN_ID));
    assert_eq!(add_custom(&s, "Staging", &login("a")).unwrap_err(), "There is nowhere to keep a list of databases.");
    assert!(s.get("tcm-v2/db/custom-00000000").unwrap().is_none());
    assert_eq!(ids(&s).len(), DB_PRESETS.len() + 1);
}

#[test]
fn a_list_that_cannot_be_read_is_left_alone_and_hides_only_the_own_ones() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    std::fs::write(dir.path().join("databases.json"), "{ not json").unwrap();
    assert!(!is_known(&s, OWN_ID));
    assert!(is_known(&s, DB_PRESETS[0].id));
    assert_eq!(ids(&s).len(), DB_PRESETS.len(), "the shipped ones still show");
    assert!(add_custom(&s, "Staging", &login("a")).unwrap_err().contains("not readable"));
    assert_eq!(list_text(&dir), "{ not json", "a broken list was replaced");
}

/// Sign-out of another account wipes every own database's login and the
/// list itself: what is left is how a fresh install starts.
#[test]
fn forget_all_clears_every_own_login_and_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let a = add_custom(&s, "Staging", &login("a")).unwrap();
    let b = add_custom(&s, "QA box", &login("b")).unwrap();
    save(&s, OWN_ID, &login("me")).unwrap();
    save(&s, DB_PRESETS[0].id, &form("x", Some("y"))).unwrap();
    forget_all(&s).unwrap();
    for id in [&a.id, &b.id] {
        assert_eq!(s.get(&format!("tcm-v2/db/{id}")).unwrap(), None, "{id}'s login stayed");
        assert!(!is_known(&s, id));
    }
    assert_eq!(resolve(&s, OWN_ID).unwrap(), None);
    let mut want: Vec<String> = DB_PRESETS.iter().map(|p| p.id.to_string()).collect();
    want.push(OWN_ID.to_string());
    assert_eq!(ids(&s), want);
    assert!(databases(&s).iter().all(|d| !d.customised));
    // Forgotten ids are retired too.
    let c = add_custom(&s, "Staging", &login("c")).unwrap();
    assert!(c.id != a.id && c.id != b.id);
}

/// The write rule is the connection's user - nothing about a database
/// being one of the person's own, or what it is called, changes that.
#[test]
fn the_write_rule_is_still_the_user_of_an_own_database() {
    use v2_lib::db::{access_for, Access};
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    let dev = add_custom(&s, "Dev writes", &login("app_DevLogin")).unwrap();
    let reader = add_custom(&s, "Staging devlogin", &login("reader")).unwrap();
    assert_eq!(access_for(&resolve(&s, &dev.id).unwrap().unwrap()), Access::DevWrites);
    assert_eq!(access_for(&resolve(&s, &reader.id).unwrap().unwrap()), Access::ReadOnly);
}

#[test]
fn a_view_of_an_own_database_never_carries_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let s = listed(&dir);
    add_custom(&s, "Staging", &own("h", None, "d", "u", Some("S3cret-XYZ"))).unwrap();
    let json = serde_json::to_string(&databases(&s)).unwrap();
    assert!(!json.contains("S3cret-XYZ") && !json.contains("Password="), "{json}");
}
