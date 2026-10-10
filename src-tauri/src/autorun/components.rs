//! Components: reusable steps, kept per project beside the discovery map as
//! `projects/<slug>-components.json`. A script uses one with a
//! `use_component` action naming it; the component's own actions run with
//! the inputs the script passes. Names compare by `nav::module_key`, so
//! case and runs of spaces do not tell two names apart.

use super::recipe::project_slug;
use crate::browser::actions::{Action, ActionOutcome};
use crate::browser::locator::{has_input_placeholder, LocatorStep, Target};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How many changes a component can have before the guide tells the
/// assistant to stop and ask the person instead of changing it again.
pub const CHANGE_CAP: u32 = 3;

#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Text,
    Target,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct ComponentInput {
    pub name: String,
    pub kind: InputKind,
    pub description: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct Component {
    pub name: String,
    pub description: String,
    pub inputs: Vec<ComponentInput>,
    pub actions: Vec<Action>,
    /// When it last ran on the live application, in milliseconds since the
    /// epoch; 0 when never.
    #[serde(default)]
    #[specta(type = f64)]
    pub tried_at: u64,
    /// The area it was tried in.
    #[serde(default)]
    pub tried_area: String,
    #[serde(default)]
    pub version: u32,
    /// How many times it has been changed so far (see `CHANGE_CAP`).
    #[serde(default)]
    pub changes: u32,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct ComponentFile {
    pub components: Vec<Component>,
}

/// Who uses a component: saved scripts, by case id.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct Users {
    pub cases: Vec<i32>,
}

pub fn components_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-components.json", project_slug(org, project)))
}

/// The file as the person can find it under the Auto Run folder; a refusal
/// names this, never a full path.
fn components_file_name(org: &str, project: &str) -> String {
    format!("projects/{}-components.json", project_slug(org, project))
}

/// Every read-change-write of the file holds this.
fn write_lock() -> &'static Mutex<()> {
    static L: Mutex<()> = Mutex::new(());
    &L
}

/// Runs `f` holding the lock every component save holds. A script save
/// that checks against the components wraps the load, the check and its
/// write in this, so a component save (which re-checks the scripts that
/// use it) cannot run in between and miss the new script. `f` must not
/// save a component itself: the lock is not reentrant.
pub fn with_components_locked<T>(f: impl FnOnce() -> T) -> T {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    f()
}

enum ReadError {
    Damaged,
    Unreadable(String),
}

fn read_file(root: &Path, org: &str, project: &str) -> Result<ComponentFile, ReadError> {
    match std::fs::read_to_string(components_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|_| ReadError::Damaged)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ComponentFile::default()),
        Err(e) => Err(ReadError::Unreadable(e.to_string())),
    }
}

/// A missing file is an empty one.
pub fn load_components(root: &Path, org: &str, project: &str) -> Result<ComponentFile, String> {
    read_file(root, org, project).map_err(|e| {
        let file = components_file_name(org, project);
        match e {
            ReadError::Damaged => format!("the components file {file} could not be read; Reset it in Auto Run"),
            ReadError::Unreadable(why) => format!("the components file {file} could not be read: {why}"),
        }
    })
}

fn key(name: &str) -> String {
    super::nav::module_key(name)
}

pub fn find<'a>(f: &'a ComponentFile, name: &str) -> Option<&'a Component> {
    let k = key(name);
    f.components.iter().find(|c| key(&c.name) == k)
}

/// `v` with every object's keys in sorted order, so the same value always
/// writes the same text.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), canonical(&map[k]))).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// What a component does, as a sha256 hex string: its name's key, its
/// inputs and its actions, written as canonical JSON. Its description,
/// version and when it was tried do not count, so a component tried in a
/// discovery is recognised when it is saved unchanged.
pub fn draft_fingerprint(c: &Component) -> String {
    use sha2::{Digest, Sha256};
    let doc = serde_json::json!({ "name_key": key(&c.name), "inputs": c.inputs, "actions": c.actions });
    let text = serde_json::to_string(&canonical(&doc)).unwrap_or_default();
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Load, change and write back under the lock; a change that leaves the
/// file as it was writes nothing, and a change that refuses writes nothing
/// either.
fn update_with<T>(
    root: &Path,
    org: &str,
    project: &str,
    change: impl FnOnce(&mut ComponentFile) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut file = load_components(root, org, project)?;
    let before = file.clone();
    let out = change(&mut file)?;
    if file == before {
        return Ok(out);
    }
    let path = components_path(root, org, project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &text)?;
    Ok(out)
}

/// `update_with`, for a change that cannot refuse.
fn update(root: &Path, org: &str, project: &str, change: impl FnOnce(&mut ComponentFile)) -> Result<(), String> {
    update_with(root, org, project, |f| {
        change(f);
        Ok(())
    })
}

/// `c` in `f`, replacing the component of the same name (by key).
fn put_in(f: &mut ComponentFile, c: Component) {
    let k = key(&c.name);
    match f.components.iter().position(|x| key(&x.name) == k) {
        Some(i) => f.components[i] = c,
        None => f.components.push(c),
    }
}

/// Saves `c`, replacing the component of the same name (by key).
pub fn put(root: &Path, org: &str, project: &str, c: Component) -> Result<(), String> {
    update(root, org, project, |f| put_in(f, c))
}

pub fn remove(root: &Path, org: &str, project: &str, name: &str) -> Result<(), String> {
    update(root, org, project, |f| {
        let k = key(name);
        f.components.retain(|x| key(&x.name) != k);
    })
}

/// Move a damaged file aside as `<slug>-components.corrupt-<now>.json`
/// beside it so an empty one starts; never deleted. Hands back the
/// project-relative name it was moved to. A file that reads is refused.
pub fn reset_components(root: &Path, org: &str, project: &str) -> Result<String, String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let from = components_path(root, org, project);
    let file = components_file_name(org, project);
    if !from.exists() {
        return Err(format!("There is no components file {file}, so there is nothing to reset."));
    }
    if read_file(root, org, project).is_ok() {
        return Err(format!("The components file {file} can be read, so there is nothing to reset."));
    }
    let aside = format!("{}-components.corrupt-{}.json", project_slug(org, project), super::sessions::now_ms());
    std::fs::rename(&from, from.with_file_name(&aside))
        .map_err(|e| format!("The components file {file} could not be moved aside: {e}"))?;
    crate::applog::info(format!("Components: a damaged file was moved aside as projects/{aside}"));
    Ok(format!("projects/{aside}"))
}

fn uses(actions: &[Action], k: &str) -> bool {
    actions
        .iter()
        .flat_map(|a| a.each())
        .any(|a| matches!(a, Action::UseComponent { component, .. } if key(component) == k))
}

/// The saved scripts whose steps use `name`, a use inside a `when_visible`
/// included, in no set order. Scripts that do not read are skipped (and
/// logged by `store::list_scripts`).
fn scripts_using(root: &Path, name: &str) -> Vec<super::CaseScript> {
    let k = key(name);
    super::store::list_scripts(root)
        .into_iter()
        .filter(|s| s.steps.iter().any(|st| uses(&st.actions, &k)))
        .collect()
}

/// The saved scripts whose steps use `name`, by case id (`scripts_using`).
pub fn users_of(root: &Path, name: &str) -> Users {
    let mut cases: Vec<i32> = scripts_using(root, name).into_iter().map(|s| s.case_id).collect();
    cases.sort_unstable();
    Users { cases }
}

/// One `use_component` a step ran: the component's saved name and the
/// version it had then, kept on the step's record.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq, Eq)]
pub struct ComponentUse {
    pub name: String,
    pub version: u32,
}

/// Said when a script names a component this project does not have.
pub fn not_saved(name: &str) -> String {
    format!("{} is not saved in this project", name.trim())
}

fn needs(c: &Component, input: &str) -> String {
    format!("{} needs {input}", c.name)
}

/// The caller's value for a target input, as the links it stands for: a
/// locator object or a list of them, never a legacy string and never a
/// placeholder itself.
fn target_links(c: &Component, name: &str, v: &Value) -> Result<Vec<LocatorStep>, String> {
    let wrong = || format!("{} to be a locator", needs(c, name));
    if !(v.is_object() || v.is_array()) {
        return Err(wrong());
    }
    let t: Target = serde_json::from_value(v.clone()).map_err(|_| wrong())?;
    if t.validate().is_err() || has_input_placeholder(&t) {
        return Err(wrong());
    }
    Ok(t.links())
}

/// `{{name}}` in `s` replaced by the text input of that name, in one pass
/// (`expand` refuses a value holding `{{` or `}}`); a name that is not a
/// text input is left alone.
fn fill_text(s: &str, text: &BTreeMap<&str, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        match after.find("}}").and_then(|end| text.get(after[..end].trim()).map(|v| (end, v))) {
            Some((end, v)) => {
                out.push_str(v);
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn fill_text_in(v: &mut Value, text: &BTreeMap<&str, String>) {
    match v {
        Value::String(s) => *s = fill_text(s, text),
        Value::Array(items) => items.iter_mut().for_each(|x| fill_text_in(x, text)),
        Value::Object(map) => map.values_mut().for_each(|x| fill_text_in(x, text)),
        _ => {}
    }
}

/// `t` with each placeholder link replaced by its target input's links,
/// spliced in place: one link stays a single locator, more make a chain.
fn put_targets(t: &mut Target, links: &BTreeMap<&str, Vec<LocatorStep>>, c: &Component) -> Result<(), String> {
    if !has_input_placeholder(t) {
        return Ok(());
    }
    let mut out: Vec<LocatorStep> = Vec::new();
    for step in t.links() {
        match step.input.as_deref().map(str::trim) {
            None => out.push(step),
            Some(name) => out.extend(links.get(name).ok_or_else(|| needs(c, name))?.iter().cloned()),
        }
    }
    *t = if out.len() == 1 { Target::One(out.remove(0)) } else { Target::Chain(out) };
    Ok(())
}

/// The component's actions with `inputs` put in: each text input where its
/// `{{name}}` is written in a value, then each target input in place of the
/// locator link that names it. Every declared input must be given. The
/// caller's locators go in last, so nothing in them is read as a
/// placeholder.
pub fn expand(c: &Component, inputs: &serde_json::Map<String, Value>) -> Result<Vec<Action>, String> {
    let mut text: BTreeMap<&str, String> = BTreeMap::new();
    let mut links: BTreeMap<&str, Vec<LocatorStep>> = BTreeMap::new();
    for input in &c.inputs {
        let name = input.name.trim();
        let v = inputs.get(name).filter(|v| !v.is_null()).ok_or_else(|| needs(c, name))?;
        match input.kind {
            InputKind::Text => {
                let s = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => return Err(format!("{} to be text", needs(c, name))),
                };
                // Put into a locator, "{{day}}" would leave it reading as
                // the component wrote it, past the seen check. A data
                // placeholder ("{{setup.cycle_id}}") is the script's: the
                // run fills it in before the component expands, the save
                // checks the locator it makes by its shape, and the run
                // checks the value it took
                // (`seen_check::check_resolved_inputs`).
                if (s.contains("{{") || s.contains("}}")) && !super::seen_check::only_data_placeholders(&s) {
                    return Err(format!("{} got a placeholder as {name}", c.name));
                }
                text.insert(name, s);
            }
            InputKind::Target => {
                links.insert(name, target_links(c, name, v)?);
            }
        }
    }
    let unreadable = |e: serde_json::Error| format!("{} could not be read: {e}", c.name);
    let mut out = Vec::with_capacity(c.actions.len());
    for action in &c.actions {
        let mut a = action.clone();
        if !text.is_empty() {
            let mut v = serde_json::to_value(&a).map_err(unreadable)?;
            fill_text_in(&mut v, &text);
            a = serde_json::from_value(v).map_err(unreadable)?;
        }
        for t in a.targets_mut() {
            put_targets(t, &links, c)?;
        }
        out.push(a);
    }
    Ok(out)
}

fn uses_any(actions: &[Action]) -> bool {
    actions.iter().flat_map(|a| a.each()).any(|a| matches!(a, Action::UseComponent { .. }))
}

/// One action of a step with every `use_component` in it expanded, a
/// `when_visible`'s guarded ones included (the `when_visible` stays one
/// action, so it is not tagged).
fn expand_one(
    file: &ComponentFile,
    action: &Action,
    uses: &mut Vec<ComponentUse>,
) -> Result<Vec<(Action, Option<String>)>, String> {
    match action {
        Action::UseComponent { component, inputs } => {
            let c = find(file, component).ok_or_else(|| not_saved(component))?;
            let actions = expand(c, inputs)?;
            uses.push(ComponentUse { name: c.name.clone(), version: c.version });
            Ok(actions.into_iter().map(|a| (a, Some(c.name.clone()))).collect())
        }
        Action::WhenVisible { selector, within_ms, then } if uses_any(then) => {
            let mut inner = Vec::with_capacity(then.len());
            for a in then {
                inner.extend(expand_one(file, a, uses)?.into_iter().map(|(a, _)| a));
            }
            Ok(vec![(Action::WhenVisible { selector: selector.clone(), within_ms: *within_ms, then: inner }, None)])
        }
        other => Ok(vec![(other.clone(), None)]),
    }
}

/// A step's actions as they run: each `use_component` replaced by its
/// component's actions with the inputs put in, each tagged with the
/// component's name, and every use in order with the version it had. The
/// components file is read only when the step uses one, so a step without
/// any never depends on it. An error says which of the step's actions it
/// belongs to, by index: the use that could not be expanded (the first
/// use, when the file itself could not be read).
#[allow(clippy::type_complexity)]
pub fn expand_step(
    root: &Path,
    org: &str,
    project: &str,
    actions: &[Action],
) -> Result<(Vec<(Action, Option<String>)>, Vec<ComponentUse>), (usize, String)> {
    let Some(first) = actions.iter().position(|a| uses_any(std::slice::from_ref(a))) else {
        return Ok((actions.iter().cloned().map(|a| (a, None)).collect(), Vec::new()));
    };
    let file = load_components(root, org, project).map_err(|why| (first, why))?;
    let mut uses = Vec::new();
    let mut out = Vec::with_capacity(actions.len());
    for (i, a) in actions.iter().enumerate() {
        out.extend(expand_one(&file, a, &mut uses).map_err(|why| (i, why))?);
    }
    Ok((out, uses))
}

/// What one recorded outcome of a step ran, read back after the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    /// The action, when it can still be named: the script's own, or the
    /// component's as it expands now. `None` when the script or the
    /// component has changed since the run.
    pub action: Option<Action>,
    /// The component the outcome came from.
    pub component: Option<String>,
}

/// A step's recorded `outcomes` paired with the actions that ran them: the
/// script step's `actions` with each `use_component` expanded the way the
/// runner expands it, from `file` as it is now. A component that is gone,
/// that is not the version the step recorded in `used`, or that now
/// expands to a different number of actions than ran, leaves its outcomes
/// named by their `component` only (the readers say the component changed
/// since the run). A run with no component outcome pairs by position, as
/// it always has.
pub fn ran_actions(actions: &[Action], outcomes: &[ActionOutcome], file: &ComponentFile, used: &[ComponentUse]) -> Vec<Ran> {
    if outcomes.iter().all(|o| o.component.is_none()) {
        return (0..outcomes.len()).map(|i| Ran { action: actions.get(i).cloned(), component: None }).collect();
    }
    let tag = |o: &ActionOutcome| o.component.as_deref().map(key);
    let mut out: Vec<Ran> = Vec::with_capacity(outcomes.len());
    let mut script = actions.iter().peekable();
    while out.len() < outcomes.len() {
        let j = out.len();
        let Some(a) = script.next() else {
            out.push(Ran { action: None, component: outcomes[j].component.clone() });
            continue;
        };
        let Action::UseComponent { component, inputs } = a else {
            out.push(Ran { action: Some(a.clone()), component: None });
            continue;
        };
        let k = key(component);
        let ran = outcomes[j..].iter().take_while(|o| tag(o).as_deref() == Some(k.as_str())).count();
        if ran == 0 {
            // Not what ran here: the script has changed since the run.
            continue;
        }
        let name = outcomes[j].component.clone();
        let again = matches!(script.peek(), Some(Action::UseComponent { component: next, .. }) if key(next) == k);
        // A component whose version is not the one the step recorded is
        // not what ran, however many actions it has now.
        let now = find(file, component)
            .filter(|c| !used.iter().any(|u| key(&u.name) == k && u.version != c.version))
            .and_then(|c| expand(c, inputs).ok())
            .filter(|ex| !ex.is_empty() && (ex.len() == ran || (again && ex.len() < ran)));
        match now {
            Some(ex) => out.extend(ex.into_iter().map(|a| Ran { action: Some(a), component: name.clone() })),
            None => out.extend((0..ran).map(|_| Ran { action: None, component: name.clone() })),
        }
    }
    out
}

// ---- saving and removing ----

/// Said when a component is saved that the open discovery has not seen
/// work, exactly as it is sent.
pub const TRY_IT_FIRST: &str = "Try the component live in discovery first: run a use_component of it with discover_autorun_action, sending this component as its draft, and save it once that works, unchanged.";

/// The open discovery, as a component save reads it: its area, and the
/// `draft_fingerprint`s of the components it tried that worked.
#[derive(Debug, Clone, Copy)]
pub struct TriedIn<'a> {
    pub area: Option<&'a str>,
    pub tried: &'a [String],
}

/// The test cases of the saved scripts that use a component, read from
/// Azure DevOps before a save. Scripts are kept by case id, not by
/// project, so the cases say which are this project's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserCases {
    /// This project's cases, by id, with their text: each step's action
    /// then its expected result.
    pub here: BTreeMap<i32, Vec<String>>,
    /// The cases found in no case of this project: another project's or
    /// another organization's, or no longer there. Their scripts do not
    /// use this project's components.
    pub elsewhere: Vec<i32>,
}

/// Said when a component that saved scripts use is saved with no way to
/// read their test cases: the check of those scripts is never skipped.
pub const SIGN_IN_TO_CHECK_USERS: &str = "Sign in so the scripts that use this component can be checked.";

/// Every saved script of this project that uses `name`, checked against
/// `after` (the file with the version being saved), the way its own save
/// was checked: each locator an input goes into, and that each use still
/// expands. Refused with every break, one line each, case by case. A
/// script whose case is neither here nor elsewhere in `cases` began using
/// the component after the cases were read, and refuses the save too.
fn check_users(
    root: &Path,
    map: &super::discovery_map::DiscoveryMap,
    after: &ComponentFile,
    name: &str,
    cases: Option<&UserCases>,
) -> Result<(), String> {
    let mut users = scripts_using(root, name);
    if users.is_empty() {
        return Ok(());
    }
    let Some(cases) = cases else {
        return Err(SIGN_IN_TO_CHECK_USERS.to_string());
    };
    users.sort_by_key(|s| s.case_id);
    let mut broken: Vec<String> = Vec::new();
    for script in &users {
        if cases.elsewhere.contains(&script.case_id) {
            continue;
        }
        let Some(text) = cases.here.get(&script.case_id) else {
            return Err(format!(
                "Case {} began using {name} while this was checked: save the component again.",
                script.case_id
            ));
        };
        for u in super::seen_check::check_component_uses(map, after, script, text, name) {
            broken.push(format!("Case {}, {}", script.case_id, u.broken_by_change()));
        }
    }
    if broken.is_empty() {
        Ok(())
    } else {
        Err(broken.join("\n"))
    }
}

/// What a save answers.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Saved {
    /// The component's name.
    pub saved: String,
    pub version: u32,
    pub changes: u32,
    /// The component has been changed `CHANGE_CAP` times or more: the
    /// a person looking at it.
    pub cap_reached: bool,
}

/// Said when a component's string has a `{{` with no `}}` after it, or a
/// `}}` with no `{{` before it.
pub const UNCLOSED: &str = "A component has an unclosed {{ placeholder.";

/// Every `{{x}}` written in `v`'s strings, by its trimmed name, read the
/// way `fill_text` reads them; `Err` when a string holds a `{{` or a `}}`
/// that is not part of a complete placeholder.
fn text_placeholders(v: &Value, out: &mut Vec<String>) -> Result<(), ()> {
    match v {
        Value::String(s) => {
            let mut rest = s.as_str();
            loop {
                match (rest.find("{{"), rest.find("}}")) {
                    (None, None) => return Ok(()),
                    (Some(open), Some(close)) if open < close => {
                        out.push(rest[open + 2..close].trim().to_string());
                        rest = &rest[close + 2..];
                    }
                    _ => return Err(()),
                }
            }
        }
        Value::Array(items) => items.iter().try_for_each(|x| text_placeholders(x, out)),
        Value::Object(map) => map.values().try_for_each(|x| text_placeholders(x, out)),
        _ => Ok(()),
    }
}

/// Does `v` hold a complete `{{x}}` text placeholder in any of its
/// strings, read the way `fill_text` reads one?
pub fn holds_text_placeholder(v: &Value) -> bool {
    let mut found = Vec::new();
    text_placeholders(v, &mut found).is_ok() && !found.is_empty()
}

/// A name a component may never type: a script signs in as its account.
fn is_credential(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "username" | "password")
}

/// The save rules a component is held to on its own, with no file, map or
/// discovery: a name and a description; inputs declared once each, every
/// one used and nothing used undeclared (`{{x}}` for a text input,
/// `{"input": "x"}` for a target one); no sign-in, username or password;
/// no other component; no address an input makes; and every action valid.
pub fn check_component(c: &Component) -> Result<(), String> {
    let name = c.name.trim();
    if name.is_empty() {
        return Err("A component needs a name.".to_string());
    }
    if c.description.trim().is_empty() {
        return Err(format!("{name} needs a description."));
    }
    if c.actions.is_empty() {
        return Err(format!("{name} needs at least one action."));
    }

    let mut declared: Vec<(&str, InputKind)> = Vec::new();
    for input in &c.inputs {
        let n = input.name.trim();
        if n.is_empty() {
            return Err(format!("{name} declares an input with no name."));
        }
        if declared.iter().any(|(d, _)| *d == n) {
            return Err(format!("{name} declares the input {n} more than once."));
        }
        declared.push((n, input.kind));
    }
    let kind_of = |n: &str| declared.iter().find(|(d, _)| *d == n).map(|(_, k)| *k);
    let every: Vec<&Action> = c.actions.iter().flat_map(Action::each).collect();

    let mut texts: Vec<String> = Vec::new();
    for a in &c.actions {
        text_placeholders(&serde_json::to_value(a).unwrap_or(Value::Null), &mut texts)
            .map_err(|()| UNCLOSED.to_string())?;
    }
    let mut targets: Vec<String> = Vec::new();
    for a in &every {
        for t in a.targets() {
            targets.extend(t.links().into_iter().filter_map(|l| l.input.map(|i| i.trim().to_string())));
        }
    }
    // A username or password is refused below, declared or not.
    if let Some(x) = texts.iter().find(|x| !is_credential(x) && kind_of(x) != Some(InputKind::Text)) {
        return Err(format!("{name} uses {{{{{x}}}}} but declares no text input {x}."));
    }
    if let Some(x) = targets.iter().find(|x| kind_of(x) != Some(InputKind::Target)) {
        return Err(format!("{name} uses {{\"input\": \"{x}\"}} but declares no target input {x}."));
    }
    if let Some((x, _)) = declared.iter().find(|(d, _)| !texts.iter().chain(&targets).any(|u| u == d)) {
        return Err(format!("{name} declares the input {x} but never uses it."));
    }

    if every.iter().any(|a| matches!(a, Action::SignIn { .. })) {
        return Err("A component cannot sign in: a script signs in as its account.".to_string());
    }
    if texts.iter().any(|x| is_credential(x)) {
        return Err("A component cannot type a username or password: a script signs in as its account.".to_string());
    }
    if every.iter().any(|a| matches!(a, Action::UseComponent { .. })) {
        return Err("A component cannot use another component.".to_string());
    }
    if every.iter().any(|a| matches!(a, Action::Navigate { url } | Action::OpenTab { url, .. } if url.contains("{{"))) {
        return Err("A component's address cannot come from an input.".to_string());
    }
    for (i, a) in c.actions.iter().enumerate() {
        a.validate().map_err(|why| format!("{name}, action {}: {why}", i + 1))?;
    }
    Ok(())
}

/// Saves `draft` under every save rule, in order: `check_component`; a
/// discovery going (`session`); its fixed locators seen in that
/// discovery's area (`check_component_seen`); tried in that discovery
/// exactly as it is sent; when a
/// component of that name is saved already, a reason (`why`) and no check
/// lost (`edits::weakens`), which makes it the next version and one more
/// change toward `CHANGE_CAP`; and, saved already or not, every saved
/// script of this project that uses the name still seen as it would run
/// (`check_users`, with `cases`, the users' test cases; `None` when they
/// could not be read). A change past the cap still saves: `cap_reached`
/// says the assistant should stop and ask the person. `now` is when it
/// was tried, in milliseconds since the epoch.
#[allow(clippy::too_many_arguments)]
pub fn save_tried(
    root: &Path,
    org: &str,
    project: &str,
    draft: Component,
    why: Option<&str>,
    session: Option<TriedIn<'_>>,
    now: u64,
    cases: Option<&UserCases>,
) -> Result<Saved, String> {
    tried(root, org, project, draft, why, session, now, cases, true)
}

/// [`save_tried`]'s every rule, in the same order, and what it would
/// answer, without writing anything: the components file is read, never
/// written, and its lock is not taken.
#[allow(clippy::too_many_arguments)]
pub fn check_tried(
    root: &Path,
    org: &str,
    project: &str,
    draft: Component,
    why: Option<&str>,
    session: Option<TriedIn<'_>>,
    now: u64,
    cases: Option<&UserCases>,
) -> Result<Saved, String> {
    tried(root, org, project, draft, why, session, now, cases, false)
}

/// [`save_tried`], or with `write` false [`check_tried`].
#[allow(clippy::too_many_arguments)]
fn tried(
    root: &Path,
    org: &str,
    project: &str,
    draft: Component,
    why: Option<&str>,
    session: Option<TriedIn<'_>>,
    now: u64,
    cases: Option<&UserCases>,
    write: bool,
) -> Result<Saved, String> {
    check_component(&draft)?;
    // With no discovery going, nothing was tried, and there is no area to
    // check the locators in.
    let Some(session) = session else {
        return Err(TRY_IT_FIRST.to_string());
    };
    let area = session.area.map(str::trim).filter(|a| !a.is_empty());
    let map = super::seen_check::load_checked_map(root, org, project)?;
    super::seen_check::check_component_seen(&map, area, &draft.actions)?;
    if !session.tried.contains(&draft_fingerprint(&draft)) {
        return Err(TRY_IT_FIRST.to_string());
    }
    let name = draft.name.trim().to_string();
    // The saved one is read, compared and replaced under one lock, so two
    // saves can never both become the same next version.
    let change = |file: &mut ComponentFile| {
        let (version, changes) = match find(file, &name) {
            None => (1, 0),
            Some(old) => {
                if why.is_none_or(|w| w.trim().is_empty()) {
                    return Err(format!("{name} is already saved: say why it changes in \"why\"."));
                }
                if let Some(weaker) = super::edits::weakens(&old.actions, &draft.actions) {
                    return Err(format!("{name} {weaker}"));
                }
                (old.version + 1, old.changes + 1)
            }
        };
        let saved = Component {
            name: name.clone(),
            description: draft.description.trim().to_string(),
            tried_at: now,
            tried_area: area.unwrap_or("").to_string(),
            version,
            changes,
            ..draft
        };
        // Every script that uses the name, a fresh save's included, is
        // checked against what it would now run.
        let mut after = file.clone();
        put_in(&mut after, saved.clone());
        check_users(root, &map, &after, &name, cases)?;
        put_in(file, saved);
        Ok(Saved { saved: name.clone(), version, changes, cap_reached: changes >= CHANGE_CAP })
    };
    if write {
        update_with(root, org, project, change)
    } else {
        change(&mut load_components(root, org, project)?)
    }
}

/// Removes `name` unless a saved script uses it, handing back its saved
/// name; the refusal names those scripts by case id.
/// The in-use check and the removal run under one lock.
pub fn remove_unused(root: &Path, org: &str, project: &str, name: &str) -> Result<String, String> {
    update_with(root, org, project, |file| {
        let saved = find(file, name).ok_or_else(|| not_saved(name))?.name.clone();
        match users_of(root, &saved).cases.as_slice() {
            [] => {
                let k = key(&saved);
                file.components.retain(|x| key(&x.name) != k);
                Ok(saved)
            }
            [one] => Err(format!("{saved} is used by case {one}: change that script first.")),
            many => {
                let ids = many.iter().map(i32::to_string).collect::<Vec<_>>().join(", ");
                Err(format!("{saved} is used by cases {ids}: change those scripts first."))
            }
        }
    })
}

/// The live guide's list of this project's components: each one's name,
/// its inputs as `name: kind`, and its description, then a line for each
/// one changed `CHANGE_CAP` times or more. Empty with none saved.
pub fn guide_section(file: &ComponentFile) -> String {
    if file.components.is_empty() {
        return String::new();
    }
    let mut out = String::from("## This project's components\n\n");
    out.push_str("Use one with a use_component action instead of repeating its actions (see \"Components\").\n\n");
    for c in &file.components {
        let inputs = if c.inputs.is_empty() {
            "no inputs".to_string()
        } else {
            c.inputs
                .iter()
                .map(|i| {
                    let kind = match i.kind {
                        InputKind::Text => "text",
                        InputKind::Target => "target",
                    };
                    format!("{}: {kind}", i.name.trim())
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let description = c.description.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("- {} ({inputs}): {description}\n", c.name.trim()));
    }
    let capped: Vec<&Component> = file.components.iter().filter(|c| c.changes >= CHANGE_CAP).collect();
    if !capped.is_empty() {
        out.push('\n');
        for c in capped {
            out.push_str(&format!(
                "{} has had {} accepted changes: stop and report to the person before changing it again.\n",
                c.name.trim(),
                c.changes
            ));
        }
    }
    out
}
