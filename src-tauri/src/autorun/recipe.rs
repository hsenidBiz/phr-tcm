//! One sign-in recipe per project: how to get from a fresh browser to a
//! signed-in one, written in the same actions a case script uses.
//!
//! The recipe is where a login lives so that no case script has to carry
//! one. `{{username}}` and `{{password}}` are filled in for the account
//! being signed in, here and nowhere else.

use super::accounts::Account;
use crate::browser::actions::{Action, NESTED_WHEN_VISIBLE, WITHIN_MS_ZERO};
use crate::browser::locator::Target;
use std::path::{Path, PathBuf};

pub const USERNAME: &str = "{{username}}";
pub const PASSWORD: &str = "{{password}}";

pub fn has_placeholder(s: &str) -> bool {
    s.contains(USERNAME) || s.contains(PASSWORD)
}

// `WhenVisible`'s `kind` field: always this one variant, serialized (via
// `rename_all`) as `"when_visible"`, so the field is self-validating by
// construction rather than needing a hand-written check.
//
// Why a whole type for one literal: `#[serde(tag = "kind", rename =
// "when_visible")]` directly on `WhenVisible` - the container attribute
// serde itself supports for internally-tagged plain structs, not only enum
// variants - serializes correctly (`{"kind":"when_visible", ...}`,
// confirmed with a scratch test) but breaks specta's TypeScript export:
// renaming the container makes specta register two type entries under the
// identical name `"when_visible"` and refuse with "Detected multiple types
// with the same name". Without the rename, the tag serializes as the Rust
// type's own name, `"WhenVisible"` (wrong case). A field with its own
// small literal type sidesteps both: nothing about `WhenVisible` itself is
// renamed, so there is nothing for specta to collide with.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WhenVisibleKind {
    WhenVisible,
}

/// A prompt that may or may not appear. Recipe only.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct WhenVisible {
    pub kind: WhenVisibleKind,
    pub selector: Target,
    pub within_ms: u32,
    pub then: Vec<Action>,
}

// Serialized untagged - a plain action object, or `WhenVisible`'s own
// `{ "kind": "when_visible", ... }` - so the JSON on disk never wraps a
// step in `{ "Do": ... }` or `{ "WhenVisible": ... }`. `serde::Serialize`
// is derived (needed to legally write `#[serde(untagged)]` at all: that
// attribute is only recognised alongside a real `#[derive(Serialize |
// Deserialize)]`) but `Deserialize` stays hand-written below, because a
// derived untagged `Deserialize` would lose the inner "unknown variant"
// detail `a_bad_step_says_what_is_wrong_with_it` checks for - see `Target`
// in `browser::locator` for the same split.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(untagged)]
pub enum RecipeStep {
    Do(Action),
    WhenVisible(WhenVisible),
}

/// By hand, through a `Value`, so the error for a bad step is the inner
/// one ("unknown variant `clik`") and not "did not match any variant".
impl<'de> serde::Deserialize<'de> for RecipeStep {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let v = serde_json::Value::deserialize(d)?;
        if v.get("kind").and_then(|k| k.as_str()) == Some("when_visible") {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Raw {
                kind: WhenVisibleKind,
                selector: Target,
                within_ms: u32,
                then: Vec<Action>,
            }
            let raw: Raw = serde_json::from_value(v).map_err(D::Error::custom)?;
            // A script's `when_visible` is an action too, so it would parse
            // inside `then`; a recipe file holding one is refused as it is
            // read, the way it was before scripts had one.
            if raw.then.iter().any(|a| matches!(a, Action::WhenVisible { .. })) {
                return Err(D::Error::custom(NESTED_WHEN_VISIBLE));
            }
            return Ok(RecipeStep::WhenVisible(WhenVisible {
                kind: raw.kind,
                selector: raw.selector,
                within_ms: raw.within_ms,
                then: raw.then,
            }));
        }
        serde_json::from_value::<Action>(v).map(RecipeStep::Do).map_err(D::Error::custom)
    }
}

// The doc comments below say "consent banner", not the other word for it:
// they are exported into the TypeScript bindings, where `tests/bindings.rs`
// refuses that word as a sign a session secret crossed the IPC boundary.
/// A pop-up the runner closes by itself (`SignInRecipe::close_when_shown`):
/// while `shown` is showing, `close` is clicked, right before an action
/// that touches the page. Written for PeoplesHR's consent banner
/// (2026-10-14), which comes back after every reload and sits over the
/// controls along the bottom of each page.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct ClosePopup {
    /// What a step that closed it says: "closed the <name> first".
    pub name: String,
    /// The pop-up itself, as it looks while it is up.
    pub shown: Target,
    /// What closes it - never what accepts something on the person's
    /// behalf (a consent banner's x, not its Accept All).
    pub close: Target,
}

/// A loading indicator the runner waits out by itself
/// (`SignInRecipe::wait_while_shown`): while `shown` is VISIBLE, right
/// before an action that touches or checks the page, the runner waits for
/// it to go, up to `max_ms`, and only then does the action start its own
/// timeout. Written for PeoplesHR's full-page spinner (2026-10-16), up for
/// 20-60 s on a cold start, which made the first step of almost every run
/// time out. An indicator left in the page hidden is not loading.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct LoadingIndicator {
    /// What a step that waited for it says: "waited 12 s for <name>".
    pub name: String,
    /// The indicator itself, as it looks while the page loads.
    pub shown: Target,
    /// The longest the runner waits for it to go, from
    /// `loading::MIN_MAX_MS` to `loading::MAX_MAX_MS`; left out,
    /// `loading::DEFAULT_MAX_MS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ms: Option<u32>,
}

impl LoadingIndicator {
    /// `max_ms`, or the default when it is left out.
    pub fn limit_ms(&self) -> u32 {
        self.max_ms.unwrap_or(super::loading::DEFAULT_MAX_MS)
    }
}

fn default_minutes() -> u32 {
    480
}

/// The application's menu-preferences dialog, as far as the project's areas
/// need it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct MenuPreferences {
    /// From a signed-in page, the clicks that open the preferences dialog, in order.
    pub open: Vec<Target>,
    /// The control that closes it.
    pub close: Target,
    pub settings: Vec<MenuPreference>,
}

/// One setting in the dialog.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct MenuPreference {
    /// How answers and the area file name it: "Group Menu Items".
    pub name: String,
    /// A checkbox or switch, readable while the dialog is open: checked (or
    /// aria-checked="true") is on.
    pub control: Target,
    /// What the project's areas need when an area recorded no value for it.
    pub on: bool,
}

/// One of the application's modes (Admin, Self Service): the same screens
/// drawn for a different role, switched between by a control.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct MenuMode {
    /// How areas and answers name it: "Admin", "Self Service".
    pub name: String,
    /// Visible while the account is in this mode.
    pub shown: Target,
    /// Clicked, from another mode, to switch to this one.
    pub switch: Target,
}

/// Why a recipe's modes cannot be used, or `Ok`.
fn check_modes(modes: &[MenuMode]) -> Result<(), String> {
    if modes.is_empty() {
        return Ok(());
    }
    if modes.len() < 2 {
        return Err("modes: name every mode the application has - one mode is nothing to switch between".to_string());
    }
    let mut seen: Vec<String> = Vec::new();
    for (i, m) in modes.iter().enumerate() {
        let n = i + 1;
        let name = m.name.trim();
        if name.is_empty() {
            return Err(format!("modes: mode {n} needs a name"));
        }
        let lower = name.to_lowercase();
        if seen.contains(&lower) {
            return Err(format!("modes: mode {n}: the name \"{name}\" is used twice"));
        }
        seen.push(lower);
        m.shown.validate().map_err(|e| format!("modes: mode {n} ({name}): shown: {e}"))?;
        m.switch.validate().map_err(|e| format!("modes: mode {n} ({name}): switch: {e}"))?;
    }
    if serde_json::to_value(modes).is_ok_and(|v| has_placeholder_anywhere(&v)) {
        return Err(
            "modes: {{username}} and {{password}} are filled in for the recipe's own steps only - here they would be used as they are"
                .to_string(),
        );
    }
    Ok(())
}

/// Why a recipe's menu preferences cannot be used, or `Ok`.
fn check_menu_preferences(prefs: &MenuPreferences) -> Result<(), String> {
    if prefs.open.is_empty() {
        return Err("menu_preferences: \"open\" needs at least one click".to_string());
    }
    if prefs.settings.is_empty() {
        return Err("menu_preferences: \"settings\" needs at least one setting".to_string());
    }
    for (i, t) in prefs.open.iter().enumerate() {
        t.validate().map_err(|e| format!("menu_preferences: open {}: {e}", i + 1))?;
    }
    prefs.close.validate().map_err(|e| format!("menu_preferences: close: {e}"))?;
    let mut seen: Vec<String> = Vec::new();
    for (i, s) in prefs.settings.iter().enumerate() {
        let n = i + 1;
        let name = s.name.trim();
        if name.is_empty() {
            return Err(format!("menu_preferences: setting {n} needs a name"));
        }
        let lower = name.to_lowercase();
        if seen.contains(&lower) {
            return Err(format!("menu_preferences: setting {n}: the name \"{name}\" is used twice"));
        }
        seen.push(lower);
        s.control.validate().map_err(|e| format!("menu_preferences: setting {n} ({name}): {e}"))?;
    }
    if serde_json::to_value(prefs).is_ok_and(|v| has_placeholder_anywhere(&v)) {
        return Err(
            "menu_preferences: {{username}} and {{password}} are filled in for the recipe's own steps only - here they would be used as they are"
                .to_string(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct SignInRecipe {
    /// Where a fresh browser goes first. Absolute, http or https.
    pub start_url: String,
    pub steps: Vec<RecipeStep>,
    /// Run after EVERY sign-in - the recipe's own, or a saved session put
    /// back, which skips `steps` - to leave the application the way the
    /// scripts expect it. The same step vocabulary, so `when_visible` makes
    /// a toggle safe to run twice. Written for PeoplesHR (2026-09-25): its
    /// menu list is drawn closed in a fresh browser and opens only from an
    /// unlabelled icon that toggles, so a module path recorded with the
    /// menu open failed its check with "is outside the visible part of the
    /// page". No login is filled in here: a placeholder is refused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_sign_in: Vec<RecipeStep>,
    /// The application's own menu preferences the project's areas depend
    /// on: how to open the dialog, each setting's control, and the value
    /// the areas need. After a sign-in the app sets each one that differs
    /// from what the area recorded (`menu_setup`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu_preferences: Option<MenuPreferences>,
    /// The application's modes (Admin, Self Service), when it has them: what
    /// shows in each and the control that switches to it. An area records
    /// the mode it was saved in, and every trip to it switches to that mode
    /// first (`menu_setup::match_mode`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<MenuMode>,
    /// Pop-ups the runner closes by itself: before every action that
    /// touches the page (a click, a fill, a drag, an upload) it looks
    /// once, without waiting, for each, and clicks the close control of one
    /// that is up - so no script has to guard against them. Nothing that is
    /// not listed here is ever closed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub close_when_shown: Vec<ClosePopup>,
    /// Loading indicators the runner waits out by itself: before every
    /// action that touches or checks the page it looks once, without
    /// waiting, for each, and while one is visible it waits for it to go
    /// (`loading::wait_out`) - so no script needs a long first-step wait.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wait_while_shown: Vec<LoadingIndicator>,
    /// Exactly one visible match of this means "signed in".
    pub signed_in: Target,
    /// Other origins `navigate` may go to. The start address's own origin
    /// is always allowed.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// How long a saved session is trusted.
    #[serde(default = "default_minutes")]
    pub session_minutes: u32,
}

/// `scheme://host[:port]`, lowercased, user info dropped. Every file
/// address is the one origin `file://`. Anything else has no origin here.
///
/// Two things a browser does that this has to match, or the check and the
/// browser disagree about where an address goes:
/// - A browser treats `\` the same as `/` inside an http(s) authority, so
///   the authority ends at the first of `/`, `\`, `?` or `#` - not just
///   `/`, `?` or `#`. Ending it at `/` alone lets `evil.example\@allowed/`
///   read back as the origin `allowed` while a browser sends it to
///   `evil.example`.
/// - A browser silently strips a tab, CR or LF from inside an address
///   before using it, so such an address never means what it reads as
///   here. Any other control character or whitespace inside it is the
///   same problem. Refusing to name an origin for any of these - `None`,
///   even though the text looks parseable - is the fail-closed answer.
pub fn origin_of(url: &str) -> Option<String> {
    let u = url.trim();
    if u.chars().any(|c| c.is_ascii_control() || c.is_ascii_whitespace()) {
        return None;
    }
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("file://") {
        return Some("file://".to_string());
    }
    let scheme = if lower.starts_with("https://") {
        "https"
    } else if lower.starts_with("http://") {
        "http"
    } else {
        return None;
    };
    let rest = &u[scheme.len() + 3..];
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    // An explicit default port is the same origin as no port at all, the
    // way a browser treats it: `https://host:443/x` and `https://host/x`
    // must agree, or a policy written as one and a script's `navigate`
    // written as the other would disagree about whether they match.
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    let host = host.strip_suffix(default_port).unwrap_or(&host);
    Some(format!("{scheme}://{host}"))
}

/// `origin_of` alone cannot refuse a path: it stops parsing at the
/// authority either way, so `origin_of("https://x/a")` and
/// `origin_of("https://x")` are the same value. An origin written with its
/// scheme's own default port (`https://x:443`) must still be accepted -
/// `origin_of` strips that port, so the raw text is compared against the
/// canonical origin WITH that port added back, as well as without it - and
/// nothing may follow the authority but one optional trailing slash.
pub fn is_bare_origin(s: &str) -> bool {
    let Some(canonical) = origin_of(s) else { return false };
    let default_port = if canonical.starts_with("https://") {
        ":443"
    } else if canonical.starts_with("http://") {
        ":80"
    } else {
        ""
    };
    let t = s.trim().to_ascii_lowercase();
    [canonical.clone(), format!("{canonical}{default_port}")]
        .iter()
        .any(|c| t == *c || t == format!("{c}/"))
}

/// Does any string anywhere in this JSON value hold a placeholder? Used to
/// scan a whole action (or a fill's selector alone) at once rather than
/// hand-checking each field of every action variant.
fn has_placeholder_anywhere(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(s) => has_placeholder(s),
        serde_json::Value::Array(a) => a.iter().any(has_placeholder_anywhere),
        serde_json::Value::Object(o) => o.values().any(has_placeholder_anywhere),
        _ => false,
    }
}

fn check(action: &Action) -> Result<(), String> {
    let v = serde_json::to_value(action).ok();
    let kind = v.as_ref().and_then(|v| v["kind"].as_str()).map(str::to_string);
    // Only `then` reaches here holding one: a recipe's own `when_visible`
    // is read as `RecipeStep::WhenVisible`, never as an action.
    if kind.as_deref() == Some("when_visible") {
        return Err(NESTED_WHEN_VISIBLE.to_string());
    }
    if kind.as_deref() == Some("sign_in") {
        return Err("a recipe cannot contain sign_in - it IS the sign-in".to_string());
    }
    // Signing in never sends a document, and a recipe runs for every
    // project's sign-in, where no Test files folder is in reach.
    if kind.as_deref() == Some("upload") {
        return Err("a recipe cannot contain upload - signing in never sends a file; upload belongs in a case script".to_string());
    }
    // Checking what a page asks its server is part of a case: a recipe
    // runs for every case's sign-in, and its steps carry no expectations.
    // Nor does it check a downloaded file.
    if let Some(k @ ("expect_response" | "api_request" | "expect_download")) = kind.as_deref() {
        return Err(format!("a sign-in recipe cannot contain {k} - it belongs in a case script"));
    }
    // Ending the session undoes the sign-in itself, and going to a case's
    // area means a case: neither has a place in how every case signs in.
    // Resetting a list to a known state is a case's own setup, never part
    // of how every case signs in.
    if let Some(k @ ("expire_session" | "return_to_area" | "repeat_while_visible")) = kind.as_deref() {
        return Err(format!("a sign-in recipe cannot contain {k} - it belongs in a case script"));
    }
    // A dialog a step expects is a case's check; a sign-in's dialogs are
    // answered as always, and its steps carry no expectations.
    if kind.as_deref() == Some("expect_dialog") {
        return Err("a sign-in recipe cannot contain expect_dialog - it belongs in a case script".to_string());
    }
    // Every case signs in in its first tab, `main`: tabs are a case's own.
    if let Some(k @ ("expect_tab" | "open_tab" | "switch_tab" | "close_tab" | "expect_tab_closed")) = kind.as_deref() {
        return Err(format!("a sign-in recipe cannot contain {k} - it belongs in a case script"));
    }
    // `{{username}}`/`{{password}}` are filled in only for a fill's own
    // VALUE (see `fill_in`); anywhere else - a navigate url, a locator, an
    // expectation, even a fill's own selector - the placeholder is left
    // literal, which is a recipe that looks right and does nothing.
    if let Some(v) = &v {
        let scanned = if kind.as_deref() == Some("fill") { v.get("selector") } else { Some(v) };
        if scanned.is_some_and(has_placeholder_anywhere) {
            return Err(
                "a placeholder ({{username}} or {{password}}) belongs only in a fill's value".to_string(),
            );
        }
    }
    action.validate()
}

/// Each step in a list, named "<label> <n>" in a refusal. `fill_placeholders`
/// is whether this list gets the account's login filled in - only the
/// recipe's own `steps` do; anywhere else a placeholder would be typed as
/// it stands.
fn check_steps(steps: &[RecipeStep], label: &str, fill_placeholders: bool) -> Result<(), String> {
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        let at = |e: String| format!("{label} {n}: {e}");
        let actions: Vec<&Action> = match step {
            RecipeStep::Do(a) => vec![a],
            RecipeStep::WhenVisible(w) => {
                if w.within_ms == 0 {
                    return Err(at(WITHIN_MS_ZERO.to_string()));
                }
                w.selector.validate().map_err(|e| at(e))?;
                w.then.iter().collect()
            }
        };
        for a in actions {
            check(a).map_err(|e| at(e))?;
            if !fill_placeholders && serde_json::to_value(a).is_ok_and(|v| has_placeholder_anywhere(&v)) {
                return Err(at(
                    "{{username}} and {{password}} are filled in for the recipe's own steps only - here they would be typed as they are"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Each pop-up the runner closes, named "close_when_shown <n> (<name>)" in
/// a refusal: a name to say it by, and two selectors that can be looked up
/// as they stand - nothing is filled in here.
fn check_popups(popups: &[ClosePopup]) -> Result<(), String> {
    for (i, p) in popups.iter().enumerate() {
        let n = i + 1;
        let name = p.name.trim();
        if name.is_empty() {
            return Err(format!(
                "close_when_shown {n}: give the pop-up a short name, such as \"cookie banner\" - a step that closed it says \"closed the cookie banner\""
            ));
        }
        let at = |e: String| format!("close_when_shown {n} ({name}): {e}");
        for (field, target) in [("shown", &p.shown), ("close", &p.close)] {
            target.validate().map_err(|e| at(format!("\"{field}\" - {e}")))?;
            if crate::browser::locator::has_input_placeholder(target) {
                return Err(at(format!("\"{field}\" - a component input has no value here; write the selector out")));
            }
        }
        if serde_json::to_value(p).is_ok_and(|v| has_placeholder_anywhere(&v)) {
            return Err(at(
                "{{username}} and {{password}} are filled in for the recipe's own steps only - here they would be typed as they are"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

/// Each loading indicator the runner waits out, named "wait_while_shown
/// <n> (<name>)" in a refusal: a name to say it by, a selector that can be
/// looked up as it stands, and a `max_ms` in range.
fn check_loaders(loaders: &[LoadingIndicator]) -> Result<(), String> {
    use super::loading::{DEFAULT_MAX_MS, MAX_MAX_MS, MIN_MAX_MS};
    for (i, l) in loaders.iter().enumerate() {
        let n = i + 1;
        let name = l.name.trim();
        if name.is_empty() {
            return Err(format!(
                "wait_while_shown {n}: give the loading indicator a short name, such as \"page loader\" - a step that waited for it says \"waited 12 s for page loader\""
            ));
        }
        let at = |e: String| format!("wait_while_shown {n} ({name}): {e}");
        l.shown.validate().map_err(|e| at(format!("\"shown\" - {e}")))?;
        if crate::browser::locator::has_input_placeholder(&l.shown) {
            return Err(at("\"shown\" - a component input has no value here; write the selector out".to_string()));
        }
        if serde_json::to_value(l).is_ok_and(|v| has_placeholder_anywhere(&v)) {
            return Err(at(
                "{{username}} and {{password}} are filled in for the recipe's own steps only - here they would be typed as they are"
                    .to_string(),
            ));
        }
        if l.max_ms.is_some_and(|ms| !(MIN_MAX_MS..=MAX_MAX_MS).contains(&ms)) {
            return Err(at(format!(
                "max_ms must be from {MIN_MAX_MS} to {MAX_MAX_MS} ({} to {} s) - leave it out to wait up to {DEFAULT_MAX_MS}",
                MIN_MAX_MS / 1000,
                MAX_MAX_MS / 1000
            )));
        }
    }
    Ok(())
}

/// The rule a recipe's `start_url` must pass, on its own: a recording asks
/// before it opens a browser, with the same words `validate` refuses in.
pub fn check_start_url(url: &str) -> Result<(), String> {
    match origin_of(url) {
        Some(o) if o != "file://" => Ok(()),
        _ => Err("the start address must be a full http or https address".to_string()),
    }
}

impl SignInRecipe {
    pub fn validate(&self) -> Result<(), String> {
        check_start_url(&self.start_url)?;
        if self.steps.is_empty() {
            return Err("the recipe has no steps".to_string());
        }
        check_steps(&self.steps, "step", true)?;
        check_steps(&self.after_sign_in, "after_sign_in step", false)?;
        if let Some(p) = &self.menu_preferences {
            check_menu_preferences(p)?;
        }
        check_modes(&self.modes)?;
        check_popups(&self.close_when_shown)?;
        check_loaders(&self.wait_while_shown)?;
        self.signed_in.validate().map_err(|e| format!("signed_in: {e}"))?;
        for o in &self.allowed_origins {
            if !is_bare_origin(o) {
                return Err(format!(
                    "\"{o}\" is not an origin - write it as https://host or https://host:port, with no path"
                ));
            }
        }
        if self.session_minutes == 0 {
            return Err("session_minutes must be more than 0".to_string());
        }
        Ok(())
    }

    /// Everywhere `navigate` may go: the start address first. Every entry
    /// is re-derived through `origin_of` rather than trusted as written -
    /// `load_effective_recipe` relies on that, since it validates neither a
    /// hand-edited recipe file nor a hand-edited environment's address
    /// before handing them to a run.
    pub fn origins(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let all = origin_of(&self.start_url).into_iter().chain(self.allowed_origins.iter().filter_map(|o| origin_of(o)));
        for o in all {
            if !out.contains(&o) {
                out.push(o);
            }
        }
        out
    }
}

fn fill_in(action: &Action, account: &Account) -> Action {
    match action {
        Action::Fill { selector, value } => Action::Fill {
            selector: selector.clone(),
            value: value.replace(USERNAME, &account.username).replace(PASSWORD, &account.password),
        },
        other => other.clone(),
    }
}

/// The steps with this account's login filled in. The result holds a
/// password: it is executed and dropped, never stored or logged.
pub fn for_account(steps: &[RecipeStep], account: &Account) -> Vec<RecipeStep> {
    steps
        .iter()
        .map(|s| match s {
            RecipeStep::Do(a) => RecipeStep::Do(fill_in(a, account)),
            RecipeStep::WhenVisible(w) => RecipeStep::WhenVisible(WhenVisible {
                kind: w.kind,
                selector: w.selector.clone(),
                within_ms: w.within_ms,
                then: w.then.iter().map(|a| fill_in(a, account)).collect(),
            }),
        })
        .collect()
}

fn slug_part(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    // A name with no ASCII alphanumerics (all-symbols, or entirely
    // non-ASCII) would otherwise slug to "", and two such names would
    // collide on the same file - the trailing hash in `project_slug` is
    // what actually keeps them apart, this is just a readable stand-in.
    if out.is_empty() {
        "x".to_string()
    } else {
        out
    }
}

/// 32-bit FNV-1a, written inline rather than pulling in a crate or using
/// `std::hash::DefaultHasher` (not stable across Rust releases, so a slug
/// computed by one toolchain could stop matching one computed by another).
///
/// Shared with the review page, which keys a reader's bookmark the same
/// way - one short hash that is the same on every run, not two.
pub(crate) fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h
}

/// `acme-corp__web-portal-1a2b3c4d`: safe as a file name, readable in a
/// folder. The readable parts alone are not enough to tell two projects
/// apart - `PHR Cloud`, `PHR-Cloud` and `PHR_Cloud` all read as
/// `phr-cloud` - so the trailing 8 hex digits are a stable hash of the
/// raw names, lower-cased first because Azure DevOps names are
/// case-insensitive and this slug must not change under a mere case
/// difference.
pub fn project_slug(org: &str, project: &str) -> String {
    let hash = fnv1a(&format!("{}\n{}", org.trim().to_lowercase(), project.trim().to_lowercase()));
    format!("{}__{}-{hash:08x}", slug_part(org), slug_part(project))
}

/// The projects (in the order given) that have any Auto Run file under
/// `<root>/projects/`. Every per-project file is named `{slug}.json`,
/// `{slug}.nav.json`, `{slug}.pwexport.json` or `{slug}-<kind>.json`, so a
/// project has files when some entry starts with `{slug}.` or `{slug}-`.
/// The slug ends in a hash of the whole name, so one project's prefix is
/// never another's. Read-only: a missing folder answers `[]` and nothing is
/// ever created.
pub fn projects_with_files(root: &Path, org: &str, projects: &[String]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("projects")) else {
        return Vec::new();
    };
    let names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    projects
        .iter()
        .filter(|p| {
            let slug = project_slug(org, p);
            let dotted = format!("{slug}.");
            let dashed = format!("{slug}-");
            names.iter().any(|n| n.starts_with(&dotted) || n.starts_with(&dashed))
        })
        .cloned()
        .collect()
}

pub(crate) fn recipe_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}.json", project_slug(org, project)))
}

pub fn load_recipe(root: &Path, org: &str, project: &str) -> Result<Option<SignInRecipe>, String> {
    match std::fs::read_to_string(recipe_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map(Some).map_err(|e| format!("the sign-in recipe is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// What bringing the saved sign-in recipe of one environment to another
/// would add: `None` when the target already has one or the source has none,
/// else the site address the recipe carries (empty when it names none or the
/// file cannot be read as a recipe, which is then copied as it is).
pub fn saved_recipe_to_bring(source: &Path, target: &Path, org: &str, project: &str) -> Result<Option<String>, String> {
    if recipe_path(target, org, project).exists() || !recipe_path(source, org, project).exists() {
        return Ok(None);
    }
    Ok(Some(load_recipe(source, org, project).ok().flatten().map(|r| r.start_url).unwrap_or_default()))
}

/// Copies the saved recipe file of `source` to `target` as it is, only when
/// the target has none. Never resolves an address and never signs in.
/// Answers whether it copied.
pub fn copy_saved_recipe_if_absent(source: &Path, target: &Path, org: &str, project: &str) -> Result<bool, String> {
    let (from, to) = (recipe_path(source, org, project), recipe_path(target, org, project));
    if to.exists() {
        return Ok(false);
    }
    let text = match std::fs::read_to_string(&from) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    crate::autorun::roots::create_dir_all(target, to.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    crate::autorun::roots::write_file(target, &to, &text)?;
    Ok(true)
}

/// The recipe as it runs in `env`: an environment with an address signs in
/// there, and `navigate` may go to that address and the environment's own
/// allowed sites - never the recipe's, which belong to another site. An
/// environment with no address leaves the recipe exactly as it is.
pub fn effective_recipe(recipe: &SignInRecipe, env: &crate::environments::Environment) -> SignInRecipe {
    let start = env.start_url.trim();
    if start.is_empty() {
        return recipe.clone();
    }
    SignInRecipe { start_url: start.to_string(), allowed_origins: env.allowed_origins.clone(), ..recipe.clone() }
}

/// The sign-in this app ships, used by every project that has not saved
/// its own (owner ruling, 2026-10-02). It has no address: a project using
/// it signs in at the active environment's site address, and nowhere else.
pub const BUILTIN_RECIPE_JSON: &str = include_str!("builtin_recipe.json");

/// Said when a project has no saved recipe and the active environment has
/// no site address: the built-in recipe has nowhere to sign in. Names the
/// Setup row whose Edit sets the address.
pub const NO_SITE_ADDRESS: &str = "set the site address first - Auto Run, Setup, Site address - or copy this environment's Auto Run data from another one in Edit environments on the AI Bridge tab";

/// `BUILTIN_RECIPE_JSON` as a recipe, with an empty `start_url` - it does
/// not validate until an address is filled in (`effective_recipe`).
pub fn builtin_recipe() -> SignInRecipe {
    let mut v: serde_json::Value = serde_json::from_str(BUILTIN_RECIPE_JSON).expect("the built-in recipe is JSON");
    v["start_url"] = serde_json::Value::String(String::new());
    serde_json::from_value(v).expect("the built-in recipe is a recipe")
}

/// The project's recipe as it runs in the active environment, or None when
/// there is none to run: no saved recipe AND no site address. A saved
/// recipe always wins; without one, the built-in signs in at the
/// environment's address.
pub fn load_effective_recipe_if_any(root: &Path, org: &str, project: &str) -> Result<Option<SignInRecipe>, String> {
    let saved = load_recipe(root, org, project)?;
    let env = crate::environments::active(root)?;
    Ok(match saved {
        Some(recipe) => Some(effective_recipe(&recipe, &env)),
        None if env.start_url.trim().is_empty() => None,
        None => Some(effective_recipe(&builtin_recipe(), &env)),
    })
}

/// The project's recipe as it runs in the active environment - what every
/// sign-in and every navigation uses - refused with `NO_SITE_ADDRESS` when
/// there is none to run. `load_recipe` is the raw file, read directly only
/// where the recipe is edited.
pub fn load_effective_recipe(root: &Path, org: &str, project: &str) -> Result<SignInRecipe, String> {
    load_effective_recipe_if_any(root, org, project)?.ok_or_else(|| NO_SITE_ADDRESS.to_string())
}

pub fn save_recipe(root: &Path, org: &str, project: &str, recipe: &SignInRecipe) -> Result<(), String> {
    recipe.validate()?;
    // `slug_part` never reads as empty any more (a name with no ASCII
    // alphanumerics becomes "x"), so the real refusal - nothing was typed
    // at all - has to be checked on the raw names instead.
    if org.trim().is_empty() || project.trim().is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    let path = recipe_path(root, org, project);
    crate::autorun::roots::create_dir_all(root, path.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(recipe).map_err(|e| e.to_string())?;
    crate::autorun::roots::write_file(root, &path, &json)?;
    Ok(())
}
