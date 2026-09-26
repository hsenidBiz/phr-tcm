//! The tester's own test accounts: entered once, saved in a plain file on
//! this machine, named by key from scripts.

use v2_lib::autorun::accounts::{
    account_for_run, find_account, load_accounts, save_accounts, session_path, valid_key, validate_accounts, Account,
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

/// A Windows device name is not a usable file name, whatever comes after a
/// dot - `sessions/con.json` and `sessions/con.x.json` are both `\\.\CON`.
#[test]
fn a_windows_device_name_is_not_a_usable_key() {
    for bad in [
        "con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9", "con.x", "com1.json", "nul.txt",
    ] {
        assert!(!valid_key(bad), "{bad:?} was accepted");
    }
    // Close but not actually a device name.
    for ok in ["console", "com10", "lpt0", "conx"] {
        assert!(valid_key(ok), "{ok:?} was refused");
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

/// `before` (the accounts list `save_accounts` compares against) comes
/// straight off disk with no validation. A hand-edited `accounts.json`
/// naming a key like `../../x` must not let the "this login changed, drop
/// its session" cleanup turn into a delete of a file outside `sessions/`.
#[test]
fn save_accounts_never_deletes_through_an_unvalidated_key_read_from_disk() {
    let base = tempfile::tempdir().unwrap();
    let dir = base.path().join("data");
    std::fs::create_dir_all(&dir).unwrap();
    let bad_key = "../../x";
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&[account(bad_key, "kim", "p1")]).unwrap(),
    )
    .unwrap();
    // session_path(dir, "../../x") = dir/sessions/../../x.json = base/x.json:
    // outside `dir` entirely, which is exactly what the guard must refuse to touch.
    let escape_target = session_path(&dir, bad_key);
    std::fs::write(&escape_target, "{}").unwrap();
    assert!(escape_target.exists());

    save_accounts(&dir, &[account("admin", "kim", "p1")]).unwrap();
    assert!(
        escape_target.exists(),
        "save_accounts deleted a file outside its own folder through an unvalidated key"
    );
}

#[test]
fn the_account_for_a_run_is_a_real_key_on_this_machine_or_nothing() {
    let dir = tempfile::tempdir().unwrap();
    save_accounts(dir.path(), &[account("hr.admin", "kim", "pw")]).unwrap();
    assert_eq!(account_for_run(dir.path(), None).unwrap(), None);
    assert_eq!(account_for_run(dir.path(), Some("  ")).unwrap(), None);
    assert_eq!(account_for_run(dir.path(), Some("hr.admin")).unwrap(), Some("hr.admin".to_string()));
    assert_eq!(account_for_run(dir.path(), Some("Bad Key")).unwrap_err(), "\"Bad Key\" is not a usable account key");
    assert_eq!(
        account_for_run(dir.path(), Some("ghost")).unwrap_err(),
        "there is no account \"ghost\" on this machine - add it in Auto Run, Accounts"
    );
}
