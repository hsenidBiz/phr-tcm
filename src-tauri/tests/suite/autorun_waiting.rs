//! What is waiting on the person in a project: the assistant's proposed
//! logins and sign-in. Never a divergence (those are ruled on in each case's
//! card), a login, a password or an address.

use v2_lib::autorun::divergences::{self, Divergence, DivergenceKind, DivergenceStatus};
use v2_lib::autorun::signin_proposal::{self, StoredSignInProposal};
use v2_lib::commands::autorun_waiting::{waiting_items_with, WaitingTarget};
use v2_lib::environments::{active, load_or_init, save_env_with, save_proposals, StoredProposal};

fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut e = load_or_init(dir.path(), Some("dev-read")).unwrap().environments[0].clone();
    e.name = "Staging".to_string();
    save_env_with(dir.path(), e, || vec!["dev-read".to_string()]).unwrap();
    dir
}

fn account(key: &str, proposed_ms: Option<u64>) -> StoredProposal {
    StoredProposal {
        key: key.into(),
        label: key.into(),
        username: format!("{key}-secret-login"),
        role: None,
        password: Some("hunter2".into()),
        no_password: None,
        marks: vec![],
        proposed_ms,
    }
}

fn divergence(id: &str, case_id: i32, status: DivergenceStatus, at: &str, pbi: Option<i32>) -> Divergence {
    Divergence {
        id: id.into(),
        case_id,
        step: 2,
        case_says: "four".into(),
        app_shows: "six".into(),
        kind: DivergenceKind::Data,
        reported_at: at.into(),
        status,
        ruling: None,
        ruled_at: None,
        case_fp: None,
        step_action: None,
        ruled_by: None,
        said: None,
        note: None,
        proposal: None,
        pbi,
    }
}

fn seed(dir: &std::path::Path) -> String {
    let env = active(dir).unwrap();
    save_proposals(dir, &env.id, &[account("hr.sup", Some(1000)), account("clerk", None)]).unwrap();
    let recipe = serde_json::from_value(serde_json::json!({
        "start_url": "https://x.example.test",
        "steps": [{ "kind": "navigate", "url": "https://x.example.test/login" }],
        "signed_in": { "css": "#home" }
    }))
    .unwrap();
    signin_proposal::save(
        dir,
        &env.id,
        "acme",
        "Web",
        &StoredSignInProposal {
            recipe,
            account: "sup".into(),
            proposed_ms: 2000.0,
            summary: "s".into(),
            saved_hash: String::new(),
            changes: vec![],
        },
    )
    .unwrap();
    divergences::update(dir, "acme", "Web", |l| {
        l.push(divergence("d1", 5, DivergenceStatus::Open, "3000", Some(42)));
        l.push(divergence("d2", 6, DivergenceStatus::AwaitingRepair, "4000", None));
        l.push(divergence("d3", 7, DivergenceStatus::Ruled, "5000", None));
        Ok(())
    })
    .unwrap();
    divergences::update(dir, "acme", "Other", |l| {
        l.push(divergence("d1", 9, DivergenceStatus::Open, "9000", None));
        Ok(())
    })
    .unwrap();
    env.id
}

#[test]
fn everything_waiting_comes_newest_first() {
    let dir = root();
    let env_id = seed(dir.path());
    let w = waiting_items_with(dir.path(), "acme", "Web").unwrap();
    assert_eq!(w.env_id, env_id);
    assert_eq!(
        w.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
        vec!["Sign-in for Staging", "hr.sup", "clerk"]
    );
    assert_eq!(w.items[0].target, WaitingTarget::SignIn);
    assert_eq!(w.items[1].target, WaitingTarget::Accounts);
    assert_eq!(w.items[2].proposed_ms, 0.0);
}

#[test]
fn open_divergences_are_not_waiting_items() {
    let dir = root();
    seed(dir.path());
    // The seed stored an open divergence; it is still stored, just not waiting.
    assert_eq!(divergences::load(dir.path(), "acme", "Web").unwrap().len(), 3);
    let w = waiting_items_with(dir.path(), "acme", "Web").unwrap();
    assert!(w.items.iter().all(|i| !i.title.contains("Case ")), "{:?}", w.items);
}

#[test]
fn nothing_waiting_is_an_empty_list() {
    let dir = root();
    let w = waiting_items_with(dir.path(), "acme", "Web").unwrap();
    assert!(w.items.is_empty());
}

#[test]
fn no_item_carries_a_login() {
    let dir = root();
    seed(dir.path());
    let w = waiting_items_with(dir.path(), "acme", "Web").unwrap();
    let json = serde_json::to_string(&w).unwrap();
    assert!(!json.contains("secret-login"), "{json}");
    assert!(!json.contains("password"), "{json}");
    assert!(!json.contains("hunter2"), "{json}");
}
