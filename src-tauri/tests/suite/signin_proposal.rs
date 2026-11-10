//! A proposed sign-in: the merge rule, the one-per-environment store, and
//! approve / reject.

use serde_json::json;
use v2_lib::autorun::recipe::{load_recipe, save_recipe, SignInRecipe};
use v2_lib::autorun::roots::shared_of;
use v2_lib::autorun::signin_proposal::{
    changed_parts, clear, load, merge, proposal_path, save, saved_hash, summary, view, StoredSignInProposal,
    PROPOSAL_STALE, RESET_SWITCH_NOT_TEST,
};
use v2_lib::commands::autorun_signin_proposal::{approve_with, proposal_view_with, reject_with};
use v2_lib::environments::{active, load_or_init, save_env_with, set_reset_expired_passwords};

const ORG: &str = "acme";
const PROJECT: &str = "portal";

fn recipe(value: serde_json::Value) -> SignInRecipe {
    serde_json::from_value(value).unwrap()
}

fn basic(start: &str, button: &str) -> SignInRecipe {
    recipe(json!({
        "start_url": start,
        "steps": [
            { "kind": "navigate", "url": format!("{start}/login") },
            { "kind": "fill", "selector": { "css": "#user" }, "value": "{{username}}" },
            { "kind": "click", "selector": { "css": button } }
        ],
        "signed_in": { "css": "#home" }
    }))
}

fn with_reset(mut r: SignInRecipe) -> SignInRecipe {
    r.password_reset = Some(
        serde_json::from_value(json!({ "path_contains": "/security/PasswordReset", "field": { "css": "#old" } })).unwrap(),
    );
    r
}

/// A data folder whose active environment has the given address, test flag
/// and switch.
fn root(start_url: &str, test_environment: bool, switch: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut e = load_or_init(dir.path(), Some("dev-read")).unwrap().environments[0].clone();
    e.test_environment = test_environment;
    e.start_url = start_url.to_string();
    let id = e.id.clone();
    save_env_with(dir.path(), e, || vec!["dev-read".to_string()]).unwrap();
    if switch {
        set_reset_expired_passwords(dir.path(), &id, true).unwrap();
    }
    dir
}

fn proposal(recipe: SignInRecipe, hash: String) -> StoredSignInProposal {
    StoredSignInProposal {
        account: "sup".into(),
        proposed_ms: 1.0,
        summary: summary(&recipe),
        changes: vec!["steps".into()],
        recipe,
        saved_hash: hash,
    }
}

fn propose(root: &std::path::Path, recipe: SignInRecipe) {
    let env = active(root).unwrap();
    save(root, &env.id, ORG, PROJECT, &proposal(recipe, saved_hash(root, ORG, PROJECT).unwrap())).unwrap();
}

#[test]
fn merge_takes_only_the_five_parts_from_the_draft() {
    let mut saved = basic("https://saved.example.test", "#old-go");
    saved.after_sign_in = vec![serde_json::from_value(json!({ "kind": "click", "selector": { "css": "#menu" } })).unwrap()];
    saved.session_minutes = 30;
    saved.allowed_origins = vec!["https://sso.example.test".into()];
    saved.close_when_shown = vec![serde_json::from_value(
        json!({ "name": "banner", "shown": { "css": "#b" }, "close": { "css": "#bx" } }),
    )
    .unwrap()];

    let mut draft = with_reset(basic("https://draft.example.test", "#new-go"));
    draft.signed_in = serde_json::from_value(json!({ "css": "#welcome" })).unwrap();
    draft.signed_in_in_menu = true;
    draft.session_minutes = 999;

    let merged = merge(&draft, Some(&saved));
    assert_eq!(merged.start_url, draft.start_url);
    assert_eq!(merged.steps, draft.steps);
    assert_eq!(merged.signed_in, draft.signed_in);
    assert!(merged.signed_in_in_menu);
    assert_eq!(merged.password_reset, draft.password_reset);
    assert_eq!(merged.after_sign_in, saved.after_sign_in);
    assert_eq!(merged.session_minutes, 30);
    assert_eq!(merged.allowed_origins, saved.allowed_origins);
    assert_eq!(merged.close_when_shown, saved.close_when_shown);

    assert_eq!(merge(&draft, None), draft);
}

#[test]
fn changed_parts_name_what_differs() {
    let saved = basic("https://a.example.test", "#go");
    let mut draft = saved.clone();
    draft.steps = basic("https://a.example.test", "#other").steps;
    let parts = changed_parts(&merge(&draft, Some(&saved)), Some(&saved));
    assert_eq!(parts, vec!["steps".to_string()]);

    let fresh = changed_parts(&with_reset(saved.clone()), None);
    assert_eq!(fresh, vec!["address", "steps", "signed-in check", "change-password part"]);
    assert_eq!(changed_parts(&saved, None), vec!["address", "steps", "signed-in check"]);
}

#[test]
fn a_new_proposal_replaces_the_earlier_one() {
    let dir = root("", false, false);
    let env = active(dir.path()).unwrap();
    let a = proposal(basic("https://a.example.test", "#a"), String::new());
    let b = proposal(basic("https://b.example.test", "#b"), String::new());
    save(dir.path(), &env.id, ORG, PROJECT, &a).unwrap();
    save(dir.path(), &env.id, ORG, PROJECT, &b).unwrap();
    assert_eq!(load(dir.path(), &env.id, ORG, PROJECT).unwrap(), Some(b));

    let path = proposal_path(dir.path(), &env.id, ORG, PROJECT);
    assert!(path.starts_with(shared_of(dir.path()).join("proposals")));
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(&format!("signin-{}-", env.id)) && name.ends_with(".json"), "{name}");

    clear(dir.path(), &env.id, ORG, PROJECT).unwrap();
    assert_eq!(load(dir.path(), &env.id, ORG, PROJECT).unwrap(), None);
}

#[test]
fn an_unreadable_proposal_file_is_a_sentence_that_quotes_nothing() {
    let dir = root("", false, false);
    let env = active(dir.path()).unwrap();
    let path = proposal_path(dir.path(), &env.id, ORG, PROJECT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ \"account\": \"hunter2\" oops").unwrap();
    let err = load(dir.path(), &env.id, ORG, PROJECT).unwrap_err();
    assert!(!err.contains("hunter2") && !err.contains("expected") && !err.contains("line"), "{err}");
}

#[test]
fn approve_refuses_when_the_saved_recipe_changed() {
    let dir = root("", false, false);
    let r1 = basic("https://a.example.test", "#one");
    save_recipe(dir.path(), ORG, PROJECT, &r1).unwrap();
    propose(dir.path(), basic("https://a.example.test", "#proposed"));

    let r2 = basic("https://a.example.test", "#two");
    save_recipe(dir.path(), ORG, PROJECT, &r2).unwrap();

    assert_eq!(approve_with(dir.path(), ORG, PROJECT, false), Err(PROPOSAL_STALE.to_string()));
    assert_eq!(load_recipe(dir.path(), ORG, PROJECT).unwrap(), Some(r2));
    assert!(proposal_view_with(dir.path(), ORG, PROJECT).unwrap().is_some());
}

#[test]
fn approve_with_nothing_proposed_says_so() {
    let dir = root("", false, false);
    assert_eq!(
        approve_with(dir.path(), ORG, PROJECT, false),
        Err("there is no proposed sign-in to approve".to_string())
    );
}

#[test]
fn approve_keeps_the_environments_own_address() {
    let dir = root("https://env.example.test", false, false);
    let saved = basic("https://saved.example.test", "#one");
    save_recipe(dir.path(), ORG, PROJECT, &saved).unwrap();
    let proposed = basic("https://env.example.test", "#proposed");
    propose(dir.path(), proposed.clone());

    approve_with(dir.path(), ORG, PROJECT, false).unwrap();
    let now = load_recipe(dir.path(), ORG, PROJECT).unwrap().unwrap();
    assert_eq!(now.start_url, "https://saved.example.test");
    assert_eq!(now.steps, proposed.steps);
    assert!(proposal_view_with(dir.path(), ORG, PROJECT).unwrap().is_none());
}

#[test]
fn approve_turns_the_switch_on_only_on_a_test_environment() {
    // Not a test environment: refused, nothing written, proposal kept.
    let dir = root("", false, false);
    propose(dir.path(), with_reset(basic("https://a.example.test", "#go")));
    assert_eq!(approve_with(dir.path(), ORG, PROJECT, true), Err(RESET_SWITCH_NOT_TEST.to_string()));
    assert_eq!(load_recipe(dir.path(), ORG, PROJECT).unwrap(), None);
    assert!(proposal_view_with(dir.path(), ORG, PROJECT).unwrap().is_some());

    // A test environment: saved, switch on, proposal gone.
    let dir = root("", true, false);
    propose(dir.path(), with_reset(basic("https://a.example.test", "#go")));
    approve_with(dir.path(), ORG, PROJECT, true).unwrap();
    assert!(active(dir.path()).unwrap().reset_expired_passwords);
    assert!(load_recipe(dir.path(), ORG, PROJECT).unwrap().is_some());
    assert!(proposal_view_with(dir.path(), ORG, PROJECT).unwrap().is_none());

    // Ticked off: the switch stays off.
    let dir = root("", true, false);
    propose(dir.path(), with_reset(basic("https://a.example.test", "#go")));
    approve_with(dir.path(), ORG, PROJECT, false).unwrap();
    assert!(!active(dir.path()).unwrap().reset_expired_passwords);
}

#[test]
fn reject_clears_and_saves_nothing() {
    let dir = root("", false, false);
    propose(dir.path(), basic("https://a.example.test", "#go"));
    reject_with(dir.path(), ORG, PROJECT).unwrap();
    assert!(proposal_view_with(dir.path(), ORG, PROJECT).unwrap().is_none());
    assert_eq!(load_recipe(dir.path(), ORG, PROJECT).unwrap(), None);
    // Rejecting again is not an error.
    reject_with(dir.path(), ORG, PROJECT).unwrap();
}

#[test]
fn the_view_names_no_host_and_no_fixed_text() {
    let r = with_reset(recipe(json!({
        "start_url": "https://hr.example.internal/login?x=1",
        "steps": [
            { "kind": "navigate", "url": "https://hr.example.internal/login?x=1" },
            { "kind": "fill", "selector": { "css": "#branch" }, "value": "Branch-7" },
            { "kind": "fill", "selector": { "css": "#pw" }, "value": "{{password}}" }
        ],
        "signed_in": { "css": "#home" }
    })));
    let p = proposal(r, String::new());

    let env_no_address = root("", true, false);
    let env = active(env_no_address.path()).unwrap();
    let v = view(&p, &env);
    let text = serde_json::to_string(&v).unwrap();
    assert!(text.contains("/login"), "{text}");
    assert!(!text.contains("hr.example.internal"), "{text}");
    assert!(!text.contains("Branch-7"), "{text}");
    assert!(text.contains("with fixed text") && text.contains("with the password"), "{text}");
    assert!(v.offer_reset_switch);
    assert!(v.password_reset.as_deref().unwrap().starts_with("page /security/PasswordReset: new password"));

    // Switch already on, or not a test environment: no offer.
    let on = root("", true, true);
    assert!(!view(&p, &active(on.path()).unwrap()).offer_reset_switch);
    let not_test = root("", false, false);
    assert!(!view(&p, &active(not_test.path()).unwrap()).offer_reset_switch);
    // No change-password part: no offer.
    let plain = proposal(basic("https://a.example.test", "#go"), String::new());
    assert!(!view(&plain, &env).offer_reset_switch);

    // An environment with its own address says so instead of a path.
    let own = root("https://env.example.test", true, false);
    assert_eq!(view(&p, &active(own.path()).unwrap()).address, "this environment's address");
}
