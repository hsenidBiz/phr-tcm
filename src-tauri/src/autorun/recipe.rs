//! One sign-in recipe per project: how to get from a fresh browser to a
//! signed-in one, written in the same actions a case script uses.
//!
//! The recipe is where a login lives so that no case script has to carry
//! one. `{{username}}` and `{{password}}` are filled in for the account
//! being signed in, here and nowhere else.

use super::accounts::Account;
use crate::browser::actions::Action;
use crate::browser::locator::Target;
use std::path::{Path, PathBuf};

pub const USERNAME: &str = "{{username}}";
pub const PASSWORD: &str = "{{password}}";

pub fn has_placeholder(s: &str) -> bool {
    s.contains(USERNAME) || s.contains(PASSWORD)
}

/// A prompt that may or may not appear. Recipe only.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct WhenVisible {
    pub selector: Target,
    pub within_ms: u32,
    pub then: Vec<Action>,
}

// NOTE (not a doc comment - kept out of the generated bindings): specta's
// TypeScript for this is the externally-tagged `{ Do: Action } | {
// WhenVisible: WhenVisible }`, not the actual wire shape the hand-written
// impls below read and write. `#[serde(untagged)]` would fix that, but
// rustc refuses it here ("cannot find attribute `serde` in this scope"):
// the attribute is only legal alongside a real `#[derive(serde::Serialize
// | Deserialize)]`, and this enum deliberately has neither, because a
// derived untagged Deserialize would lose the inner "unknown variant"
// detail `a_bad_step_says_what_is_wrong_with_it` checks for. Left as a
// known limitation; see the Task 2 report.
#[derive(Debug, Clone, PartialEq, specta::Type)]
pub enum RecipeStep {
    Do(Action),
    WhenVisible(WhenVisible),
}

impl serde::Serialize for RecipeStep {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            RecipeStep::Do(a) => a.serialize(s),
            RecipeStep::WhenVisible(w) => {
                let mut v = serde_json::to_value(w).map_err(serde::ser::Error::custom)?;
                v["kind"] = serde_json::json!("when_visible");
                v.serialize(s)
            }
        }
    }
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
                #[allow(dead_code)]
                kind: String,
                selector: Target,
                within_ms: u32,
                then: Vec<Action>,
            }
            let raw: Raw = serde_json::from_value(v).map_err(D::Error::custom)?;
            return Ok(RecipeStep::WhenVisible(WhenVisible {
                selector: raw.selector,
                within_ms: raw.within_ms,
                then: raw.then,
            }));
        }
        serde_json::from_value::<Action>(v).map(RecipeStep::Do).map_err(D::Error::custom)
    }
}

fn default_minutes() -> u32 {
    480
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct SignInRecipe {
    /// Where a fresh browser goes first. Absolute, http or https.
    pub start_url: String,
    pub steps: Vec<RecipeStep>,
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
pub fn origin_of(url: &str) -> Option<String> {
    let u = url.trim();
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
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", host.to_ascii_lowercase()))
}

fn is_bare_origin(s: &str) -> bool {
    origin_of(s).is_some_and(|o| o == s.trim().trim_end_matches('/').to_ascii_lowercase())
}

fn check(action: &Action) -> Result<(), String> {
    if serde_json::to_value(action).ok().and_then(|v| v["kind"].as_str().map(str::to_string)).as_deref()
        == Some("sign_in")
    {
        return Err("a recipe cannot contain sign_in - it IS the sign-in".to_string());
    }
    action.validate()
}

impl SignInRecipe {
    pub fn validate(&self) -> Result<(), String> {
        match origin_of(&self.start_url) {
            Some(o) if o != "file://" => {}
            _ => return Err("the start address must be a full http or https address".to_string()),
        }
        if self.steps.is_empty() {
            return Err("the recipe has no steps".to_string());
        }
        for (i, step) in self.steps.iter().enumerate() {
            let n = i + 1;
            match step {
                RecipeStep::Do(a) => check(a).map_err(|e| format!("step {n}: {e}"))?,
                RecipeStep::WhenVisible(w) => {
                    if w.within_ms == 0 {
                        return Err(format!("step {n}: within_ms must be more than 0"));
                    }
                    w.selector.validate().map_err(|e| format!("step {n}: {e}"))?;
                    for a in &w.then {
                        check(a).map_err(|e| format!("step {n}: {e}"))?;
                    }
                }
            }
        }
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

    /// Everywhere `navigate` may go: the start address first.
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
    out.trim_matches('-').to_string()
}

/// `acme-corp__web-portal`: safe as a file name, readable in a folder.
pub fn project_slug(org: &str, project: &str) -> String {
    format!("{}__{}", slug_part(org), slug_part(project))
}

fn recipe_path(root: &Path, org: &str, project: &str) -> PathBuf {
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

pub fn save_recipe(root: &Path, org: &str, project: &str, recipe: &SignInRecipe) -> Result<(), String> {
    recipe.validate()?;
    if slug_part(org).is_empty() || slug_part(project).is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    let path = recipe_path(root, org, project);
    std::fs::create_dir_all(path.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(recipe).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}
