//! The tester's own test accounts: entered once, saved in a plain file on
//! this machine, named by key from scripts.

use v2_lib::autorun::accounts::{
    find_account, load_accounts, save_accounts, session_path, valid_key, validate_accounts, Account,
};

fn account(key: &str, username: &str, password: &str) -> Account {
    Account { key: key.into(), label: format!("{key} label"), username: username.into(), password: password.into() }
}

#[test]
fn a_key_is_short_lowercase_and_safe_as_a_file_name() {
    for ok in ["admin", "hr.supervisor", "emp-2", "a", "qa_lead.1"] {
        assert!(valid_key(ok), "{ok}");
    }
    for bad in ["", "Admin", " admin", ".hidden", "-x", "a/b", "a\\b", "a b", "../x", "naïve", &"k".repeat(65)] {
        assert!(!valid_key(bad), "{bad:?} was accepted");
    }
}

#[test]
fn validation_names_the_problem() {
    assert!(validate_accounts(&[account("admin", "a", "p")]).is_ok());
    let dup = validate_accounts(&[account("admin", "a", "p"), account("admin", "b", "q")]).unwrap_err();
    assert!(dup.contains("admin") && dup.contains("more than once"), "{dup}");
    let key = validate_accounts(&[account("Bad Key", "a", "p")]).unwrap_err();
    assert!(key.contains("Bad Key"), "{key}");
    let user = validate_accounts(&[account("admin", "  ", "p")]).unwrap_err();
    assert!(user.contains("username"), "{user}");
    // An empty password is allowed: some test accounts have none.
    assert!(validate_accounts(&[account("guest", "guest", "")]).is_ok());
}

/// Anything that prints an Account for a person (a log line, a panic, a
/// test failure) must not carry the password.
#[test]
fn debug_never_shows_the_password() {
    let shown = format!("{:?}", account("admin", "kim", "s3cret-Value"));
    assert!(shown.contains("kim") && shown.contains("admin"), "{shown}");
    assert!(!shown.contains("s3cret-Value"), "{shown}");
    assert!(shown.contains("(hidden)"), "{shown}");
}

#[test]
fn no_file_yet_is_an_empty_list_and_a_saved_list_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_accounts(dir.path()).unwrap().is_empty());
    let list = vec![account("admin", "kim", "p1"), account("emp", "lee", "p2")];
    assert!(save_accounts(dir.path(), &list).unwrap().is_empty());
    assert_eq!(load_accounts(dir.path()).unwrap(), list);
    assert_eq!(find_account(dir.path(), "emp").unwrap(), Some(list[1].clone()));
    assert_eq!(find_account(dir.path(), "nobody").unwrap(), None);
}

#[test]
fn an_invalid_list_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    save_accounts(dir.path(), &[account("admin", "kim", "p1")]).unwrap();
    assert!(save_accounts(dir.path(), &[account("admin", "", "x")]).is_err());
    assert_eq!(load_accounts(dir.path()).unwrap(), vec![account("admin", "kim", "p1")]);
}

/// A saved session belongs to the login it was made with. Change the
/// login, or remove the account, and the session goes with it.
#[test]
fn a_changed_or_removed_account_loses_its_saved_session() {
    let dir = tempfile::tempdir().unwrap();
    save_accounts(dir.path(), &[account("admin", "kim", "p1"), account("emp", "lee", "p2"), account("sup", "ann", "p3")]).unwrap();
    for key in ["admin", "emp", "sup"] {
        let p = session_path(dir.path(), key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    }
    // admin: label only (keeps its session); emp: password changed; sup: removed.
    let mut admin = account("admin", "kim", "p1");
    admin.label = "Administrator".into();
    let mut dropped = save_accounts(dir.path(), &[admin, account("emp", "lee", "NEW")]).unwrap();
    dropped.sort();
    assert_eq!(dropped, vec!["emp".to_string(), "sup".to_string()]);
    assert!(session_path(dir.path(), "admin").is_file());
    assert!(!session_path(dir.path(), "emp").exists());
    assert!(!session_path(dir.path(), "sup").exists());
}
