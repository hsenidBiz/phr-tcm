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
    table_parts, AccountLookup, Found, NoRows, DEFAULT_PASSWORD_COLUMN, DEFAULT_TABLE, DEFAULT_USERNAME_COLUMN,
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
                ("USRACC_PASSWORD", "nvarchar"),
                ("USRACC_PASSWORD_ACTIVATE_DATE", "datetime"),
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

/// The lookup of a table whose password column is not named: it is detected.
fn detect() -> AccountLookup {
    AccountLookup { password_column: String::new(), ..AccountLookup::default() }
}

fn columns_answer(columns: &[(&str, &str)]) -> String {
    let body: Vec<String> = columns.iter().map(|(n, t)| format!("{n}\t{t}")).collect();
    format!("COLUMN_NAME\tDATA_TYPE\n-----------\t---------\n{}\n\n({} rows affected)\n", body.join("\n"), columns.len())
}

fn rows_answer(rows: &[(&str, &str)]) -> String {
    let body: Vec<String> = rows.iter().map(|(u, p)| format!("{u}\t{p}")).collect();
    format!(
        "USRACC_USER_ID\tUSRACC_PASSWORD\n--------------\t---------------\n{}\n\n({} rows affected)\n",
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
    assert_eq!((d.table.as_str(), d.username_column.as_str(), d.password_column.as_str()), (DEFAULT_TABLE, DEFAULT_USERNAME_COLUMN, "USRACC_PASSWORD"));
    assert_eq!(DEFAULT_PASSWORD_COLUMN, "USRACC_PASSWORD");
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
    assert_eq!(found.password_column, "USRACC_PASSWORD");
    assert!(!format!("{found:?}").contains(SECRET), "Debug hides the password");
}

#[tokio::test]
async fn it_detects_the_column_once_and_reads_names_before_values() {
    let fake = Fake::standard(&[("sup1", SECRET)]);
    look_up(&fake, &detect(), &users(&["sup1"])).await.unwrap();
    let asked = fake.asked();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].contains("INFORMATION_SCHEMA.COLUMNS") && !asked[0].contains("sup1"));
    assert_eq!(
        asked[1],
        "SELECT [USRACC_USER_ID], [USRACC_PASSWORD] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT] WHERE [USRACC_USER_ID] IN (N'sup1')"
    );
}

#[tokio::test]
async fn a_set_password_column_skips_the_detection() {
    let fake = Fake::standard(&[("sup1", SECRET)]);
    let lookup = AccountLookup { password_column: "[USRACC_PASSWORD]".into(), ..AccountLookup::default() };
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
    let err = look_up(&missing, &detect(), &users(&["a"])).await.unwrap_err();
    assert!(err.contains("not found"), "{err}");
    // An ambiguous column says so, naming the candidates.
    let two = Fake::new(&[("USRACC_USER_ID", "nvarchar"), ("PWD_A", "nvarchar"), ("PASS_B", "nvarchar")], &[]);
    let err = look_up(&two, &detect(), &users(&["a"])).await.unwrap_err();
    assert!(err.contains("PWD_A") && err.contains("PASS_B") && err.contains("set the password column"), "{err}");
}

#[tokio::test]
async fn the_dry_check_reads_column_names_and_never_a_value() {
    let fake = Fake::standard(&[("sup1", SECRET)]);
    assert_eq!(resolve_password_column(&fake, &detect()).await.unwrap(), "USRACC_PASSWORD");
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
    assert_eq!(part.password_column, "USRACC_PASSWORD");
}

fn loaded_password_column(table: &str, column: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let file = load_or_init(dir.path(), None).unwrap();
    let mut json: Value = serde_json::from_str(&v2_lib::environments::file_text(&file).unwrap()).unwrap();
    json["environments"][0]["account_lookup"] =
        json!({ "table": table, "username_column": "USRACC_USER_ID", "password_column": column });
    let after = v2_lib::environments::parse_file(json.to_string().as_bytes()).unwrap();
    after.environments[0].account_lookup.password_column.clone()
}

#[test]
fn a_saved_environment_with_no_password_column_reads_the_default_on_the_default_table() {
    assert_eq!(loaded_password_column(DEFAULT_TABLE, ""), "USRACC_PASSWORD");
    assert_eq!(loaded_password_column("[PeoplesHR].[HS_SM_USER_ACCOUNT]", ""), "USRACC_PASSWORD");
    assert_eq!(loaded_password_column(" peopleshr.hs_sm_user_account ", " "), "USRACC_PASSWORD");
}

#[test]
fn a_custom_table_with_no_password_column_still_detects_it() {
    assert_eq!(loaded_password_column("dbo.Users", ""), "");
}

#[test]
fn a_named_password_column_is_kept() {
    assert_eq!(loaded_password_column(DEFAULT_TABLE, "USRACC_PWD"), "USRACC_PWD");
    assert_eq!(loaded_password_column("dbo.Users", "Secret"), "Secret");
}

#[test]
fn an_environment_file_read_from_disk_gets_the_default_password_column() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = load_or_init(root, None).unwrap();
    let mut json: Value = serde_json::from_str(&v2_lib::environments::file_text(&file).unwrap()).unwrap();
    json["environments"][0]["account_lookup"]["password_column"] = json!("");
    std::fs::write(v2_lib::autorun::roots::shared_of(root).join("environments.json"), json.to_string()).unwrap();
    assert_eq!(first_env(root).account_lookup.password_column, "USRACC_PASSWORD");
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
    assert!(fake.asked().iter().any(|q| q.starts_with("SELECT [USRACC_USER_ID], [USRACC_PASSWORD] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT]")));
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
    // Detection only runs for a table with no named password column.
    let id = active_id(root).unwrap();
    v2_lib::environments::set_account_lookup(root, &id, AccountLookup { table: "dbo.Users".into(), ..detect() }).unwrap();
    let (status, out) = propose(Ok(&missing), vec![proposal("hr.emp", "emp1")]).await;
    v2_lib::environments::set_account_lookup(root, &id, AccountLookup::default()).unwrap();
    assert_eq!(status, 200, "{out}");
    assert!(out.contains("PWD_A") && out.contains("PASS_B") && out.contains("set the password column"), "{out}");

    let gone = Fake { columns: Err("Invalid object name 'x'".into()), rows: Err("Invalid object name 'x'".into()), asked: Mutex::new(vec![]) };
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
    assert!(out.contains("The lookup can work") && out.contains("USRACC_PASSWORD"), "{out}");
    assert_eq!(
        first_env(root).account_lookup,
        AccountLookup { table: DEFAULT_TABLE.into(), username_column: DEFAULT_USERNAME_COLUMN.into(), password_column: DEFAULT_PASSWORD_COLUMN.into() }
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
            password_source: None,
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
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 1, no_row: 2, hash_only: 1, other_database: 0 });
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
    Account { key: key.into(), label: key.into(), username: user.into(), password: password.into(), password_source: None }
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
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 1, no_row: 1, hash_only: 1, other_database: 0 });

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
    assert_eq!(again, FillCounts { updated: 0, unchanged: 2, no_row: 1, hash_only: 1, other_database: 0 });
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
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 0, no_row: 0, hash_only: 0, other_database: 0 });
    let now = load_accounts(root).unwrap();
    assert_eq!(now[0].password, "New-A");
    assert_eq!(now[1].password, "Old-B");
    // Only the named one was asked about.
    assert!(fake.asked().iter().filter(|q| !q.contains("INFORMATION_SCHEMA")).all(|q| q.contains("N'a1'") && !q.contains("N'b1'")), "{:?}", fake.asked());
}

// ---- Review fixes -----------------------------------------------------------

use std::path::PathBuf;
use std::time::Duration;
use v2_lib::autorun::account_lookup::{DB_TOOL_OFF, SAME_AS_DB_LOGIN};
use v2_lib::autorun::setup_find::SqlcmdRows;
use v2_lib::db::{parse_connection, Output, Runner};
use v2_lib::db_defaults::DB_PRESETS;

/// A stand-in for the sqlcmd process: whatever it prints goes through the real
/// `run_query` (the guard, the row cap, the connection's password hidden, the
/// unreachable mapping) before the lookup sees it.
struct Process {
    users: Vec<(String, String)>,
    unreachable: bool,
    asked: Mutex<Vec<String>>,
}

impl Process {
    fn new(users: Vec<(String, String)>) -> Process {
        Process { users, unreachable: false, asked: Mutex::new(vec![]) }
    }
    fn selects(&self) -> Vec<String> {
        self.asked.lock().unwrap().iter().filter(|q| !q.contains("INFORMATION_SCHEMA")).cloned().collect()
    }
}

impl Runner for Process {
    async fn run(&self, _exe: &std::path::Path, args: &[String], _env: &[(String, String)], _t: Duration) -> Result<Output, String> {
        let sql = args.iter().find(|a| a.starts_with("SELECT")).cloned().unwrap_or_default();
        self.asked.lock().unwrap().push(sql.clone());
        if self.unreachable {
            return Ok(Output {
                status: 1,
                stdout: String::new(),
                stderr: "Sqlcmd: Error: Microsoft ODBC Driver: Login timeout expired.".into(),
            });
        }
        let out = if sql.contains("INFORMATION_SCHEMA") {
            columns_answer(&[("USRACC_USER_ID", "nvarchar"), ("USRACC_PASSWORD", "nvarchar")])
        } else {
            let wanted: Vec<String> = sql
                .split("N'")
                .skip(1)
                .map(|p| p.split('\'').next().unwrap_or("").to_lowercase())
                .collect();
            let rows: Vec<(&str, &str)> = self
                .users
                .iter()
                .filter(|(u, _)| wanted.contains(&u.to_lowercase()))
                .map(|(u, p)| (u.as_str(), p.as_str()))
                .collect();
            rows_answer(&rows)
        };
        Ok(Output { status: 0, stdout: out, stderr: String::new() })
    }
}

fn through_run_query(process: Process) -> SqlcmdRows<Process> {
    SqlcmdRows { runner: process, exe: PathBuf::from("sqlcmd"), conn: parse_connection(DB_PRESETS[0].connection_string).unwrap() }
}

#[tokio::test]
async fn a_password_the_database_layer_hides_is_never_used() {
    let db_password = parse_connection(DB_PRESETS[0].connection_string).unwrap().password;
    assert!(!db_password.is_empty());
    let rows = through_run_query(Process::new(vec![
        ("sup1".into(), db_password.clone()),
        ("adm1".into(), format!("pre-{db_password}-post")),
        ("emp1".into(), SECRET.into()),
    ]));
    let found = look_up(&rows, &AccountLookup::default(), &users(&["sup1", "adm1", "emp1"])).await.unwrap();
    assert_eq!(found.of("sup1"), Some(&Found::Masked));
    assert_eq!(found.of("adm1"), Some(&Found::Masked));
    assert_eq!(found.of("emp1"), Some(&Found::Password(SECRET.into())));
    assert!(!format!("{found:?}").contains(&db_password));
}

/// An assistant that read the table through db_query saw the masked value
/// too: sent back as a password, it is never kept - not when the lookup
/// finds the same masked row, and not when the lookup cannot run at all.
#[tokio::test]
async fn a_masked_value_the_assistant_sends_as_a_password_is_never_kept() {
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    mark_test_environment(root);
    let db_password = parse_connection(DB_PRESETS[0].connection_string).unwrap().password;
    let rows = through_run_query(Process::new(vec![("sup1".into(), db_password.clone())]));

    let (status, out) = propose(Ok(&rows), vec![with_password("hr.sup", "sup1", "(hidden)")]).await;
    assert_eq!(status, 200, "{out}");
    let list = stored(root);
    assert_eq!(list[0].password, None);
    assert_eq!(list[0].no_password.as_deref(), Some(SAME_AS_DB_LOGIN));

    // The lookup failing does not let it through either.
    let failed: Result<&SqlcmdRows<Process>, String> = Err("no database connection is chosen".into());
    let (status, out) = propose(failed, vec![with_password("hr.sup", "sup1", "x(hidden)y")]).await;
    assert_eq!(status, 200, "{out}");
    let list = stored(root);
    assert_eq!(list[0].password, None);
    assert_eq!(list[0].no_password.as_deref(), Some(SAME_AS_DB_LOGIN));
    assert!(!out.contains("(hidden)"), "{out}");
}

#[tokio::test]
async fn a_masked_login_is_proposed_with_its_reason_and_filled_without_touching_the_saved_password() {
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    mark_test_environment(root);
    let db_password = parse_connection(DB_PRESETS[0].connection_string).unwrap().password;
    let rows = through_run_query(Process::new(vec![("sup1".into(), db_password.clone()), ("emp1".into(), SECRET.into())]));

    let (status, out) = propose(Ok(&rows), vec![proposal("hr.sup", "sup1"), proposal("hr.emp", "emp1")]).await;
    assert_eq!(status, 200, "{out}");
    let list = stored(root);
    assert_eq!(list[0].password, None);
    assert_eq!(list[0].no_password.as_deref(), Some(SAME_AS_DB_LOGIN));
    assert_eq!(list[1].password.as_deref(), Some(SECRET));
    assert!(!out.contains(&db_password) && !out.contains("(hidden)"), "{out}");

    // The saved account that shares the database login's password keeps its own.
    save_accounts(root, &[account("hr.sup", "sup1", "Good-Saved-Pw"), account("hr.emp", "emp1", "Old")]).unwrap();
    let env_id = active_id(root).unwrap();
    std::fs::create_dir_all(session_path_for(root, &env_id, "hr.sup").parent().unwrap()).unwrap();
    std::fs::write(session_path_for(root, &env_id, "hr.sup"), "{}").unwrap();
    let counts = fill_accounts_in(root, Ok(&rows), &[]).await.unwrap();
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 1, no_row: 0, hash_only: 0, other_database: 0 });
    let now = load_accounts(root).unwrap();
    assert_eq!(now[0].password, "Good-Saved-Pw");
    assert_eq!(now[1].password, SECRET);
    assert!(session_path_for(root, &env_id, "hr.sup").exists(), "an untouched account keeps its session");
}

#[tokio::test]
async fn an_unreachable_database_is_said_as_not_answering_through_the_real_mapping() {
    let mut process = Process::new(vec![]);
    process.unreachable = true;
    let rows = through_run_query(process);
    let err = look_up(&rows, &AccountLookup::default(), &users(&["a"]))
        .await
        .unwrap_err();
    assert!(err.contains("did not answer"), "{err}");
    let err = look_up(&rows, &detect(), &users(&["a"])).await.unwrap_err();
    assert!(err.contains("did not answer"), "also while reading the column list: {err}");
}

#[tokio::test]
async fn more_logins_than_one_answer_holds_are_looked_up_in_chunks() {
    let all: Vec<(String, String)> = (0..450).map(|i| (format!("user{i:04}"), format!("pw-{i}"))).collect();
    let rows = through_run_query(Process::new(all.clone()));
    let lookup = AccountLookup::default();
    let names: Vec<String> = all.iter().map(|(u, _)| u.clone()).collect();
    let found = look_up(&rows, &lookup, &names).await.unwrap();
    for (u, p) in &all {
        assert_eq!(found.of(u), Some(&Found::Password(p.clone())), "{u}");
    }
    let selects = rows.runner.selects();
    assert_eq!(selects.len(), 3, "450 logins in chunks of at most 150");
    assert!(selects.iter().all(|q| q.chars().count() < 15_500), "each statement stays well under the guard's limit");
}

#[tokio::test]
async fn very_long_usernames_chunk_by_statement_length() {
    let all: Vec<(String, String)> = (0..80).map(|i| (format!("{}{i:03}", "x".repeat(400)), format!("pw-{i}"))).collect();
    let rows = through_run_query(Process::new(all.clone()));
    let lookup = AccountLookup::default();
    let names: Vec<String> = all.iter().map(|(u, _)| u.clone()).collect();
    let found = look_up(&rows, &lookup, &names).await.unwrap();
    assert!(all.iter().all(|(u, p)| found.of(u) == Some(&Found::Password(p.clone()))));
    assert!(rows.runner.selects().len() >= 3);
}

#[tokio::test]
async fn a_table_with_too_many_columns_to_list_asks_for_the_column() {
    let many: Vec<(String, String)> = (0..300).map(|i| (format!("COL_{i}"), "nvarchar".to_string())).collect();
    let pairs: Vec<(&str, &str)> = many.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    struct Wide(String);
    impl Runner for Wide {
        async fn run(&self, _e: &std::path::Path, _a: &[String], _env: &[(String, String)], _t: Duration) -> Result<Output, String> {
            Ok(Output { status: 0, stdout: self.0.clone(), stderr: String::new() })
        }
    }
    let rows = SqlcmdRows {
        runner: Wide(columns_answer(&pairs)),
        exe: PathBuf::from("sqlcmd"),
        conn: parse_connection(DB_PRESETS[0].connection_string).unwrap(),
    };
    let err = look_up(&rows, &detect(), &users(&["a"])).await.unwrap_err();
    assert!(err.contains("too many columns") && err.contains("set the password column"), "{err}");
}

#[tokio::test]
async fn filling_the_saved_accounts_loses_to_a_change_made_during_the_lookup() {
    struct Hooked<'a>(Fake, &'a Path);
    impl RowSource for Hooked<'_> {
        async fn select(&self, sql: &str) -> Result<String, String> {
            let answer = self.0.select(sql).await;
            if !sql.contains("INFORMATION_SCHEMA") {
                // The person saves a new password for hr.a while the lookup runs.
                v2_lib::autorun::accounts::set_password_for(self.1, &active_id(self.1).unwrap(), "hr.a", "Typed-Meanwhile").unwrap();
            }
            answer
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("hr.a", "a1", "Old-A"), account("hr.b", "b1", "Old-B")]).unwrap();
    let hooked = Hooked(Fake::standard(&[("a1", "From-Db-A"), ("b1", "From-Db-B")]), root);
    let counts = fill_accounts_in(root, Ok(&hooked), &[]).await.unwrap();
    assert_eq!(counts, FillCounts { updated: 1, unchanged: 1, no_row: 0, hash_only: 0, other_database: 0 });
    let now = load_accounts(root).unwrap();
    assert_eq!(now[0].password, "Typed-Meanwhile", "the change made during the lookup wins");
    assert_eq!(now[1].password, "From-Db-B");
}

#[tokio::test]
async fn filling_proposals_checks_the_environment_before_reading_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    let path = v2_lib::environments::proposals_path_for(root, &active_id(root).unwrap());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not json").unwrap();
    let fake = Fake::standard(&[]);
    let err = fill_proposals_in(root, Ok(&fake)).await.unwrap_err();
    assert!(err.contains("test environment"), "the environment is judged first: {err}");
    let err = fill_accounts_in(root, Ok(&fake), &[]).await.unwrap_err();
    assert!(err.contains("test environment"), "{err}");
}

#[tokio::test]
async fn a_switched_off_database_tool_is_not_called_the_module_being_off() {
    use v2_lib::ai_bridge::route;
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    mark_test_environment(dir.path());
    let ctx = BridgeContext { disabled_tools: vec!["db_query".to_string()], ..BridgeContext::default() };
    let body = json!({ "accounts": [proposal("hr.sup", "sup1")] }).to_string();
    let (status, out) = route(&ctx, None, "POST", "/accounts-propose", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    if v2_lib::modules::on(v2_lib::modules::Module::DbAccess) {
        assert!(out.contains(DB_TOOL_OFF), "{out}");
        assert!(!out.contains("Database Access module"), "{out}");
    }
}

// ---- Fill passwords from database: two safeguards ----

use v2_lib::commands::environments::{add_proposals_with, fill_db_change_in, AccountInput, FillDbChange};

fn point_environment_at(root: &Path, db_id: &str) {
    let mut e = first_env(root);
    e.db_id = db_id.to_string();
    save_env(root, e, &known()).unwrap();
}

fn sourced(key: &str, user: &str, password: &str, source: Option<&str>) -> Account {
    Account { password_source: source.map(str::to_string), ..account(key, user, password) }
}

#[tokio::test]
async fn a_fill_records_the_database_each_password_came_from_and_a_hand_typed_one_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(
        root,
        &[account("hr.sup", "sup1", "Old-Pw"), account("hr.same", "same1", "Same-Pw"), account("hr.gone", "gone1", "Gone-Pw")],
    )
    .unwrap();
    let fake = Fake::standard(&[("sup1", SECRET), ("same1", "Same-Pw")]);
    fill_accounts_in(root, Ok(&fake), &[]).await.unwrap();
    let now = load_accounts(root).unwrap();
    let source = |k: &str| now.iter().find(|a| a.key == k).unwrap().password_source.clone();
    assert_eq!(source("hr.sup").as_deref(), Some("dev-read"), "a filled password remembers its database");
    assert_eq!(source("hr.same"), None, "a password that was not written is not claimed");
    assert_eq!(source("hr.gone"), None);
    // The source is an id in the list, never a secret: nothing but the id is kept.
    let file = std::fs::read_to_string(v2_lib::autorun::accounts::accounts_path_for(root, &active_id(root).unwrap())).unwrap();
    assert!(file.contains("\"password_source\": \"dev-read\""), "{file}");
    assert!(!file.contains("Server="), "{file}");
}

#[tokio::test]
async fn a_fill_counts_the_updated_passwords_that_came_from_a_different_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    point_environment_at(root, "qa-read");
    save_accounts(
        root,
        &[
            sourced("hr.a", "a1", "Old-A", Some("dev-read")),
            sourced("hr.b", "b1", "Old-B", Some("qa-read")),
            // Typed by hand: no source, never counted.
            sourced("hr.c", "c1", "Old-C", None),
            // From elsewhere, but the same in this database: not updated, not counted.
            sourced("hr.d", "d1", "Same-D", Some("dev-read")),
        ],
    )
    .unwrap();
    let fake = Fake::standard(&[("a1", "New-A"), ("b1", "New-B"), ("c1", "New-C"), ("d1", "Same-D")]);
    let counts = fill_accounts_in(root, Ok(&fake), &[]).await.unwrap();
    assert_eq!(counts, FillCounts { updated: 3, unchanged: 1, no_row: 0, hash_only: 0, other_database: 1 });
    let now = load_accounts(root).unwrap();
    for k in ["hr.a", "hr.b", "hr.c"] {
        assert_eq!(now.iter().find(|a| a.key == k).unwrap().password_source.as_deref(), Some("qa-read"), "{k}");
    }
    // Filling again from the same database counts none.
    save_accounts(root, &[sourced("hr.a", "a1", "Old-A", Some("qa-read"))]).unwrap();
    assert_eq!(fill_accounts_in(root, Ok(&fake), &[]).await.unwrap().other_database, 0);
}

#[tokio::test]
async fn proposals_filled_from_the_database_carry_their_source_to_the_account_when_added() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    seed_proposals(root, &[("hr.sup", "sup1", Some("Old-Pw"), None), ("hr.typed", "typed1", None, None)]);
    let fake = Fake::standard(&[("sup1", SECRET), ("typed1", "Db-Typed")]);
    let counts = fill_proposals_in(root, Ok(&fake)).await.unwrap();
    assert_eq!((counts.updated, counts.other_database), (2, 0));
    assert_eq!(stored(root).iter().find(|p| p.key == "hr.sup").unwrap().password_source.as_deref(), Some("dev-read"));

    // The environment moves to another database and the proposals are filled again.
    point_environment_at(root, "qa-read");
    let other = Fake::standard(&[("sup1", "Qa-Sup"), ("typed1", "Db-Typed")]);
    let again = fill_proposals_in(root, Ok(&other)).await.unwrap();
    assert_eq!((again.updated, again.unchanged, again.other_database), (1, 1, 1));

    // Accepted: the password the proposal held keeps its source; a typed one has none.
    let store = MemoryStore::default();
    let pick = |key: &str, user: &str, password: &str| AccountInput {
        key: key.into(),
        label: key.into(),
        username: user.into(),
        password: password.into(),
    };
    add_proposals_with(root, &store, vec![pick("hr.sup", "sup1", ""), pick("hr.typed", "typed1", "Typed-By-Hand")], vec![])
        .unwrap();
    let now = load_accounts(root).unwrap();
    let find = |k: &str| now.iter().find(|a| a.key == k).unwrap().clone();
    assert_eq!(find("hr.sup").password, "Qa-Sup");
    assert_eq!(find("hr.sup").password_source.as_deref(), Some("qa-read"));
    assert_eq!(find("hr.typed").password, "Typed-By-Hand");
    assert_eq!(find("hr.typed").password_source, None, "typed by hand has no source");
}

#[tokio::test]
async fn the_environments_last_fill_database_is_recorded_and_a_change_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("hr.a", "a1", "Old-A")]).unwrap();
    let env_id = active_id(root).unwrap();

    // Before any Fill there is nothing to compare with: no question.
    assert_eq!(v2_lib::environments::last_fill_db(root, &env_id), None);
    assert_eq!(fill_db_change_in(root, &store).unwrap(), None);

    let fake = Fake::standard(&[("a1", "New-A")]);
    fill_accounts_in(root, Ok(&fake), &[]).await.unwrap();
    assert_eq!(v2_lib::environments::last_fill_db(root, &env_id).as_deref(), Some("dev-read"));
    assert_eq!(fill_db_change_in(root, &store).unwrap(), None, "the same database asks nothing");

    // The environment's database changes (the AI Bridge card, or Edit environments).
    point_environment_at(root, "qa-read");
    let change: FillDbChange = fill_db_change_in(root, &store).unwrap().expect("a change is found");
    let label = |id: &str| DB_PRESETS.iter().find(|p| p.id == id).map(|p| p.label.to_string()).unwrap();
    assert_eq!(change, FillDbChange { from: label("dev-read"), to: label("qa-read") });
    assert!(!change.from.contains("Server=") && !change.to.contains("Password"), "labels only: {change:?}");
    // Asking is not a Fill: cancelling leaves the record as it was.
    assert_eq!(v2_lib::environments::last_fill_db(root, &env_id).as_deref(), Some("dev-read"));
    assert!(fill_db_change_in(root, &store).unwrap().is_some());

    // A Fill that ran against the new database records it, and the question is over.
    fill_accounts_in(root, Ok(&fake), &[]).await.unwrap();
    assert_eq!(v2_lib::environments::last_fill_db(root, &env_id).as_deref(), Some("qa-read"));
    assert_eq!(fill_db_change_in(root, &store).unwrap(), None);
}

#[tokio::test]
async fn a_fill_that_did_not_run_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("hr.a", "a1", "Old-A")]).unwrap();
    let env_id = active_id(root).unwrap();
    let _ = fill_accounts_in::<NoRows>(root, Err("the database module is off".to_string()), &[]).await.unwrap_err();
    assert_eq!(v2_lib::environments::last_fill_db(root, &env_id), None);
}

#[tokio::test]
async fn proposing_reports_a_changed_database_and_counts_saved_passwords_from_another_one() {
    let _log = crate::serial::log_tail();
    let _root = crate::serial::autorun();
    let dir = bridge_root();
    let root = dir.path();
    mark_test_environment(root);
    let env_id = active_id(root).unwrap();
    save_accounts(root, &[sourced("hr.sup", "sup1", "Old-Pw", Some("dev-read"))]).unwrap();
    // A Fill read dev-read earlier; the environment uses qa-read now.
    v2_lib::environments::set_last_fill_db(root, &env_id, "dev-read");
    point_environment_at(root, "qa-read");
    let ctx = BridgeContext { db_secrets: Some(std::sync::Arc::new(MemoryStore::default())), ..BridgeContext::default() };
    let fake = Fake::standard(&[("sup1", SECRET)]);
    let (status, out) =
        accounts_propose_in(&ctx, Ok(&fake), &json!({ "accounts": [proposal("hr.sup", "sup1")] }).to_string()).await;
    assert_eq!(status, 200, "{out}");
    let label = |id: &str| DB_PRESETS.iter().find(|p| p.id == id).map(|p| p.label.to_string()).unwrap();
    assert!(out.contains(&format!("now uses {} instead of {}", label("qa-read"), label("dev-read"))), "{out}");
    assert!(out.contains("1 of them match saved accounts whose password came from a different database"), "{out}");
    assert!(!out.contains(SECRET), "{out}");
    // No person is there to ask, and a proposal is not a Fill: nothing is recorded.
    assert_eq!(v2_lib::environments::last_fill_db(root, &env_id).as_deref(), Some("dev-read"));
    assert_eq!(stored(root)[0].password_source.as_deref(), Some("qa-read"));
}

// ---- Sync Database: add accounts from the database -------------------------

use std::collections::HashSet;
use v2_lib::autorun::account_lookup::{
    key_for_login, EMPTY_PASSWORD, HASH_ONLY, LIST_LIMIT, NOT_TEST,
};
use v2_lib::commands::autorun::save_accounts_in;
use v2_lib::commands::environments::{add_logins_in, list_logins_in, AddedCounts};

fn logins_answer(names: &[&str]) -> String {
    let body: Vec<String> = names.iter().map(|u| u.to_string()).collect();
    format!("USRACC_USER_ID\n--------------\n{}\n\n({} rows affected)\n", body.join("\n"), names.len())
}

fn list_fake(names: &[&str]) -> Fake {
    Fake { columns: Ok(String::new()), rows: Ok(logins_answer(names)), asked: Mutex::new(vec![]) }
}

#[test]
fn the_list_select_is_app_built_and_a_read() {
    let l = AccountLookup::default();
    assert_eq!(
        l.list_sql("", 200).unwrap(),
        "SELECT TOP (200) [USRACC_USER_ID] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT] WHERE [USRACC_USER_ID] IS NOT NULL ORDER BY [USRACC_USER_ID]"
    );
    assert_eq!(
        l.list_sql("hr", 200).unwrap(),
        "SELECT TOP (200) [USRACC_USER_ID] FROM [PeoplesHR].[HS_SM_USER_ACCOUNT] WHERE [USRACC_USER_ID] IS NOT NULL AND [USRACC_USER_ID] LIKE N'%hr%' ESCAPE N'!' ORDER BY [USRACC_USER_ID]"
    );
    let odd = l.list_sql("a%b_c[d'e!", 200).unwrap();
    assert!(odd.contains("LIKE N'%a!%b!_c![d''e!!%' ESCAPE N'!'"), "{odd}");
}

#[test]
fn keys_are_made_from_logins() {
    let none = HashSet::new();
    assert_eq!(key_for_login("HR.Admin@x", &none), "hr.admin-x");
    let taken: HashSet<String> = ["hr.admin-x".to_string(), "hr.admin-x-2".to_string()].into();
    assert_eq!(key_for_login("HR.Admin@x", &taken), "hr.admin-x-3");
    assert_eq!(key_for_login("con", &none), "user-con");
    assert_eq!(key_for_login("", &none), "user-1");
    assert_eq!(key_for_login("@@@", &none), "user-1");
    let one: HashSet<String> = ["user-1".to_string()].into();
    assert_eq!(key_for_login("@@@", &one), "user-2");
    let two: HashSet<String> = ["user-1".to_string(), "user-2".to_string()].into();
    assert_eq!(key_for_login("", &two), "user-3");
    assert!(v2_lib::autorun::accounts::valid_key(&key_for_login("@@@", &none)));
    assert_eq!(key_for_login("a  b", &none), "a-b");
    let long = "a".repeat(100);
    assert_eq!(key_for_login(&long, &none).len(), 64);
    let clash: HashSet<String> = ["a".repeat(64)].into();
    let k = key_for_login(&long, &clash);
    assert_eq!(k.len(), 64);
    assert!(k.ends_with("-2"));
}

#[tokio::test]
async fn listing_marks_saved_logins_case_insensitively_and_carries_no_password() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("hr.a", "A.One", "Secret-Pw-1")]).unwrap();
    let fake = list_fake(&["a.one", "b.two"]);
    let list = list_logins_in(root, Ok(&fake), "").await.unwrap();
    assert_eq!(list.logins.len(), 2);
    assert!(list.logins[0].saved);
    assert!(!list.logins[1].saved);
    assert!(!list.cut_short);
    let json = format!("{list:?}");
    assert!(!json.contains("Secret-Pw-1"));
    assert!(fake.asked().iter().all(|q| !q.contains("USRACC_PASSWORD")), "{:?}", fake.asked());
}

#[tokio::test]
async fn a_full_list_is_cut_short_and_the_gates_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    let names: Vec<String> = (0..LIST_LIMIT).map(|i| format!("u{i:03}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let fake = list_fake(&refs);
    // Not a test environment yet.
    let err = list_logins_in(root, Ok(&fake), "").await.unwrap_err();
    assert!(err.contains(NOT_TEST), "{err}");
    mark_test_environment(root);
    assert!(list_logins_in(root, Ok(&fake), "").await.unwrap().cut_short);
    let off = list_logins_in::<NoRows>(root, Err(v2_lib::autorun::account_lookup::DB_OFF.to_string()), "").await.unwrap_err();
    assert!(off.contains("reading the database is off"), "{off}");
}

#[tokio::test]
async fn adding_logins_reads_passwords_in_rust_and_says_why_some_were_not_added() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("hr.old", "old1", "Old-Pw")]).unwrap();
    let fake = Fake::standard(&[
        ("HR.New@x", "New-Pw"),
        ("old1", "Other"),
        ("hashy", "5f4dcc3b5aa765d61d8327deb882cf99"),
        ("empty1", ""),
        ("mask1", "(hidden)"),
    ]);
    let asked = users(&["HR.New@x", "OLD1", "hashy", "empty1", "mask1", "ghost"]);
    let counts = add_logins_in(root, Ok(&fake), &asked).await.unwrap();
    assert_eq!(counts.added, 1);
    assert_eq!(counts.already_saved, 1);
    let why = |l: &str| counts.not_added.iter().find(|n| n.login == l).map(|n| n.reason.clone());
    assert_eq!(why("hashy").as_deref(), Some(HASH_ONLY));
    assert_eq!(why("empty1").as_deref(), Some(EMPTY_PASSWORD));
    assert_eq!(why("mask1").as_deref(), Some(SAME_AS_DB_LOGIN));
    assert!(why("ghost").unwrap().starts_with("no row in"));
    assert!(!format!("{counts:?}").contains("New-Pw"));

    let now = load_accounts(root).unwrap();
    assert_eq!(now.len(), 2);
    assert_eq!(now[0].password, "Old-Pw", "existing accounts are untouched");
    let made = &now[1];
    assert_eq!(made.key, "hr.new-x");
    assert_eq!(made.label, "HR.New@x");
    assert_eq!(made.username, "HR.New@x");
    assert_eq!(made.password, "New-Pw");
    assert_eq!(made.password_source, Some(load_or_init(root, None).unwrap().environments[0].db_id.clone()));

    let again: AddedCounts = add_logins_in(root, Ok(&fake), &users(&["hr.new@x"])).await.unwrap();
    assert_eq!((again.added, again.already_saved), (0, 1));
}

#[test]
fn saving_with_a_blank_key_makes_it_from_the_login() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    let env_id = active_id(root).unwrap();
    let saved = save_accounts_in(
        root,
        &env_id,
        vec![account("", "HR.Admin", "p1"), account("", "hr.admin", "p2"), account("typed.key", "x", "p3"), account("", "typed.key", "p4")],
        None,
    )
    .unwrap();
    let made: Vec<(u32, &str)> = saved.made.iter().map(|m| (m.index, m.key.as_str())).collect();
    assert_eq!(made, [(0, "hr.admin"), (1, "hr.admin-2"), (3, "typed.key-2")]);
    assert!(!format!("{saved:?}").contains("p1"), "the answer carries no password");
    assert_eq!(load_accounts(root).unwrap().len(), 4);

    // A blank key with a blank username keeps the old refusal.
    let err = save_accounts_in(root, &env_id, vec![account("", " ", "p")], None).unwrap_err();
    assert!(err.contains("not a usable account key"), "{err}");
    // A typed key is still validated.
    let err = save_accounts_in(root, &env_id, vec![account("Bad Key", "u", "p")], None).unwrap_err();
    assert!(err.contains("not a usable account key"), "{err}");
}

#[tokio::test]
async fn logins_compare_by_unicode_lowercase() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    save_accounts(root, &[account("elan", "Élan", "Pw-1")]).unwrap();
    let fake = list_fake(&["élan", "other"]);
    let list = list_logins_in(root, Ok(&fake), "").await.unwrap();
    assert!(list.logins[0].saved, "Élan and élan are the same login");
    assert!(!list.logins[1].saved);
    let counts: AddedCounts = add_logins_in(root, Ok(&fake), &users(&["ÉLAN"])).await.unwrap();
    assert_eq!((counts.added, counts.already_saved), (0, 1));
}

#[tokio::test]
async fn long_searches_and_long_login_lists_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    mark_test_environment(root);
    let fake = list_fake(&["a"]);
    let fine = "a".repeat(128);
    assert!(list_logins_in(root, Ok(&fake), &fine).await.is_ok());
    let err = list_logins_in(root, Ok(&fake), &"a".repeat(129)).await.unwrap_err();
    assert_eq!(err, "the search is too long - use fewer characters");

    let many: Vec<String> = (0..=LIST_LIMIT).map(|i| format!("u{i}")).collect();
    let err = add_logins_in(root, Ok(&fake), &many).await.unwrap_err();
    assert_eq!(err, "too many logins at once - tick fewer and add them again");
    let ok: Vec<String> = (0..LIST_LIMIT).map(|i| format!("u{i}")).collect();
    let at_limit = add_logins_in(root, Ok(&fake), &ok).await;
    assert!(at_limit.err().is_none_or(|e| !e.contains("too many")), "the limit itself is allowed");
}

#[test]
fn saving_an_empty_list_removes_every_account_and_drops_their_saved_sessions() {
    // Remove all on the Accounts tab saves an empty list.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, Some("dev-read")).unwrap();
    let env_id = active_id(root).unwrap();
    save_accounts(root, &[account("hr.a", "a1", "Pw-A"), account("hr.b", "b1", "Pw-B")]).unwrap();
    for k in ["hr.a", "hr.b"] {
        let p = session_path_for(root, &env_id, k);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    }
    let saved = save_accounts_in(root, &env_id, vec![], None).unwrap();
    assert!(saved.made.is_empty());
    let mut dropped = saved.dropped.clone();
    dropped.sort();
    assert_eq!(dropped, ["hr.a", "hr.b"], "a removed account's saved session goes with it, as for one removed row");
    assert!(load_accounts(root).unwrap().is_empty());
    assert!(!session_path_for(root, &env_id, "hr.a").exists());
}
