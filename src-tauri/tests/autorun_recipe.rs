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

#[test]
fn an_allowed_origin_may_be_written_with_its_default_port() {
    let mut v = sample();
    v["allowed_origins"] = json!(["https://sso.example.internal:443", "http://intranet.example.internal:80"]);
    let r = recipe(v);
    r.validate().expect("a default port is the same origin");
    let origins = r.origins();
    assert!(origins.contains(&"https://sso.example.internal".to_string()), "{origins:?}");
    assert!(origins.contains(&"http://intranet.example.internal".to_string()), "{origins:?}");
    // A path is still refused.
    let mut bad = sample();
    bad["allowed_origins"] = json!(["https://sso.example.internal:443/login"]);
    assert!(recipe(bad).validate().is_err());
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

/// An explicit default port is the same origin as none, exactly as a
/// browser treats it - but a non-default port is still its own origin.
#[test]
fn origin_of_drops_only_the_schemes_own_default_port() {
    assert_eq!(origin_of("https://host:443/x"), origin_of("https://host/x"));
    assert_eq!(origin_of("https://host/x").as_deref(), Some("https://host"));
    assert_eq!(origin_of("http://host:80/x"), origin_of("http://host/x"));
    assert_eq!(origin_of("http://host/x").as_deref(), Some("http://host"));
    assert_eq!(origin_of("https://host:8443/x").as_deref(), Some("https://host:8443"));
    // The other scheme's default port is not dropped.
    assert_eq!(origin_of("http://host:443/x").as_deref(), Some("http://host:443"));
    assert_eq!(origin_of("https://host:80/x").as_deref(), Some("https://host:80"));
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
    let slug = project_slug("Acme Corp", "Web/Portal");
    // The readable part is unchanged; an 8-hex-digit hash is appended so
    // two projects that read the same never share a file (see
    // `two_look_alike_names_never_share_a_slug`).
    assert!(slug.starts_with("acme-corp__web-portal-"), "{slug}");
    assert_eq!(slug.len(), "acme-corp__web-portal-".len() + 8, "{slug}");
    assert!(load_recipe(dir.path(), "Acme", "Web").unwrap().is_none());
    let r = recipe(sample());
    save_recipe(dir.path(), "Acme", "Web", &r).unwrap();
    assert_eq!(load_recipe(dir.path(), "Acme", "Web").unwrap(), Some(r.clone()));
    assert!(load_recipe(dir.path(), "Acme", "Other").unwrap().is_none());
    assert!(dir.path().join("projects").join(format!("{}.json", project_slug("Acme", "Web"))).is_file());
    // An invalid recipe is refused and the saved one stays.
    let mut broken = r.clone();
    broken.steps.clear();
    assert!(save_recipe(dir.path(), "Acme", "Web", &broken).is_err());
    assert_eq!(load_recipe(dir.path(), "Acme", "Web").unwrap(), Some(r));
}

/// `PHR Cloud`, `PHR-Cloud` and `PHR_Cloud` all read the same
/// (`phr-cloud`) once punctuation is folded to a hyphen - the whole point
/// of the appended hash is that they must not overwrite one another.
#[test]
fn two_look_alike_names_never_share_a_slug() {
    let a = project_slug("PHR Cloud", "X");
    let b = project_slug("PHR-Cloud", "X");
    let c = project_slug("PHR_Cloud", "X");
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
}

/// Azure DevOps names are case-insensitive, so the same project under a
/// different case must still land in the one file it already has.
#[test]
fn the_same_name_in_different_case_gives_the_same_slug() {
    assert_eq!(project_slug("Acme", "Web"), project_slug("ACME", "WEB"));
    assert_eq!(project_slug("Acme", "Web"), project_slug("acme", "web"));
}

/// A name with no ASCII alphanumerics (here, entirely non-ASCII) must
/// still save and load - it reads as "x" but the hash keeps it distinct.
#[test]
fn a_non_ascii_name_saves_and_loads() {
    let dir = tempfile::tempdir().unwrap();
    let r = recipe(sample());
    save_recipe(dir.path(), "日本語", "組織", &r).unwrap();
    assert_eq!(load_recipe(dir.path(), "日本語", "組織").unwrap(), Some(r));
    let slug = project_slug("日本語", "組織");
    assert!(slug.starts_with("x__x-"), "{slug}");
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

/// A placeholder is only ever filled in for a fill's own VALUE - anywhere
/// else it is left literal, which is a recipe that looks right and does
/// nothing. Save must refuse it there instead.
#[test]
fn a_placeholder_is_refused_anywhere_but_a_fills_value() {
    // In a navigate url.
    let mut v = sample();
    v["steps"][2] = json!({ "kind": "navigate", "url": "https://hr.example.internal/{{username}}" });
    let err = recipe(v).validate().unwrap_err();
    assert!(err.contains("step 3") && err.contains("belongs only in a fill's value"), "{err}");

    // In a fill's own selector - its VALUE is the one place this is fine.
    let mut v = sample();
    v["steps"][0]["selector"] = json!({ "css": "#{{username}}" });
    let err = recipe(v).validate().unwrap_err();
    assert!(err.contains("step 1") && err.contains("belongs only in a fill's value"), "{err}");

    // In an expectation.
    let mut v = sample();
    v["steps"][2] = json!({ "kind": "check_text", "value": "welcome {{username}}" });
    let err = recipe(v).validate().unwrap_err();
    assert!(err.contains("step 3") && err.contains("belongs only in a fill's value"), "{err}");

    // Inside a when_visible's own `then` actions too.
    let mut v = sample();
    v["steps"][3]["then"][0] = json!({ "kind": "click", "selector": { "css": "#{{password}}" } });
    let err = recipe(v).validate().unwrap_err();
    assert!(err.contains("step 4") && err.contains("belongs only in a fill's value"), "{err}");

    // A fill's VALUE is exactly where a placeholder belongs.
    assert!(recipe(sample()).validate().is_ok());
}

// ------------------------------------------------------------ after_sign_in

fn with_after(after: serde_json::Value) -> serde_json::Value {
    let mut v = sample();
    v["after_sign_in"] = after;
    v
}

/// PeoplesHR draws its menu list closed in a fresh browser and opens it
/// only from an unlabelled icon that toggles - so what a project needs
/// after signing in is "click it if it is closed", in the recipe's own
/// vocabulary (2026-09-25).
#[test]
fn after_sign_in_takes_the_recipes_steps_and_is_optional() {
    let menu = with_after(json!([
        { "kind": "when_visible", "selector": { "css": "#sidebar-toggle-menu:not(.active)" }, "within_ms": 1500,
          "then": [ { "kind": "click", "selector": { "css": "#sidebar-toggle-menu" } } ] }
    ]));
    let r = recipe(menu);
    assert_eq!(r.after_sign_in.len(), 1);
    assert!(r.validate().is_ok(), "{:?}", r.validate());

    // A recipe written before the field existed still loads, and one that
    // does not use it saves exactly as it did.
    let old = recipe(sample());
    assert!(old.after_sign_in.is_empty());
    assert!(old.validate().is_ok());
    let saved = serde_json::to_value(&old).unwrap();
    assert!(saved.get("after_sign_in").is_none(), "{saved}");
}

#[test]
fn after_sign_in_is_validated_like_the_steps() {
    let bad = |after: serde_json::Value| recipe(with_after(after)).validate().unwrap_err();

    // An empty list is fine: it is optional.
    assert!(recipe(with_after(json!([]))).validate().is_ok());

    let why = bad(json!([{ "kind": "click", "selector": {} }]));
    assert!(why.contains("after_sign_in step 1"), "{why}");

    let why = bad(json!([{ "kind": "when_visible", "selector": "#x", "within_ms": 0, "then": [] }]));
    assert!(why.contains("after_sign_in step 1") && why.contains("within_ms"), "{why}");

    let why = bad(json!([{ "kind": "sign_in", "account": "hr.admin" }]));
    assert!(why.contains("after_sign_in step 1"), "{why}");

    // The login is filled in for the recipe's own steps only; here a
    // placeholder would be typed as the literal text - so it is refused.
    let why = bad(json!([
        { "kind": "click", "selector": { "css": "#ok" } },
        { "kind": "fill", "selector": { "css": "#pin" }, "value": "{{password}}" }
    ]));
    assert!(why.contains("after_sign_in step 2") && why.contains("{{password}}"), "{why}");
}
