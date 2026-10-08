//! Components: reusable steps, kept per project beside the discovery map as
//! `projects/<slug>-components.json`. A script uses one with a
//! `use_component` action naming it; the component's own actions run with
//! the inputs the script passes. Names compare by `nav::module_key`, so
//! case and runs of spaces do not tell two names apart.

use super::recipe::project_slug;
use crate::browser::actions::Action;
use crate::browser::locator::{has_input_placeholder, LocatorStep, Target};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How many times a component may be changed by an assistant before a
/// person has to look at it.
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
    /// Changes made since a person last saved it (see `CHANGE_CAP`).
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

/// Load, change and write back under the lock; a change that leaves the
/// file as it was writes nothing.
fn update(root: &Path, org: &str, project: &str, change: impl FnOnce(&mut ComponentFile)) -> Result<(), String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut file = load_components(root, org, project)?;
    let before = file.clone();
    change(&mut file);
    if file == before {
        return Ok(());
    }
    let path = components_path(root, org, project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&path, &text)
}

/// Saves `c`, replacing the component of the same name (by key).
pub fn put(root: &Path, org: &str, project: &str, c: Component) -> Result<(), String> {
    update(root, org, project, |f| {
        let k = key(&c.name);
        match f.components.iter().position(|x| key(&x.name) == k) {
            Some(i) => f.components[i] = c,
            None => f.components.push(c),
        }
    })
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
/// included. Scripts that do not read are skipped.
pub fn users_of(root: &Path, name: &str) -> Users {
    let k = key(name);
    let mut cases: Vec<i32> = super::store::list_scripts(root)
        .into_iter()
        .filter(|s| s.steps.iter().any(|st| uses(&st.actions, &k)))
        .map(|s| s.case_id)
        .collect();
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

/// `{{name}}` in `s` replaced by the text input of that name, in one pass:
/// a value that itself reads like a placeholder is put in as it is, and a
/// name that is not a text input is left alone.
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
/// any never depends on it.
#[allow(clippy::type_complexity)]
pub fn expand_step(
    root: &Path,
    org: &str,
    project: &str,
    actions: &[Action],
) -> Result<(Vec<(Action, Option<String>)>, Vec<ComponentUse>), String> {
    if !uses_any(actions) {
        return Ok((actions.iter().cloned().map(|a| (a, None)).collect(), Vec::new()));
    }
    let file = load_components(root, org, project)?;
    let mut uses = Vec::new();
    let mut out = Vec::with_capacity(actions.len());
    for a in actions {
        out.extend(expand_one(&file, a, &mut uses)?);
    }
    Ok((out, uses))
}
