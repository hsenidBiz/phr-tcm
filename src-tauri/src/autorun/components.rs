//! Components: reusable steps, kept per project beside the discovery map as
//! `projects/<slug>-components.json`. A script uses one with a
//! `use_component` action naming it; the component's own actions run with
//! the inputs the script passes. Names compare by `nav::module_key`, so
//! case and runs of spaces do not tell two names apart.

use super::recipe::project_slug;
use crate::browser::actions::Action;
use serde::{Deserialize, Serialize};
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
