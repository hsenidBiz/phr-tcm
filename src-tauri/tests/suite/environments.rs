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
