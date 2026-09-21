//! How a project says "this is how you sign in here".

use serde_json::json;
use v2_lib::autorun::accounts::Account;
use v2_lib::autorun::recipe::{
    for_account, has_placeholder, load_recipe, origin_of, project_slug, save_recipe, RecipeStep, SignInRecipe,
};
use v2_lib::browser::actions::Action;

fn recipe(v: serde_json::Value) -> SignInRecipe {
    serde_json::from_value(v).expect("the test wrote an unparseable recipe")
}

fn sample() -> serde_json::Value {
    json!({
        "start_url": "https://HR.Example.internal/login",
        "steps": [
            { "kind": "fill", "selector": { "role": "textbox", "name": "Username" }, "value": "{{username}}" },
            { "kind": "fill", "selector": { "css": "input[type=password]" }, "value": "{{password}}" },
            { "kind": "click", "selector": { "role": "button", "name": "Login" } },
            { "kind": "when_visible", "selector": { "role": "button", "name": "Continue here" }, "within_ms": 5000,
              "then": [ { "kind": "click", "selector": { "role": "button", "name": "Continue here" } } ] }
        ],
        "signed_in": { "css": "#sidebar-toggle-menu" }
    })
}

#[test]
fn a_recipe_parses_with_defaults_and_round_trips() {
    let r = recipe(sample());
    assert_eq!(r.session_minutes, 480);
    assert!(r.allowed_origins.is_empty());
    assert_eq!(r.steps.len(), 4);
    assert!(matches!(r.steps[0], RecipeStep::Do(Action::Fill { .. })));
    assert!(matches!(r.steps[3], RecipeStep::WhenVisible(ref w) if w.within_ms == 5000 && w.then.len() == 1));
    assert!(r.validate().is_ok());
    let again: SignInRecipe = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(again, r);
    assert_eq!(serde_json::to_value(&r).unwrap()["steps"][3]["kind"], "when_visible");
}

#[test]
fn a_step_serializes_with_no_do_or_when_visible_wrapper() {
    let r = recipe(sample());
    // steps[0] is a plain fill action: its JSON is the action's own JSON,
    // not `{ "Do": { "kind": "fill", ... } }`.
    let action_json = serde_json::to_value(&r.steps[0]).unwrap();
    assert_eq!(action_json, json!({
        "kind": "fill",
        "selector": { "role": "textbox", "name": "Username" },
        "value": "{{username}}"
    }));
    // steps[3] is a when_visible step: its JSON carries "kind" itself, not
    // `{ "WhenVisible": { "selector": ..., ... } }`.
    let when_visible_json = serde_json::to_value(&r.steps[3]).unwrap();
    assert_eq!(when_visible_json["kind"], "when_visible");
    assert!(when_visible_json.get("WhenVisible").is_none());
    assert!(when_visible_json.get("Do").is_none());
}

#[test]
fn a_bad_step_says_what_is_wrong_with_it() {
    let mut v = sample();
    v["steps"][2] = json!({ "kind": "clik", "selector": "#go" });
    let err = serde_json::from_value::<SignInRecipe>(v).unwrap_err().to_string();
    assert!(err.contains("clik"), "{err}");

    let mut v = sample();
    v["steps"][3] = json!({ "kind": "when_visible", "selector": "#x", "within_ms": 100,
        "then": [ { "kind": "when_visible", "selector": "#y", "within_ms": 1, "then": [] } ] });
    assert!(serde_json::from_value::<SignInRecipe>(v).is_err(), "when_visible does not nest");
}

#[test]
fn validation_covers_the_address_the_steps_and_the_origins() {
    let bad = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut v = sample();
        edit(&mut v);
        recipe(v).validate().unwrap_err()
    };
    assert!(bad(&|v| v["start_url"] = json!("/login")).contains("start address"));
    assert!(bad(&|v| v["start_url"] = json!("javascript:alert(1)")).contains("start address"));
    assert!(bad(&|v| v["steps"] = json!([])).contains("no steps"));
    assert!(bad(&|v| v["signed_in"] = json!({})).contains("role, text or css"));
    assert!(bad(&|v| v["steps"][2]["selector"] = json!({})).contains("step 3"));
    assert!(bad(&|v| v["steps"][3]["then"][0]["selector"] = json!({})).contains("step 4"));
    assert!(bad(&|v| v["steps"][3]["within_ms"] = json!(0)).contains("within_ms"));
    assert!(bad(&|v| v["allowed_origins"] = json!(["not an origin"])).contains("not an origin"));
    assert!(bad(&|v| v["allowed_origins"] = json!(["https://x.example/path"])).contains("https://x.example/path"));
    assert!(bad(&|v| v["session_minutes"] = json!(0)).contains("session_minutes"));
    // A backslash or hidden whitespace inside the start address or an
    // allowed origin cannot be read as an origin at all (see
    // `origin_of_ends_the_authority_at_a_backslash_like_a_browser_does`
    // and `origin_of_refuses_an_address_with_hidden_characters_inside`),
    // so both fail validation with a sensible message rather than being
    // silently accepted.
    assert!(bad(&|v| v["start_url"] = json!("https://hr.example.internal/\tlogin")).contains("start address"));
    assert!(bad(&|v| v["allowed_origins"] = json!(["https://hr.example\t.internal"])).contains("not an origin"));
}

#[test]
fn origins_are_normalised_and_the_start_address_is_always_allowed() {
    assert_eq!(origin_of("https://HR.Example.internal/login?x=1#y").as_deref(), Some("https://hr.example.internal"));
    assert_eq!(origin_of("http://user:pw@127.0.0.1:8080/a").as_deref(), Some("http://127.0.0.1:8080"));
    assert_eq!(origin_of("file:///C:/x/page.html").as_deref(), Some("file://"));
    assert_eq!(origin_of("javascript:alert(1)"), None);
    assert_eq!(origin_of("/relative"), None);
    assert_eq!(origin_of("https:///nohost"), None);

    let mut v = sample();
    v["allowed_origins"] = json!(["https://sso.example.internal", "HTTPS://HR.example.internal"]);
    assert_eq!(
        recipe(v).origins(),
        vec!["https://hr.example.internal".to_string(), "https://sso.example.internal".to_string()]
    );
}

/// A browser treats `\` as `/` in an http(s) authority, so the origin
/// check has to end the authority there too, or an address like
/// `https://evil.example\@hr.example.internal/` - which a browser sends
/// to `evil.example`, path `/@hr.example.internal/` - would be read by
/// this function as the allowed origin `hr.example.internal` and let
/// through what the browser actually sends somewhere else entirely.
#[test]
fn origin_of_ends_the_authority_at_a_backslash_like_a_browser_does() {
    assert_eq!(
        origin_of("https://evil.example\\@hr.example.internal/").as_deref(),
        Some("https://evil.example")
    );
    assert_eq!(
        origin_of("https://hr.example.internal\\@evil.example/").as_deref(),
        Some("https://hr.example.internal"),
        "that is where a browser actually goes"
    );
}

/// Browsers silently strip a tab, CR or LF from inside an address before
/// using it, and other control characters and whitespace inside an
/// address mean the address does not read the way it is written either.
/// Refusing to name an origin for any of these is the fail-closed answer.
#[test]
fn origin_of_refuses_an_address_with_hidden_characters_inside() {
    assert_eq!(origin_of("https://hr.example.internal/\tlogin"), None);
    assert_eq!(origin_of("https://hr.example.internal/\nlogin"), None);
    assert_eq!(origin_of("https://hr.example.internal/ login"), None);
}

#[test]
fn placeholders_are_filled_in_for_one_account_everywhere_they_appear() {
    let account = Account { key: "admin".into(), label: "Admin".into(), username: "kim".into(), password: "p\"w".into() };
    let mut v = sample();
    v["steps"][3]["then"] = json!([{ "kind": "fill", "selector": "#again", "value": "{{password}}" }]);
    let steps = for_account(&recipe(v).steps, &account);
    let text = serde_json::to_string(&steps).unwrap();
    // The password is p"w, which JSON writes as p\"w.
    assert_eq!(text.matches("p\\\"w").count(), 2, "the password belongs in both fills: {text}");
    assert!(text.contains("kim") && !text.contains("{{"), "{text}");
    assert!(has_placeholder("x {{password}}") && has_placeholder("{{username}}") && !has_placeholder("plain"));
}

#[test]
fn a_recipe_is_saved_per_project() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(project_slug("Acme Corp", "Web/Portal"), "acme-corp__web-portal");
    assert!(load_recipe(dir.path(), "Acme", "Web").unwrap().is_none());
    let r = recipe(sample());
    save_recipe(dir.path(), "Acme", "Web", &r).unwrap();
    assert_eq!(load_recipe(dir.path(), "Acme", "Web").unwrap(), Some(r.clone()));
    assert!(load_recipe(dir.path(), "Acme", "Other").unwrap().is_none());
    assert!(dir.path().join("projects").join("acme__web.json").is_file());
    // An invalid recipe is refused and the saved one stays.
    let mut broken = r.clone();
    broken.steps.clear();
    assert!(save_recipe(dir.path(), "Acme", "Web", &broken).is_err());
    assert_eq!(load_recipe(dir.path(), "Acme", "Web").unwrap(), Some(r));
}

/// A recipe cannot name `sign_in` as one of its own steps - it IS the
/// sign-in, so a step that tries to change account mid-recipe makes no
/// sense.
#[test]
fn a_recipe_may_not_contain_sign_in() {
    let mut v = sample();
    v["steps"][2] = json!({ "kind": "sign_in", "account": "admin" });
    assert!(recipe(v).validate().unwrap_err().contains("sign_in"));
}
