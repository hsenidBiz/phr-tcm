//! Environments: one named website, database and default password, one of
//! them active. The store is a plain JSON file under the Auto Run root; the
//! default password lives only in the secret store (`MemoryStore` here -
//! a test must never write to the real Credential Manager).

use std::path::Path;
use v2_lib::commands::autorun_record::RecorderClaim;
use v2_lib::commands::autorun_replay::OneAtATime;
use v2_lib::commands::environments::{
    clear_default_password_with, list_view, refuse_switch, remove_with, save_with, set_active_with,
    set_default_password_with, EnvInput, SWITCH_BUSY, SWITCH_SUPERVISED_OPEN, SWITCH_TEMPLATE_RUNNING,
};
use v2_lib::db::credentials::{MemoryStore, SecretStore};
use v2_lib::environments::*;

fn known() -> Vec<String> {
    ["dev-read", "dev-login", "qa-read", "own"].iter().map(|s| s.to_string()).collect()
}

fn env(id: &str, name: &str, start_url: &str) -> Environment {
    Environment {
        id: id.into(),
        name: name.into(),
        start_url: start_url.into(),
        allowed_origins: vec![],
        db_id: "dev-read".into(),
        test_environment: false,
    }
}

fn file(envs: Vec<Environment>) -> EnvFile {
    EnvFile { active: envs[0].id.clone(), environments: envs }
}

fn input(name: &str) -> EnvInput {
    EnvInput {
        id: String::new(),
        name: name.into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: "qa-read".into(),
        test_environment: false,
    }
}

/// A root with Default already made, plus a second environment `QA`.
fn two_envs(root: &Path) -> (String, String) {
    let first = load_or_init(root, Some("dev-read")).unwrap().active;
    let after = save_env(root, env("", "QA", ""), &known()).unwrap();
    let qa = after.environments.iter().find(|e| e.name == "QA").unwrap().id.clone();
    (first, qa)
}

#[test]
fn first_load_creates_default_and_moves_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let accounts = r#"[{"key":"hr.sup","label":"Supervisor","username":"sup1","password":"pw-Zq9"}]"#;
    std::fs::write(root.join("accounts.json"), accounts).unwrap();

    let f = load_or_init(root, Some("qa-read")).unwrap();
    assert_eq!(f.environments.len(), 1);
    let d = &f.environments[0];
    assert_eq!(d.name, "Default");
    assert_eq!(d.db_id, "qa-read");
    assert_eq!(d.start_url, "");
    assert!(!d.test_environment);
    assert_eq!(f.active, d.id);
    assert!(d.id.starts_with("env-") && d.id.len() == 12, "{}", d.id);
    assert!(d.id[4..].chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)), "{}", d.id);

    let copied = std::fs::read_to_string(root.join("accounts").join(format!("{}.json", d.id))).unwrap();
    assert!(copied.contains("sup1"), "{copied}");

    // The second call reads the file - it does not make another Default.
    let again = load_or_init(root, Some("dev-read")).unwrap();
    assert_eq!(again, f);
    assert_eq!(active_id(root).unwrap(), d.id);
    assert_eq!(active(root).unwrap().name, "Default");
}

#[test]
fn first_load_without_a_chosen_database_uses_the_first_preset() {
    let dir = tempfile::tempdir().unwrap();
    let f = load_or_init(dir.path(), None).unwrap();
    assert_eq!(f.environments[0].db_id, v2_lib::db_defaults::DB_PRESETS[0].id);
    // Nothing to copy: no accounts file appears from nowhere.
    assert!(!dir.path().join("accounts").exists());
}

#[test]
fn two_environments_may_share_an_address() {
    let f = file(vec![
        env("env-00000001", "Local dev", "https://hr.example.com"),
        env("env-00000002", "Hosted dev", "https://hr.example.com"),
    ]);
    assert_eq!(validate(&f, &known()), Ok(()));
}

#[test]
fn names_are_unique_ignoring_case() {
    let f = file(vec![env("env-00000001", "Dev", ""), env("env-00000002", "dev", "")]);
    let err = validate(&f, &known()).unwrap_err();
    assert!(err.contains("already"), "{err}");

    let dir = tempfile::tempdir().unwrap();
    load_or_init(dir.path(), None).unwrap();
    let err = save_env(dir.path(), env("", " default ", ""), &known()).unwrap_err();
    assert!(err.contains("already"), "{err}");
}

#[test]
fn a_name_is_required() {
    let f = file(vec![env("env-00000001", "  ", "")]);
    assert!(validate(&f, &known()).is_err());
}

#[test]
fn bad_address_and_origins_are_refused() {
    let f = file(vec![env("env-00000001", "Dev", "ftp://x")]);
    assert!(validate(&f, &known()).is_err());

    let mut e = env("env-00000001", "Dev", "https://x");
    e.allowed_origins = vec!["https://x/path".into()];
    let err = validate(&file(vec![e.clone()]), &known()).unwrap_err();
    assert!(err.contains("https://x/path"), "{err}");

    e.allowed_origins = vec!["https://login.x".into(), "https://x:8443/".into()];
    assert_eq!(validate(&file(vec![e]), &known()), Ok(()));
}

#[test]
fn unknown_database_is_refused() {
    let mut e = env("env-00000001", "Dev", "");
    e.db_id = "gone".into();
    let err = validate(&file(vec![e]), &known()).unwrap_err();
    assert!(err.contains("database"), "{err}");
}

#[test]
fn saving_with_an_id_replaces_and_without_one_adds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    assert_ne!(first, qa);

    let mut edited = active(root).unwrap();
    edited.name = "Local".into();
    edited.start_url = "https://hr.example.com".into();
    let f = save_env(root, edited, &known()).unwrap();
    assert_eq!(f.environments.len(), 2);
    assert_eq!(active(root).unwrap().name, "Local");

    let mut stale = env("env-deadbeef", "Ghost", "");
    stale.db_id = "qa-read".into();
    assert!(save_env(root, stale, &known()).is_err(), "an id that is not in the file is not an add");
}

#[test]
fn active_and_last_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let only = load_or_init(root, None).unwrap().active;
    let err = remove_env(root, &only).unwrap_err();
    assert!(err.contains("last"), "{err}");

    let (first, qa) = two_envs(root);
    let err = remove_env(root, &first).unwrap_err();
    assert!(err.contains("active"), "{err}");

    let f = remove_env(root, &qa).unwrap();
    assert_eq!(f.environments.len(), 1);
    assert_eq!(f.active, first);
}

#[test]
fn removing_deletes_only_that_environments_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    for id in [&first, &qa] {
        std::fs::create_dir_all(root.join("accounts")).unwrap();
        std::fs::write(root.join("accounts").join(format!("{id}.json")), "[]").unwrap();
        std::fs::create_dir_all(root.join("sessions").join(id)).unwrap();
        std::fs::write(root.join("sessions").join(id).join("hr.sup.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("proposals")).unwrap();
        std::fs::write(root.join("proposals").join(format!("{id}.json")), "[]").unwrap();
    }
    remove_env(root, &qa).unwrap();

    assert!(!root.join("accounts").join(format!("{qa}.json")).exists());
    assert!(!root.join("sessions").join(&qa).exists());
    assert!(!root.join("proposals").join(format!("{qa}.json")).exists());
    assert!(root.join("accounts").join(format!("{first}.json")).exists());
    assert!(root.join("sessions").join(&first).join("hr.sup.json").exists());
    assert!(root.join("proposals").join(format!("{first}.json")).exists());
}

#[test]
fn switching_changes_the_active_environment() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (_, qa) = two_envs(root);
    let f = set_active(root, &qa).unwrap();
    assert_eq!(f.active, qa);
    assert_eq!(active(root).unwrap().name, "QA");
    assert!(set_active(root, "env-deadbeef").is_err());
}

#[test]
fn a_switch_works_when_the_database_is_not_known_any_more() {
    // Review focus 2 is the UI's, but the store must not stand in its way.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (_, qa) = two_envs(root);
    let narrower: Vec<String> = vec!["dev-read".into()];
    assert!(set_active(root, &qa).is_ok());
    // And editing a different environment is not blocked by QA's database.
    let mut first = load_or_init(root, None).unwrap().environments[0].clone();
    first.name = "Renamed".into();
    assert!(save_env(root, first, &narrower).is_ok());
}

#[test]
fn password_target_is_keyed_by_id() {
    assert_eq!(password_target("env-0a1b2c3d"), "env-default-password:env-0a1b2c3d");
}

#[test]
fn default_password_never_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let view = list_view(root, &store, Some("qa-read")).unwrap();
    let id = view.active.clone();
    assert!(!view.environments[0].has_default_password);

    set_default_password_with(root, &store, &id, "Pw-Zq9-secret").unwrap();
    assert_eq!(store.get(&password_target(&id)).unwrap().as_deref(), Some("Pw-Zq9-secret"));

    let on_disk = std::fs::read_to_string(root.join("environments.json")).unwrap();
    assert!(!on_disk.contains("Pw-Zq9-secret"), "{on_disk}");

    let view = list_view(root, &store, None).unwrap();
    assert!(view.environments[0].has_default_password);
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("Pw-Zq9-secret"), "{json}");
    assert!(!json.contains("\"password\""), "{json}");

    assert!(set_default_password_with(root, &store, &id, "").is_err(), "an empty password is not a password");
    assert!(set_default_password_with(root, &store, "env-deadbeef", "x").is_err());

    clear_default_password_with(root, &store, &id).unwrap();
    assert!(!list_view(root, &store, None).unwrap().environments[0].has_default_password);
}

#[test]
fn commands_add_edit_and_remove_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    list_view(root, &store, None).unwrap();

    let view = save_with(root, &store, input("QA")).unwrap();
    let qa = view.environments.iter().find(|e| e.name == "QA").unwrap().id.clone();
    assert!(!qa.is_empty());

    let err = save_with(root, &store, EnvInput { db_id: "gone".into(), ..input("Other") }).unwrap_err();
    assert!(err.contains("database"), "{err}");

    set_default_password_with(root, &store, &qa, "pw").unwrap();
    let view = remove_with(root, &store, &qa).unwrap();
    assert_eq!(view.environments.len(), 1);
    assert_eq!(store.get(&password_target(&qa)).unwrap(), None, "a removed environment's password goes with it");
}

#[tokio::test]
async fn switching_is_refused_while_recording_or_running() {
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);

    let rec = RecorderClaim::claim().expect("nothing is recording");
    let err = set_active_with(root, &store, &qa).await.unwrap_err();
    assert_eq!(err, SWITCH_BUSY);
    assert!(err.contains("recorded or run"), "{err}");
    drop(rec);

    let run = OneAtATime::claim().expect("nothing is running");
    assert_eq!(set_active_with(root, &store, &qa).await.unwrap_err(), SWITCH_BUSY);
    drop(run);

    assert_eq!(active_id(root).unwrap(), first, "a refused switch changes nothing");
    // Nothing recording, running or open: the switch goes through.
    let view = set_active_with(root, &store, &qa).await.unwrap();
    assert_eq!(view.active, qa);
}

#[tokio::test]
async fn switching_is_refused_while_an_api_template_runs() {
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = MemoryStore::default();
    let (first, qa) = two_envs(root);

    let run = v2_lib::api_templates::runner::claim().expect("no template is running");
    assert!(v2_lib::api_templates::runner::is_running());
    let err = set_active_with(root, &store, &qa).await.unwrap_err();
    assert_eq!(err, SWITCH_TEMPLATE_RUNNING);
    assert!(err.contains("API template"), "{err}");
    assert_eq!(active_id(root).unwrap(), first, "a refused switch changes nothing");
    drop(run);

    assert!(!v2_lib::api_templates::runner::is_running(), "dropping the claim frees the slot");
    assert_eq!(set_active_with(root, &store, &qa).await.unwrap().active, qa);
    // The switch holds the run slot only while it writes: a template can
    // start straight after it.
    assert!(!v2_lib::api_templates::runner::is_running(), "a switch gives the slot back");
    assert!(v2_lib::api_templates::runner::claim().is_some());
}

/// A real supervised browser cannot be opened in a test, so the decision is
/// driven directly: the command passes in whether the slot holds a session
/// (and `switching_is_refused_while_recording_or_running` shows an empty
/// slot lets the switch through).
#[test]
fn switching_is_refused_while_the_supervised_browser_is_open() {
    let _claims = crate::serial::autorun();
    let _slot = crate::serial::api_template_run();
    let err = refuse_switch(true).unwrap_err();
    assert_eq!(err, SWITCH_SUPERVISED_OPEN);
    assert!(err.contains("supervised browser") && err.contains("close it first"), "{err}");
    assert_eq!(refuse_switch(false), Ok(()));
}

#[test]
fn a_hand_edited_id_that_could_escape_the_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let f = r#"{"active":"../x","environments":[{"id":"../x","name":"D","start_url":"","allowed_origins":[],"db_id":"dev-read"}]}"#;
    std::fs::write(root.join("environments.json"), f).unwrap();
    assert!(load_or_init(root, None).is_err());
    assert!(active_id(root).is_err());
}

#[test]
fn an_unreadable_file_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("environments.json"), "{ not json").unwrap();
    assert!(load_or_init(root, None).is_err());
    assert_eq!(std::fs::read_to_string(root.join("environments.json")).unwrap(), "{ not json");
}

fn acct(key: &str, user: &str) -> v2_lib::autorun::accounts::Account {
    v2_lib::autorun::accounts::Account {
        key: key.into(),
        label: key.into(),
        username: user.into(),
        password: "pw-Zq9".into(),
    }
}

fn session(at: u64) -> v2_lib::browser::session::SavedSession {
    v2_lib::browser::session::SavedSession { saved_at_ms: at, cookies: vec![], local_storage: vec![] }
}

#[test]
fn accounts_follow_the_active_environment() {
    use v2_lib::autorun::accounts::{load_accounts, save_accounts};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (a, b) = two_envs(root);

    save_accounts(root, &[acct("hr.sup", "sup-a")]).unwrap();
    set_active(root, &b).unwrap();
    assert!(load_accounts(root).unwrap().is_empty());

    save_accounts(root, &[acct("hr.emp", "emp-b")]).unwrap();
    assert_eq!(load_accounts(root).unwrap(), vec![acct("hr.emp", "emp-b")]);

    set_active(root, &a).unwrap();
    assert_eq!(load_accounts(root).unwrap(), vec![acct("hr.sup", "sup-a")]);
    assert!(root.join("accounts").join(format!("{a}.json")).is_file());
    assert!(root.join("accounts").join(format!("{b}.json")).is_file());
}

/// Two environments on one address must never trade a signed-in session.
#[test]
fn same_address_different_sessions() {
    use v2_lib::autorun::sessions::{forget_session, load_fresh_session, save_session};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (a, b) = two_envs(root);
    for id in [&a, &b] {
        let mut e = list(root).into_iter().find(|e| &e.id == id).unwrap();
        e.start_url = "https://hr.example.com".into();
        save_env(root, e, &known()).unwrap();
    }

    save_session(root, "hr.sup", &session(1_000)).unwrap();
    assert!(root.join("sessions").join(&a).join("hr.sup.json").is_file());

    set_active(root, &b).unwrap();
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_none());

    set_active(root, &a).unwrap();
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_some());

    // Forgetting in B leaves A's session alone.
    set_active(root, &b).unwrap();
    forget_session(root, "hr.sup");
    set_active(root, &a).unwrap();
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_some());
}

fn list(root: &Path) -> Vec<Environment> {
    load_or_init(root, None).unwrap().environments
}

#[test]
fn legacy_session_files_are_not_read() {
    use v2_lib::autorun::sessions::load_fresh_session;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    load_or_init(root, None).unwrap();
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    std::fs::write(
        root.join("sessions").join("hr.sup.json"),
        serde_json::to_string(&session(1_000)).unwrap(),
    )
    .unwrap();
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_none());
}

/// Changing an account's login drops its session in THIS environment only.
#[test]
fn a_changed_login_drops_the_session_of_the_active_environment_only() {
    use v2_lib::autorun::accounts::save_accounts;
    use v2_lib::autorun::sessions::{load_fresh_session, save_session};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (a, b) = two_envs(root);
    save_accounts(root, &[acct("hr.sup", "sup-a")]).unwrap();
    save_session(root, "hr.sup", &session(1_000)).unwrap();
    set_active(root, &b).unwrap();
    save_accounts(root, &[acct("hr.sup", "sup-b")]).unwrap();
    save_session(root, "hr.sup", &session(1_000)).unwrap();

    let dropped = save_accounts(root, &[acct("hr.sup", "sup-b2")]).unwrap();
    assert_eq!(dropped, vec!["hr.sup".to_string()]);
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_none());
    set_active(root, &a).unwrap();
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_some());
}

#[test]
fn an_unreadable_environments_file_means_no_accounts_and_no_session() {
    use v2_lib::autorun::accounts::{load_accounts, save_accounts};
    use v2_lib::autorun::sessions::{load_fresh_session, save_session};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("environments.json"), "{ not json").unwrap();
    assert!(load_accounts(root).is_err());
    assert!(save_accounts(root, &[acct("hr.sup", "u")]).is_err());
    assert!(save_session(root, "hr.sup", &session(1_000)).is_err());
    assert!(load_fresh_session(root, "hr.sup", 480, 2_000).is_none());
    assert!(!root.join("accounts").exists() && !root.join("sessions").exists());
}

// ---- The effective recipe: where sign-in and navigation go ----

fn signin_recipe(start: &str, allowed: &[&str]) -> v2_lib::autorun::recipe::SignInRecipe {
    let mut r = crate::common::recipe();
    r.start_url = start.into();
    r.allowed_origins = allowed.iter().map(|s| s.to_string()).collect();
    r
}

#[test]
fn empty_address_keeps_the_recipe() {
    use v2_lib::autorun::recipe::effective_recipe;
    let r = signin_recipe("https://a.example/login", &["https://sso.a.example"]);
    let e = env("env-0000000a", "Default", "");
    assert_eq!(effective_recipe(&r, &e), r, "an empty address means the recipe's own");
    let mut with_sites = env("env-0000000a", "Default", "");
    with_sites.allowed_origins = vec!["https://sso.b.example".into()];
    assert_eq!(effective_recipe(&r, &with_sites), r, "allowed sites without an address change nothing");
}

#[test]
fn address_replaces_address_and_allowed_sites() {
    use v2_lib::autorun::recipe::effective_recipe;
    let r = signin_recipe("https://a.example/login", &["https://sso.a.example"]);
    let mut e = env("env-0000000b", "QA", "https://b.example/login");
    e.allowed_origins = vec!["https://sso.b.example".into()];
    let out = effective_recipe(&r, &e);
    assert_eq!(out.start_url, "https://b.example/login");
    assert_eq!(out.allowed_origins, vec!["https://sso.b.example".to_string()]);
    assert_eq!(out.origins(), vec!["https://b.example".to_string(), "https://sso.b.example".to_string()]);
    // Everything else is the recipe's.
    assert_eq!(out.steps, r.steps);
    assert_eq!(out.after_sign_in, r.after_sign_in);
    assert_eq!(out.signed_in, r.signed_in);
    assert_eq!(out.session_minutes, r.session_minutes);
}

#[test]
fn prepare_signs_in_at_the_environment_address() {
    use v2_lib::autorun::accounts::save_accounts;
    use v2_lib::autorun::recipe::{load_effective_recipe, load_recipe, save_recipe};
    use v2_lib::autorun::signin::prepare;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (first, qa) = two_envs(root);
    let mut e = list(root).into_iter().find(|e| e.id == qa).unwrap();
    e.start_url = "https://b".into();
    save_env(root, e, &known()).unwrap();
    save_recipe(root, "Acme", "Web", &signin_recipe("https://a", &[])).unwrap();
    save_accounts(root, &[acct("hr.sup", "sup-a")]).unwrap();

    // Default has no address: the recipe's own.
    assert_eq!(prepare(root, "Acme", "Web", "hr.sup").unwrap().0.start_url, "https://a");

    set_active(root, &qa).unwrap();
    save_accounts(root, &[acct("hr.sup", "sup-b")]).unwrap();
    let (r, who) = prepare(root, "Acme", "Web", "hr.sup").unwrap();
    assert_eq!(r.start_url, "https://b");
    assert_eq!(who.username, "sup-b");
    assert_eq!(load_effective_recipe(root, "Acme", "Web").unwrap().unwrap().start_url, "https://b");
    assert_eq!(load_recipe(root, "Acme", "Web").unwrap().unwrap().start_url, "https://a", "the saved recipe is untouched");
    assert_eq!(load_effective_recipe(root, "Acme", "Other").unwrap(), None, "no recipe is still no recipe");

    set_active(root, &first).unwrap();
    assert_eq!(prepare(root, "Acme", "Web", "hr.sup").unwrap().0.start_url, "https://a");
}

/// The recipe's own address may be read directly only where the recipe is
/// edited; everything that signs in or navigates goes through
/// `load_effective_recipe`, or the active environment's address is ignored.
#[test]
fn only_the_helper_reads_the_recipe_address() {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    walk(&src, &mut files);
    assert!(files.len() > 20, "the scan found the source tree");
    let allowed = ["autorun/recipe.rs", "commands/autorun.rs", "commands/autorun_record_signin.rs"];
    let mut bad = vec![];
    for f in files {
        let rel = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        if allowed.contains(&rel.as_str()) {
            continue;
        }
        if std::fs::read_to_string(&f).unwrap().contains("load_recipe(") {
            bad.push(rel);
        }
    }
    assert!(bad.is_empty(), "these read the raw sign-in recipe - use recipe::load_effective_recipe: {bad:?}");
}

#[test]
fn old_runs_and_proofs_still_load() {
    use serde_json::json;
    use v2_lib::api_templates::ApiTemplate;
    use v2_lib::autorun::LocalRun;

    let run: LocalRun = serde_json::from_value(json!({
        "id": "run-1", "pbi_id": 7, "started_at": "1", "cases": [], "mode": "unattended"
    }))
    .expect("a run saved before environments still loads");
    assert_eq!(run.environment, None);
    assert!(serde_json::to_value(&run).unwrap().get("environment").is_none());
    let named = LocalRun { environment: Some("QA".into()), ..run };
    assert_eq!(serde_json::to_value(&named).unwrap()["environment"], "QA");

    let mut t = crate::common::template_on_stage("pms-rules", "Rules", "rules");
    t["proven"] = json!({ "at": "2026-09-01 09:00:00", "origin": "https://hr.example.internal", "account": "admin", "outputs": {} });
    let t: ApiTemplate = serde_json::from_value(t).expect("a proof saved before environments still loads");
    let proven = t.proven.clone().unwrap();
    assert_eq!(proven.environment, None);
    let back = serde_json::to_value(&t).unwrap();
    assert!(back["proven"].get("environment").is_none(), "{back}");
    let mut named = t;
    named.proven.as_mut().unwrap().environment = Some("QA".into());
    assert_eq!(serde_json::to_value(&named).unwrap()["proven"]["environment"], "QA");
}

/// A supervised run reaches disk through `auto_run_save_run`: the first save
/// of a new run is stamped with the active environment, and a run already
/// on disk - one saved before environments, say - is never re-stamped.
#[test]
fn a_new_run_records_the_active_environment() {
    use v2_lib::autorun::store::{load_run, save_run};
    use v2_lib::autorun::LocalRun;
    use v2_lib::commands::autorun::save_run_at;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (_, qa) = two_envs(root);
    set_active(root, &qa).unwrap();
    let run = |id: &str| LocalRun {
        id: id.into(),
        pbi_id: 7,
        started_at: "1".into(),
        cases: vec![],
        mode: String::new(),
        published: None,
        environment: None,
    };

    save_run_at(root, run("run-new")).unwrap();
    assert_eq!(load_run(root, "run-new").unwrap().unwrap().environment.as_deref(), Some("QA"));

    save_run(root, &run("run-old")).unwrap();
    save_run_at(root, run("run-old")).unwrap();
    assert_eq!(load_run(root, "run-old").unwrap().unwrap().environment, None, "an existing run is not re-stamped");

    assert!(save_run_at(root, run("../x")).is_err(), "the id is still checked");
}

// ---- Proposed accounts, and what the assistant may read ----

mod accounts_for_the_assistant {
    use super::{acct, known, list};
    use serde_json::{json, Value};
    use std::path::Path;
    use v2_lib::ai_bridge::{route, BridgeContext};
    use v2_lib::autorun::accounts::{load_accounts, save_accounts, Account};
    use v2_lib::commands::environments::{
        add_proposals_with, dismiss_proposals_with, proposals_with, set_default_password_with, AccountInput,
    };
    use v2_lib::db::credentials::MemoryStore;
    use v2_lib::environments::{
        active_id, load_or_init, load_proposals, proposals_path_for, save_env, save_proposals, ProposedAccount,
    };

    /// A root with Default made, set as the process-wide root the bridge
    /// reads. The caller holds `serial::autorun()`.
    fn bridge_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        load_or_init(dir.path(), Some("dev-read")).unwrap();
        v2_lib::autorun::store::set_root(dir.path().to_path_buf());
        dir
    }

    async fn call(method: &str, path: &str, body: &str) -> (u16, String) {
        route(&BridgeContext::default(), None, method, path, body, "1.0.0").await
    }

    fn proposal(key: &str, user: &str) -> Value {
        json!({ "key": key, "label": format!("{key} label"), "username": user, "role": "Supervisor" })
    }

    fn mark_test_environment(root: &Path) {
        let mut e = list(root).into_iter().next().unwrap();
        e.test_environment = true;
        save_env(root, e, &known()).unwrap();
    }

    fn pick(key: &str, user: &str, password: &str) -> AccountInput {
        AccountInput { key: key.into(), label: key.into(), username: user.into(), password: password.into() }
    }

    fn seed_proposals(root: &Path, keys: &[&str]) {
        let list: Vec<ProposedAccount> = keys
            .iter()
            .map(|k| ProposedAccount { key: k.to_string(), label: k.to_string(), username: format!("{k}-user"), role: None })
            .collect();
        save_proposals(root, &active_id(root).unwrap(), &list).unwrap();
    }

    #[tokio::test]
    async fn proposals_replace_and_validate() {
        let _root = crate::serial::autorun();
        let dir = bridge_root();
        let root = dir.path();
        let env_id = active_id(root).unwrap();

        // A key that could not be an account key is refused, and nothing is kept.
        let (status, out) =
            call("POST", "/accounts-propose", &json!({ "accounts": [proposal("Bad Key", "u1")] }).to_string()).await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("Bad Key"), "{out}");
        assert!(!proposals_path_for(root, &env_id).exists());

        // A username is required.
        let (status, out) =
            call("POST", "/accounts-propose", &json!({ "accounts": [proposal("hr.sup", "  ")] }).to_string()).await;
        assert_eq!(status, 400, "{out}");

        // A password is never accepted in a proposal.
        let mut with_pw = proposal("hr.sup", "sup1");
        with_pw["password"] = json!("pw-Zq9");
        let (status, out) = call("POST", "/accounts-propose", &json!({ "accounts": [with_pw] }).to_string()).await;
        assert_eq!(status, 400, "{out}");
        assert!(!proposals_path_for(root, &env_id).exists());

        // 101 is one too many; 100 is fine.
        let many: Vec<Value> = (0..101).map(|i| proposal(&format!("u{i}"), &format!("user{i}"))).collect();
        let (status, out) = call("POST", "/accounts-propose", &json!({ "accounts": many }).to_string()).await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("100"), "{out}");
        let hundred: Vec<Value> = (0..100).map(|i| proposal(&format!("u{i}"), &format!("user{i}"))).collect();
        let (status, out) = call("POST", "/accounts-propose", &json!({ "accounts": hundred }).to_string()).await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(load_proposals(root, &env_id).unwrap().len(), 100);

        // Each call REPLACES the last.
        let (status, out) = call(
            "POST",
            "/accounts-propose",
            &json!({ "accounts": [proposal("hr.sup", "sup1"), proposal("hr.emp", "emp1")] }).to_string(),
        )
        .await;
        assert_eq!(status, 200, "{out}");
        let (status, out) =
            call("POST", "/accounts-propose", &json!({ "accounts": [proposal("hr.admin", "adm1")] }).to_string()).await;
        assert_eq!(status, 200, "{out}");
        let kept = proposals_with(root).unwrap();
        assert_eq!(
            kept,
            vec![ProposedAccount {
                key: "hr.admin".into(),
                label: "hr.admin label".into(),
                username: "adm1".into(),
                role: Some("Supervisor".into()),
            }]
        );
        assert!(proposals_path_for(root, &env_id).is_file(), "stored under the environment's id");

        dismiss_proposals_with(root).unwrap();
        assert!(proposals_with(root).unwrap().is_empty());
        assert!(!proposals_path_for(root, &env_id).exists());
    }

    #[tokio::test]
    async fn get_accounts_hides_passwords_outside_test_environments() {
        let _root = crate::serial::autorun();
        let dir = bridge_root();
        save_accounts(dir.path(), &[acct("hr.sup", "sup1")]).unwrap();

        let (status, out) = call("GET", "/accounts", "").await;
        assert_eq!(status, 200, "{out}");
        assert!(!out.contains("pw-Zq9"), "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["environment"], "Default");
        assert_eq!(v["test_environment"], false);
        assert_eq!(v["accounts"][0]["key"], "hr.sup");
        assert_eq!(v["accounts"][0]["label"], "hr.sup");
        assert_eq!(v["accounts"][0]["username"], "sup1");
        assert!(v["accounts"][0].get("password").is_none(), "{out}");
        assert!(v["note"].as_str().unwrap().contains("not marked as a test environment"), "{out}");
    }

    #[tokio::test]
    async fn get_accounts_shows_them_in_a_test_environment() {
        let _root = crate::serial::autorun();
        let dir = bridge_root();
        save_accounts(dir.path(), &[acct("hr.sup", "sup1")]).unwrap();
        mark_test_environment(dir.path());

        let (status, out) = call("GET", "/accounts", "").await;
        assert_eq!(status, 200, "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["test_environment"], true);
        assert_eq!(v["accounts"][0]["password"], "pw-Zq9");
        assert!(v.get("note").is_none(), "{out}");
    }

    #[tokio::test]
    async fn get_accounts_is_not_logged() {
        let _root = crate::serial::autorun();
        let _log = crate::serial::log_tail();
        let dir = bridge_root();
        save_accounts(dir.path(), &[acct("hr.sup", "sup1")]).unwrap();
        mark_test_environment(dir.path());

        let (status, out) = call("GET", "/accounts", "").await;
        assert_eq!(status, 200, "{out}");
        assert!(out.contains("pw-Zq9"), "the password was returned: {out}");
        let tail = v2_lib::applog::recent(500);
        assert!(tail.iter().any(|l| l.message.contains("read the 1 account")), "the read is on record: {tail:?}");
        assert!(tail.iter().all(|l| !l.message.contains("pw-Zq9")), "{tail:?}");
    }

    /// The account routes are offered exactly where Auto Run is.
    #[test]
    fn the_account_routes_are_gated_like_auto_run() {
        use v2_lib::ai_bridge::{autorun_guard_for, smells_like_a_write};
        for path in ["/accounts", "/accounts-propose"] {
            let (status, body) = autorun_guard_for(path, false).unwrap_or_else(|| panic!("{path} was not refused"));
            assert_eq!((status, body.as_str()), (404, "not available in this build"), "{path}");
            assert!(autorun_guard_for(path, true).is_none(), "{path}");
            assert!(!smells_like_a_write("POST", path) && !smells_like_a_write("GET", path), "{path}");
        }
    }

    fn keys(accounts: &[Account]) -> Vec<&str> {
        accounts.iter().map(|a| a.key.as_str()).collect()
    }

    #[test]
    fn adding_an_existing_key_needs_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        load_or_init(root, Some("dev-read")).unwrap();
        save_accounts(root, &[acct("hr.sup", "sup-old")]).unwrap();
        seed_proposals(root, &["hr.sup", "hr.emp", "hr.admin"]);
        let store = MemoryStore::default();

        let picks = vec![pick("hr.sup", "sup-new", "pw-new"), pick("hr.emp", "emp1", "pw-emp")];
        let confirm = add_proposals_with(root, &store, picks.clone(), vec![]).unwrap();
        assert_eq!(confirm, vec!["hr.sup".to_string()]);
        let now = load_accounts(root).unwrap();
        assert_eq!(keys(&now), ["hr.sup", "hr.emp"]);
        assert_eq!(now[0], acct("hr.sup", "sup-old"), "not overwritten without confirmation");
        assert_eq!(now[1].password, "pw-emp");
        // What was added leaves the proposals; the rest stays to be decided.
        let left: Vec<String> = proposals_with(root).unwrap().into_iter().map(|p| p.key).collect();
        assert_eq!(left, ["hr.sup", "hr.admin"]);

        // Confirmed, it replaces in place.
        let confirm = add_proposals_with(root, &store, vec![picks[0].clone()], vec!["hr.sup".into()]).unwrap();
        assert!(confirm.is_empty(), "{confirm:?}");
        let now = load_accounts(root).unwrap();
        assert_eq!(keys(&now), ["hr.sup", "hr.emp"]);
        assert_eq!((now[0].username.as_str(), now[0].password.as_str()), ("sup-new", "pw-new"));
        let left: Vec<String> = proposals_with(root).unwrap().into_iter().map(|p| p.key).collect();
        assert_eq!(left, ["hr.admin"]);
    }

    #[test]
    fn empty_password_uses_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        load_or_init(root, Some("dev-read")).unwrap();
        let store = MemoryStore::default();
        let both = || vec![pick("hr.ok", "ok1", "typed"), pick("hr.sup", "sup1", "")];

        // No default yet: refused for that key, and nothing is written.
        let err = add_proposals_with(root, &store, both(), vec![]).unwrap_err();
        assert!(err.contains("hr.sup") && err.contains("no default password set - type one"), "{err}");
        assert!(load_accounts(root).unwrap().is_empty());

        set_default_password_with(root, &store, &active_id(root).unwrap(), "Default-Pw1").unwrap();
        let confirm = add_proposals_with(root, &store, both(), vec![]).unwrap();
        assert!(confirm.is_empty());
        let now = load_accounts(root).unwrap();
        assert_eq!(now.iter().find(|a| a.key == "hr.sup").unwrap().password, "Default-Pw1");
        assert_eq!(now.iter().find(|a| a.key == "hr.ok").unwrap().password, "typed", "a typed one wins");

        // A pick that is not a usable account is refused as a whole.
        assert!(add_proposals_with(root, &store, vec![pick("Bad Key", "u", "p")], vec![]).is_err());
        assert!(add_proposals_with(root, &store, vec![pick("hr.x", " ", "p")], vec![]).is_err());
        assert_eq!(load_accounts(root).unwrap().len(), 2);
    }
}
