//! The account lookup: TCM reads a test login's password from the
//! environment's database table itself, so it never passes through the AI
//! assistant. The table and columns are per environment; the SELECT is built
//! from validated names; `propose_accounts` and the Fill buttons use it.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use v2_lib::ai_bridge::{accounts_lookup_set_in, accounts_propose_in, BridgeContext};
use v2_lib::autorun::account_lookup::{
    column_name, detect_password_column, look_up, looks_like_hash, password_candidates, resolve_password_column,
    table_parts, AccountLookup, Found, NoRows, DEFAULT_TABLE, DEFAULT_USERNAME_COLUMN,
};
use v2_lib::autorun::accounts::{load_accounts, save_accounts, session_path_for, Account};
use v2_lib::autorun::setup_find::RowSource;
use v2_lib::commands::environments::{fill_accounts_in, fill_proposals_in, save_with, EnvInput, FillCounts};
use v2_lib::db::credentials::MemoryStore;
use v2_lib::environments::{
    active_id, load_or_init, load_proposals, save_env, save_proposals, Environment, StoredProposal,
    PROPOSED_PASSWORD_NOT_TEST,
};

const SECRET: &str = "Db-Pw-7731";

/// A database that answers the column list and the password rows it is given,
/// and remembers every statement it was asked.
struct Fake {
    columns: Result<String, String>,
    rows: Result<String, String>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn new(columns: &[(&str, &str)], rows: &[(&str, &str)]) -> Fake {
        Fake { columns: Ok(columns_answer(columns)), rows: Ok(rows_answer(rows)), asked: Mutex::new(vec![]) }
    }

    fn standard(rows: &[(&str, &str)]) -> Fake {
        Fake::new(
            &[
                ("USRACC_USER_ID", "nvarchar"),
                ("USRACC_PWD", "nvarchar"),
                ("USRACC_PWD_ACTIVATE_DATE", "datetime"),
                ("USRACC_PROMPT_CHANGE_PW", "bit"),
            ],
            rows,
        )
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl RowSource for Fake {
    async fn select(&self, sql: &str) -> Result<String, String> {
        self.asked.lock().unwrap().push(sql.to_string());
        if sql.contains("INFORMATION_SCHEMA") { self.columns.clone() } else { self.rows.clone() }
    }
}

fn columns_answer(columns: &[(&str, &str)]) -> String {
    let body: Vec<String> = columns.iter().map(|(n, t)| format!("{n}\t{t}")).collect();
    format!("COLUMN_NAME\tDATA_TYPE\n-----------\t---------\n{}\n\n({} rows affected)\n", body.join("\n"), columns.len())
}

fn rows_answer(rows: &[(&str, &str)]) -> String {
    let body: Vec<String> = rows.iter().map(|(u, p)| format!("{u}\t{p}")).collect();
    format!(
        "USRACC_USER_ID\tUSRACC_PWD\n--------------\t----------\n{}\n\n({} rows affected)\n",
        body.join("\n"),
        rows.len()
    )
}

fn users(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

// ---- Names are plain identifiers, never free SQL ----

#[test]
fn identifiers_are_validated() {
    assert_eq!(table_parts("PeoplesHR.HS_SM_USER_ACCOUNT").unwrap(), ["PeoplesHR", "HS_SM_USER_ACCOUNT"]);
    assert_eq!(table_parts("[dbo].[Users]").unwrap(), ["dbo", "Users"]);
    assert_eq!(table_parts("Db1.dbo.Users").unwrap(), ["Db1", "dbo", "Users"]);
    assert_eq!(column_name("[USRACC_USER_ID]").unwrap(), "USRACC_USER_ID");
    for bad in [
        "x; DROP TABLE y",
        "dbo.Users; --",
        "dbo.'Users'",
        "dbo.\"Users\"",
        "dbo. Users",
        "dbo.User Name",
        "Users",
        "a.b.c.d",
        "dbo.",
        ".Users",
        "dbo..Users",
        "[dbo.Users]",
        "dbo.1Users",
        "dbo.Us-ers",
        "",
    ] {
        assert!(table_parts(bad).is_err(), "{bad:?} must be refused");
    }
    for bad in ["a b", "a;b", "a'b", "a-b", "1a", "a.b", ""] {
        assert!(column_name(bad).is_err(), "{bad:?} must be refused");
    }
    let mut lookup = AccountLookup::default();
    assert!(lookup.check().is_ok());
    lookup.password_column = "pwd; DROP".into();
    assert!(lookup.check().is_err());
}

#[test]
fn the_defaults_are_peopleshr() {
    let d = AccountLookup::default();
    assert_eq!((d.table.as_str(), d.username_column.as_str(), d.password_column.as_str()), (DEFAULT_TABLE, DEFAULT_USERNAME_COLUMN, ""));
    assert_eq!(DEFAULT_TABLE, "PeoplesHR.HS_SM_USER_ACCOUNT");
    assert_eq!(DEFAULT_USERNAME_COLUMN, "USRACC_USER_ID");
}

#[test]
fn the_select_is_exactly_as_specified_and_quotes_usernames() {
    let lookup = AccountLookup::default();
    let sql = lookup.select_sql("USRACC_PWD", &users(&["sup1", "o'brien"])).unwrap();
    assert_eq!(
        sql,
        "SELECT [USRACC_USER_ID], [USRACC_PWD] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT] WHERE [USRACC_USER_ID] IN (N'sup1', N'o''brien')"
    );
    // A bracketed table and a three-part one are written the same way.
    let b = AccountLookup { table: "[dbo].[Users]".into(), username_column: "[Login]".into(), password_column: String::new() };
    assert_eq!(
        b.select_sql("Secret", &users(&["a"])).unwrap(),
        "SELECT [Login], [Secret] FROM [dbo].[Users] WHERE [Login] IN (N'a')"
    );
    let three = AccountLookup { table: "Db1.dbo.Users".into(), ..b.clone() };
    assert!(three.select_sql("Secret", &users(&["a"])).unwrap().contains("FROM [Db1].[dbo].[Users]"));
    // Nothing to ask is not a query.
    assert!(lookup.select_sql("USRACC_PWD", &[]).is_err());
    // A quote or a semicolon in a username stays inside its literal.
    let evil = lookup.select_sql("USRACC_PWD", &users(&["x'); DROP TABLE t;--"])).unwrap();
    assert!(evil.ends_with("IN (N'x''); DROP TABLE t;--')"), "{evil}");
}

#[test]
fn the_column_list_query_reads_names_only() {
    let two = AccountLookup::default().columns_sql().unwrap();
    assert_eq!(
        two,
        "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = N'PeoplesHR' AND TABLE_NAME = N'HS_SM_USER_ACCOUNT'"
    );
    let three = AccountLookup { table: "Other.dbo.Users".into(), ..AccountLookup::default() }.columns_sql().unwrap();
    assert!(three.contains("FROM [Other].INFORMATION_SCHEMA.COLUMNS"), "{three}");
    assert!(three.contains("TABLE_SCHEMA = N'dbo' AND TABLE_NAME = N'Users'"), "{three}");
}

// ---- Password column detection ----

fn cols(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect()
}

#[test]
fn detection_picks_the_one_character_column_that_looks_like_a_password() {
    let columns = cols(&[
        ("USRACC_USER_ID", "nvarchar"),
        ("USRACC_PWD", "nvarchar"),
        ("USRACC_PWD_ACTIVATE_DATE", "datetime"),
        ("USRACC_PASSWORD_EXPIRY", "varchar"),
        ("USRACC_PROMPT_CHANGE_PW", "bit"),
        ("USRACC_PWD_FLG", "varchar"),
        ("USRACC_PASS_VALID", "varchar"),
    ]);
    assert_eq!(detect_password_column(&columns, "T").unwrap(), "USRACC_PWD");
}

#[test]
fn detection_with_none_or_several_names_the_candidates() {
    let none = detect_password_column(&cols(&[("A", "nvarchar"), ("B_DATE_PWD", "datetime")]), "S.T").unwrap_err();
    assert!(none.contains("S.T") && none.contains("set the password column"), "{none}");

    let several = cols(&[("USER_PASSWORD", "nvarchar"), ("USER_PWD_OLD", "varchar"), ("OTHER", "nvarchar")]);
    assert_eq!(password_candidates(&several), ["USER_PASSWORD", "USER_PWD_OLD"]);
    let said = detect_password_column(&several, "S.T").unwrap_err();
    assert!(said.contains("USER_PASSWORD") && said.contains("USER_PWD_OLD") && said.contains("set the password column"), "{said}");
    assert!(!said.contains("OTHER"));
}

#[test]
fn a_column_of_another_type_or_with_an_excluded_word_is_not_a_candidate() {
    for (name, kind) in [
        ("PWD_DATE", "varchar"),
        ("PWD_TIME", "varchar"),
        ("PWD_DAYS", "varchar"),
        ("PWD_FLAG", "varchar"),
        ("PWD_FLG", "varchar"),
        ("PASS_PROMPT", "varchar"),
        ("PASS_VALID_FOR", "varchar"),
        ("PASS_CHANGED", "varchar"),
        ("PASS_EXPIRES", "varchar"),
        ("PASSWORD", "int"),
        ("PASSWORD", "varbinary"),
    ] {
        assert!(password_candidates(&cols(&[(name, kind)])).is_empty(), "{name} {kind}");
    }
    assert_eq!(password_candidates(&cols(&[("Password", "NVARCHAR")])), ["Password"]);
}

// ---- A hash is not a password ----

#[test]
fn clear_hashes_are_never_used_and_ordinary_passwords_are() {
    for hash in [
        "5f4dcc3b5aa765d61d8327deb882cf99",
        "5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8",
        "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8",
        "0x5F4DCC3B5AA765D61D8327DEB882CF99",
        "$2a$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy",
        "$argon2id$v=19$m=65536,t=3,p=4$c29tZXNhbHQ$abc",
        "XohImNooBHFR0OVvjcYpJ3NgPQ1qq73WKhHvch0VQtg=",
        "X03MO1qnZdYdgyfeuILPmQ==",
        "{SHA}W6ph5Mm5Pz8GgiULbPgzG37mj9g=",
        &"a".repeat(64),
    ] {
        assert!(looks_like_hash(hash), "{hash}");
    }
    for password in ["Welcome1", "Test@1234", "Pa55 word", "p", "Db-Pw-7731", "Welcome2024!Welcome", "abcdef1234567890"] {
        assert!(!looks_like_hash(password), "{password}");
    }
}

// ---- The lookup itself ----

#[tokio::test]
async fn it_finds_hashes_and_missing_rows_for_each_login() {
    let fake = Fake::standard(&[
        ("SUP1", SECRET),
        ("adm1", "5f4dcc3b5aa765d61d8327deb882cf99"),
        ("emp1", "NULL"),
    ]);
    let found = look_up(&fake, &AccountLookup::default(), &users(&["sup1", "adm1", "emp1", "ghost"])).await.unwrap();
    // Matched without regard to case, either way round.
    assert_eq!(found.of("sup1"), Some(&Found::Password(SECRET.into())));
    assert_eq!(found.of("SUP1"), Some(&Found::Password(SECRET.into())));
    assert_eq!(found.of("adm1"), Some(&Found::Hash));
    assert_eq!(found.of("emp1"), Some(&Found::Empty));
    assert_eq!(found.of("ghost"), None);
    assert_eq!(found.password_column, "USRACC_PWD");
    assert!(!format!("{found:?}").contains(SECRET), "Debug hides the password");
}

#[tokio::test]
async fn it_detects_the_column_once_and_reads_names_before_values() {
    let fake = Fake::standard(&[("sup1", SECRET)]);
    look_up(&fake, &AccountLookup::default(), &users(&["sup1"])).await.unwrap();
    let asked = fake.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].contains("INFORMATION_SCHEMA.COLUMNS") && !asked[0].contains("sup1"));
    assert_eq!(
        asked[1],
        "SELECT [USRACC_USER_ID], [USRACC_PWD] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT] WHERE [USRACC_USER_ID] IN (N'sup1')"
    );
}

#[tokio::test]
async fn a_set_password_column_skips_the_detection() {
    let fake = Fake::standard(&[("sup1", SECRET)]);
    let lookup = AccountLookup { password_column: "[USRACC_PWD]".into(), ..AccountLookup::default() };
    let found = look_up(&fake, &lookup, &users(&["sup1"])).await.unwrap();
    assert_eq!(found.of("sup1"), Some(&Found::Password(SECRET.into())));
    assert_eq!(fake.asked().len(), 1);
}

#[tokio::test]
async fn failures_are_fixed_sentences_that_never_quote_what_the_database_said() {
    let leaky = |text: &str| Fake {
        columns: Ok(columns_answer(&[("USRACC_USER_ID", "nvarchar"), ("USRACC_PWD", "nvarchar")])),
        rows: Err(text.to_string()),
        asked: Mutex::new(vec![]),
    };
    let lookup = AccountLookup { password_column: "USRACC_PWD".into(), ..AccountLookup::default() };
    for (said, expect) in [
        ("Invalid object name 'PeoplesHR.Db-Pw-7731'.", "was not found in the database"),
        ("Invalid column name 'Db-Pw-7731'.", "was not found in PeoplesHR.HS_SM_USER_ACCOUNT"),
        ("Login timeout expired near Db-Pw-7731", "did not answer"),
        ("Msg 245 conversion failed 'Db-Pw-7731'", "refused the account lookup"),
    ] {
        let err = look_up(&leaky(said), &lookup, &users(&["sup1"])).await.unwrap_err();
        assert!(err.contains(expect), "{said} -> {err}");
        assert!(!err.contains("Db-Pw-7731"), "{err}");
    }
    // A table with no columns is a table that is not there.
    let missing = Fake { columns: Ok(columns_answer(&[])), rows: Ok(String::new()), asked: Mutex::new(vec![]) };
    let err = look_up(&missing, &AccountLookup::default(), &users(&["a"])).await.unwrap_err();
    assert!(err.contains("not found"), "{err}");
    // An ambiguous column says so, naming the candidates.
    let two = Fake::new(&[("USRACC_USER_ID", "nvarchar"), ("PWD_A", "nvarchar"), ("PASS_B", "nvarchar")], &[]);
    let err = look_up(&two, &AccountLookup::default(), &users(&["a"])).await.unwrap_err();
    assert!(err.contains("PWD_A") && err.contains("PASS_B") && err.contains("set the password column"), "{err}");
}

#[tokio::test]
async fn the_dry_check_reads_column_names_and_never_a_value() {
    let fake = Fake::standard(&[("sup1", SECRET)]);
    assert_eq!(resolve_password_column(&fake, &AccountLookup::default()).await.unwrap(), "USRACC_PWD");
    assert!(fake.asked().iter().all(|q| q.contains("INFORMATION_SCHEMA")), "{:?}", fake.asked());
    let wrong_user = AccountLookup { username_column: "NOPE".into(), ..AccountLookup::default() };
    assert!(resolve_password_column(&fake, &wrong_user).await.unwrap_err().contains("NOPE"));
    let wrong_pwd = AccountLookup { password_column: "NOPE".into(), ..AccountLookup::default() };
    assert!(resolve_password_column(&fake, &wrong_pwd).await.unwrap_err().contains("NOPE"));
}

// ---- The setting, with the environment ----

fn bridge_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    load_or_init(dir.path(), Some("dev-read")).unwrap();
    v2_lib::autorun::store::set_root(dir.path().to_path_buf());
    dir
}

fn known() -> Vec<String> {
    ["dev-read", "dev-login", "qa-read", "own"].iter().map(|s| s.to_string()).collect()
}

fn first_env(root: &Path) -> Environment {
    load_or_init(root, None).unwrap().environments.into_iter().next().unwrap()
}

fn mark_test_environment(root: &Path) {
    let mut e = first_env(root);
    e.test_environment = true;
    save_env(root, e, &known()).unwrap();
}

#[test]
fn an_environment_file_from_before_loads_with_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = load_or_init(root, None).unwrap();
    let mut json: Value = serde_json::from_str(&v2_lib::environments::file_text(&file).unwrap()).unwrap();
    assert!(json["environments"][0]["account_lookup"].is_object());
    json["environments"][0].as_object_mut().unwrap().remove("account_lookup");
    let after = v2_lib::environments::parse_file(json.to_string().as_bytes()).unwrap();
    assert_eq!(after.environments[0].id, file.environments[0].id);
    assert_eq!(after.environments[0].account_lookup, AccountLookup::default());
    // A partly written setting takes the defaults for the rest.
    let part: AccountLookup = serde_json::from_value(json!({ "table": "dbo.Users" })).unwrap();
    assert_eq!(part.table, "dbo.Users");
    assert_eq!(part.username_column, DEFAULT_USERNAME_COLUMN);
    assert_eq!(part.password_column, "");
}

#[tokio::test]
async fn edit_environments_saves_the_lookup_validates_it_and_keeps_it_when_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let e = first_env(root);
    let input = |lookup: Option<AccountLookup>| EnvInput {
        id: e.id.clone(),
        name: e.name.clone(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: e.db_id.clone(),
        test_environment: true,
        test_prefix: None,
        password_expiry_query: None,
        active_session_query: None,
        account_lookup: lookup,
    };
    let mine = AccountLookup { table: " [dbo].[Users] ".into(), username_column: "Login".into(), password_column: "Secret".into() };
    let view = save_with(root, &store, input(Some(mine))).await.unwrap();
    let saved = view.environments[0].account_lookup.clone();
    assert_eq!(saved, AccountLookup { table: "[dbo].[Users]".into(), username_column: "Login".into(), password_column: "Secret".into() });

    // Left out, an edit keeps what the environment has.
    let view = save_with(root, &store, input(None)).await.unwrap();
    assert_eq!(view.environments[0].account_lookup, saved);

    // Free SQL is refused, and nothing is changed.
    for bad in [
        AccountLookup { table: "x; DROP TABLE y".into(), ..saved.clone() },
        AccountLookup { username_column: "a b".into(), ..saved.clone() },
        AccountLookup { password_column: "p'".into(), ..saved.clone() },
        AccountLookup { table: String::new(), ..saved.clone() },
    ] {
        let err = save_with(root, &store, input(Some(bad))).await.unwrap_err();
        assert!(err.contains("account lookup"), "{err}");
    }
    assert_eq!(first_env(root).account_lookup, saved);
}

// ---- propose_accounts ----

fn proposal(key: &str, user: &str) -> Value {
    json!({ "key": key, "label": format!("{key} label"), "username": user })
}

fn with_password(key: &str, user: &str, password: &str) -> Value {
    let mut p = proposal(key, user);
    p["password"] = json!(password);
    p
}

fn without_password(key: &str, user: &str, why: &str) -> Value {
    let mut p = proposal(key, user);
    p["no_password"] = json!(why);
    p
}

async fn propose<S: RowSource>(rows: Result<&S, String>, accounts: Vec<Value>) -> (u16, String) {
    accounts_propose_in(&BridgeContext::default(), rows, &json!({ "accounts": accounts }).to_string()).await
}

fn stored(root: &Path) -> Vec<StoredProposal> {
    load_proposals(root, &active_id(root).unwrap()).unwrap()
}

#[tokio::test]
async fn proposing_on_a_test_environment_looks_the_passwords_up_and_the_table_wins() {
    let _log = crate::serial::log_tail();
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    mark_test_environment(root);
    let fake = Fake::standard(&[("sup1", SECRET), ("adm1", "5f4dcc3b5aa765d61d8327deb882cf99"), ("seed1", "Seed-From-Db")]);

    let (status, out) = propose(
        Ok(&fake),
        vec![
            proposal("hr.sup", "sup1"),
            proposal("hr.adm", "adm1"),
            // The agent's own password loses to the table's.
            with_password("hr.seed", "seed1", "Agent-Guess-1"),
            // No row: the agent's password stands (from a seed script, say).
            with_password("hr.script", "scr1", "From-Seed-Script"),
            without_password("hr.reason", "rsn1", "a single sign-on login"),
            proposal("hr.none", "nobody"),
        ],
    )
    .await;
    assert_eq!(status, 200, "{out}");

    let list = stored(root);
    let by = |k: &str| list.iter().find(|p| p.key == k).unwrap().clone();
    assert_eq!(by("hr.sup").password.as_deref(), Some(SECRET));
    assert_eq!(by("hr.sup").no_password, None);
    assert_eq!(by("hr.adm").password, None);
    assert_eq!(by("hr.adm").no_password.as_deref(), Some("the database holds only a hash"));
    assert_eq!(by("hr.seed").password.as_deref(), Some("Seed-From-Db"));
    assert_eq!(by("hr.script").password.as_deref(), Some("From-Seed-Script"));
    assert_eq!(by("hr.reason").no_password.as_deref(), Some("a single sign-on login"));
    assert_eq!(by("hr.none").no_password.as_deref(), Some("no row in PeoplesHR.HS_SM_USER_ACCOUNT for this login"));

    // Counts and keys only: never a password.
    assert!(out.contains("2 of 6 passwords found in PeoplesHR.HS_SM_USER_ACCOUNT"), "{out}");
    assert!(out.contains("no row for: hr.script, hr.reason, hr.none"), "{out}");
    assert!(out.contains("hash only: hr.adm"), "{out}");
    for secret in [SECRET, "Seed-From-Db", "Agent-Guess-1", "From-Seed-Script", "5f4dcc3b5aa765d61d8327deb882cf99"] {
        assert!(!out.contains(secret), "{out}");
    }
    let tail = v2_lib::applog::recent(500);
    assert!(tail.iter().all(|l| !l.message.contains(SECRET) && !l.message.contains("Seed-From-Db")), "{tail:?}");
    // The one SELECT carried the usernames and nothing else.
    assert!(fake.asked().iter().any(|q| q.starts_with("SELECT [USRACC_USER_ID], [USRACC_PWD] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT]")));
}

#[tokio::test]
async fn proposing_with_no_passwords_at_all_is_enough_on_a_test_environment() {
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    mark_test_environment(root);
    let fake = Fake::standard(&[("sup1", SECRET)]);
    let (status, out) = propose(Ok(&fake), vec![proposal("hr.sup", "sup1")]).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("1 of 1 passwords found"), "{out}");
    assert_eq!(stored(root)[0].password.as_deref(), Some(SECRET));
}

#[tokio::test]
async fn a_failed_lookup_still_saves_the_proposal_and_says_why_and_how_to_fix_it() {
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    mark_test_environment(root);

    // The database is not there at all: the module is off, say.
    let (status, out) = propose::<NoRows>(
        Err("reading the database is off - turn the Database Access module on in Settings".into()),
        vec![with_password("hr.sup", "sup1", "From-Seed-Script"), proposal("hr.emp", "emp1")],
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("the account lookup did not run: reading the database is off"), "{out}");
    assert!(out.contains("Edit environments") && out.contains("set_account_lookup"), "{out}");
    assert!(!out.contains("From-Seed-Script"), "{out}");
    let list = stored(root);
    assert_eq!(list.len(), 2, "saved all the same");
    assert_eq!(list[0].password.as_deref(), Some("From-Seed-Script"), "an agent's password still applies");
    assert_eq!((list[1].password.clone(), list[1].no_password.clone()), (None, None), "neither is required any more");

    // The table is not found: the answer says so, with the table's name.
    let missing = Fake {
        columns: Ok(columns_answer(&[("USRACC_USER_ID", "nvarchar"), ("PWD_A", "nvarchar"), ("PASS_B", "nvarchar")])),
        rows: Ok(String::new()),
        asked: Mutex::new(vec![]),
    };
    let (status, out) = propose(Ok(&missing), vec![proposal("hr.emp", "emp1")]).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("PWD_A") && out.contains("PASS_B") && out.contains("set the password column"), "{out}");

    let gone = Fake { columns: Err("Invalid object name 'x'".into()), rows: Err(String::new()), asked: Mutex::new(vec![]) };
    let (status, out) = propose(Ok(&gone), vec![proposal("hr.emp", "emp1")]).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("PeoplesHR.HS_SM_USER_ACCOUNT was not found"), "{out}");
}

#[tokio::test]
async fn outside_a_test_environment_nothing_changes_and_no_lookup_is_made() {
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    let fake = Fake::standard(&[("sup1", SECRET)]);

    let (status, out) = propose(Ok(&fake), vec![proposal("hr.sup", "sup1")]).await;
    assert_eq!(status, 200, "{out}");
    assert!(!out.contains("passwords found"), "{out}");
    assert_eq!(stored(root)[0].password, None);

    let (status, out) = propose(Ok(&fake), vec![with_password("hr.sup", "sup1", "Agent-Guess-1")]).await;
    assert_eq!((status, out.as_str()), (400, PROPOSED_PASSWORD_NOT_TEST));
    assert!(fake.asked().is_empty(), "the database was never asked: {:?}", fake.asked());
}

// ---- set_account_lookup ----

#[tokio::test]
async fn set_account_lookup_validates_saves_on_the_active_test_environment_and_checks_the_columns() {
    let _log = crate::serial::log_tail();
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    let body = |t: &str, u: &str, p: Option<&str>| {
        let mut v = json!({ "table": t, "username_column": u });
        if let Some(p) = p {
            v["password_column"] = json!(p);
        }
        v.to_string()
    };
    let fake = Fake::standard(&[]);

    // Not a test environment: refused, nothing saved.
    let (status, out) = accounts_lookup_set_in(Ok(&fake), &body("dbo.Users", "Login", None)).await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("test environment"), "{out}");
    assert_eq!(first_env(root).account_lookup, AccountLookup::default());

    mark_test_environment(root);

    // Free SQL is refused before anything is saved.
    for bad in [body("x; DROP", "Login", None), body("dbo.Users", "a b", None), body("dbo.Users", "Login", Some("p'"))] {
        let (status, out) = accounts_lookup_set_in(Ok(&fake), &bad).await;
        assert_eq!(status, 400, "{bad}: {out}");
    }
    assert_eq!(accounts_lookup_set_in(Ok(&fake), "{}").await.0, 400);
    assert_eq!(accounts_lookup_set_in(Ok(&fake), r#"{"table":"a.b","username_column":"c","x":1}"#).await.0, 400);
    assert_eq!(first_env(root).account_lookup, AccountLookup::default());

    // Saved directly, with the dry check from the column list alone.
    let (status, out) = accounts_lookup_set_in(
        Ok(&fake),
        &body("PeoplesHR.HS_SM_USER_ACCOUNT", "USRACC_USER_ID", None),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("The lookup can work") && out.contains("USRACC_PWD"), "{out}");
    assert_eq!(
        first_env(root).account_lookup,
        AccountLookup { table: DEFAULT_TABLE.into(), username_column: DEFAULT_USERNAME_COLUMN.into(), password_column: String::new() }
    );
    assert!(fake.asked().iter().all(|q| q.contains("INFORMATION_SCHEMA")), "{:?}", fake.asked());

    // Another table the database does not have is saved, and said not to work yet.
    let gone = Fake { columns: Err("Invalid object name 'x'".into()), rows: Err(String::new()), asked: Mutex::new(vec![]) };
    let (status, out) = accounts_lookup_set_in(Ok(&gone), &body("dbo.Users", "Login", Some("Secret"))).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("cannot work yet") && out.contains("dbo.Users was not found"), "{out}");
    let saved = first_env(root).account_lookup;
    assert_eq!((saved.table.as_str(), saved.username_column.as_str(), saved.password_column.as_str()), ("dbo.Users", "Login", "Secret"));

    // No database at all: still saved, and the sentence says why it is unchecked.
    let (status, out) = accounts_lookup_set_in::<NoRows>(Err("reading the database is off".into()), &body("dbo.Users", "Login", None)).await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("cannot work yet") && out.contains("reading the database is off"), "{out}");

    // Names only in the log.
    let tail = v2_lib::applog::recent(500);
    assert!(tail.iter().any(|l| l.message.contains("changed the account lookup")), "{tail:?}");
}

// ---- Fill passwords from database ----

fn seed_proposals(root: &Path, list: &[(&str, &str, Option<&str>, Option<&str>)]) {
    let list: Vec<StoredProposal> = list
        .iter()
        .map(|(k, u, p, r)| StoredProposal {
            key: k.to_string(),
            label: k.to_string(),
            username: u.to_string(),
            role: None,
            password: p.map(str::to_string),
            no_password: r.map(str::to_string),
            marks: vec![],
            proposed_ms: None,
        })
        .collect();
    save_proposals(root, &active_id(root).unwrap(), &list).unwrap();
}

#[tokio::test]
async fn filling_the_proposals_updates_their_passwords_and_reasons_and_answers_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    seed_proposals(
        root,
        &[
            ("hr.sup", "sup1", None, Some("no row in x for this login")),
            ("hr.same", "same1", Some("Same-Pw"), None),
            ("hr.adm", "adm1", Some("Old-Pw"), None),
            ("hr.gone", "gone1", Some("Kept-Pw"), None),
            ("hr.bare", "bare1", None, None),
        ],
    );
    let fake = Fake::standard(&[("sup1", SECRET), ("same1", "Same-Pw"), ("adm1", "5f4dcc3b5aa765d61d8327deb882cf99")]);

    let counts = fill_proposals_in(root, Ok(&fake)).await.unwrap();
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 1, no_row: 2, hash_only: 1 });
    let list = load_proposals(root, &active_id(root).unwrap()).unwrap();
    let by = |k: &str| list.iter().find(|p| p.key == k).unwrap().clone();
    assert_eq!((by("hr.sup").password.as_deref(), by("hr.sup").no_password.clone()), (Some(SECRET), None));
    assert_eq!(by("hr.same").password.as_deref(), Some("Same-Pw"));
    assert_eq!((by("hr.adm").password.clone(), by("hr.adm").no_password.as_deref()), (None, Some("the database holds only a hash")));
    assert_eq!(by("hr.gone").password.as_deref(), Some("Kept-Pw"), "no row keeps what the proposal had");
    assert!(by("hr.bare").no_password.as_deref().unwrap().starts_with("no row in "));
    assert!(!format!("{counts:?}").contains(SECRET));
}

#[tokio::test]
async fn filling_needs_a_test_environment_and_names_why_a_lookup_failed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    seed_proposals(root, &[("hr.sup", "sup1", None, None)]);
    let fake = Fake::standard(&[("sup1", SECRET)]);
    let err = fill_proposals_in(root, Ok(&fake)).await.unwrap_err();
    assert!(err.contains("test environment"), "{err}");
    assert!(fake.asked().is_empty());

    mark_test_environment(root);
    let err = fill_proposals_in::<NoRows>(root, Err("reading the database is off".into())).await.unwrap_err();
    assert!(err.contains("the account lookup did not run: reading the database is off") && err.contains("Edit environments"), "{err}");
    assert_eq!(load_proposals(root, &active_id(root).unwrap()).unwrap()[0].password, None, "nothing changed");

    let err = fill_accounts_in::<NoRows>(root, Err("the database did not answer".into()), &[]).await.unwrap_err();
    assert!(err.contains("no saved accounts") || err.contains("did not run"), "{err}");
}

fn account(key: &str, user: &str, password: &str) -> Account {
    Account { key: key.into(), label: key.into(), username: user.into(), password: password.into() }
}

#[tokio::test]
async fn filling_saved_accounts_changes_only_what_differs_the_way_a_person_would() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    let env_id = active_id(root).unwrap();
    save_accounts(
        root,
        &[
            account("hr.sup", "sup1", "Old-Pw"),
            account("hr.same", "same1", "Same-Pw"),
            account("hr.adm", "adm1", "Adm-Pw"),
            account("hr.gone", "gone1", "Gone-Pw"),
        ],
    )
    .unwrap();
    // Each has a saved sign-in session; a changed password must drop its own.
    for k in ["hr.sup", "hr.same", "hr.adm", "hr.gone"] {
        let p = session_path_for(root, &env_id, k);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    }
    v2_lib::autorun::accounts::set_pending_password(root, &env_id, "hr.sup", "Pending-Pw").unwrap();

    let fake = Fake::standard(&[("SUP1", SECRET), ("same1", "Same-Pw"), ("adm1", "5f4dcc3b5aa765d61d8327deb882cf99")]);
    let counts = fill_accounts_in(root, Ok(&fake), &[]).await.unwrap();
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 1, no_row: 1, hash_only: 1 });

    let now = load_accounts(root).unwrap();
    let pw = |k: &str| now.iter().find(|a| a.key == k).unwrap().password.clone();
    assert_eq!(pw("hr.sup"), SECRET);
    assert_eq!(pw("hr.same"), "Same-Pw");
    assert_eq!(pw("hr.adm"), "Adm-Pw", "a hash is never saved");
    assert_eq!(pw("hr.gone"), "Gone-Pw", "no row leaves it as it is");
    assert!(!session_path_for(root, &env_id, "hr.sup").exists(), "a changed password drops its saved session");
    assert!(session_path_for(root, &env_id, "hr.same").exists(), "an unchanged one keeps it");
    assert!(session_path_for(root, &env_id, "hr.adm").exists());
    assert_eq!(v2_lib::autorun::accounts::pending_password_for(root, &env_id, "hr.sup"), None, "a password held pending is let go");

    // Nothing differs now: nothing is written.
    let before = v2_lib::autorun::accounts::accounts_version(root, &env_id);
    let again = fill_accounts_in(root, Ok(&fake), &[]).await.unwrap();
    assert_eq!(again, FillCounts { updated: 0, unchanged: 2, no_row: 1, hash_only: 1 });
    assert_eq!(v2_lib::autorun::accounts::accounts_version(root, &env_id), before);
}

#[tokio::test]
async fn filling_named_accounts_leaves_the_others_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("hr.a", "a1", "Old-A"), account("hr.b", "b1", "Old-B")]).unwrap();
    let fake = Fake::standard(&[("a1", "New-A"), ("b1", "New-B")]);
    let counts = fill_accounts_in(root, Ok(&fake), &["hr.a".to_string()]).await.unwrap();
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 0, no_row: 0, hash_only: 0 });
    let now = load_accounts(root).unwrap();
    assert_eq!(now[0].password, "New-A");
    assert_eq!(now[1].password, "Old-B");
    // Only the named one was asked about.
    assert!(fake.asked().iter().filter(|q| !q.contains("INFORMATION_SCHEMA")).all(|q| q.contains("N'a1'") && !q.contains("N'b1'")), "{:?}", fake.asked());
}
