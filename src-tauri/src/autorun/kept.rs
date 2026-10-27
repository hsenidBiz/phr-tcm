//! The actions an explore session ran that worked, kept in memory for the
//! assistant that ran them, so a range of them can be saved as a setup.
//!
//! A list belongs to the assistant (its `Caller`) and to its explore session,
//! not to a browser slot: a slot binding can be dropped when an extra browser
//! is freed, and the discovery state is gone once the discovery ends, but the
//! list must outlive both. Nothing here is written to disk and nothing here is
//! a cache: it is the working state of one explore session.
//!
//! Numbers are stable. The first action kept is 1; past `KEPT_MAX` the oldest
//! go and the rest keep the numbers they were given.

use crate::autorun::accounts::Account;
use crate::autorun::agents::Caller;
use crate::browser::actions::Action;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// The most actions one list holds.
pub const KEPT_MAX: usize = 200;

/// The most lists held at once, process-wide; the oldest is dropped.
pub const LISTS_MAX: usize = 16;

/// Said when the caller has no explore session's list to give.
pub const NOT_AN_EXPLORE: &str =
    "actions are kept only in an explore session - start one with start_autorun_discovery and explore: true";

/// One action that worked, as the list holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeptAction {
    /// 1 for the first action kept; never reused or shifted.
    pub n: u32,
    pub action: Action,
    /// For a `use_component`: the steps that actually ran (the expanded
    /// actions, from a draft or the saved component).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<Action>>,
    pub area: Option<String>,
    /// When the action began and ended, in milliseconds since the Unix epoch.
    #[serde(skip)]
    pub started_ms: u64,
    #[serde(skip)]
    pub ended_ms: u64,
}

/// One explore session's kept actions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeptList {
    pub session: String,
    /// The area the discovery started in.
    pub started_area: Option<String>,
    pub actions: Vec<KeptAction>,
    /// How many of the oldest the cap has dropped.
    pub dropped: u32,
}

impl KeptList {
    /// The number the next action kept is given.
    pub fn next(&self) -> u32 {
        self.dropped + u32::try_from(self.actions.len()).unwrap_or(u32::MAX) + 1
    }
}

struct Entry {
    root: PathBuf,
    list: KeptList,
}

#[derive(Default)]
struct Store {
    /// Oldest first.
    lists: Vec<Entry>,
    /// (root, owner) -> the session whose list the owner holds.
    owners: HashMap<(PathBuf, String), String>,
}

fn store() -> MutexGuard<'static, Store> {
    static S: std::sync::OnceLock<Mutex<Store>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(Store::default())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Who owns a list: nothing for the default caller, else the agent's id and
/// its subagent name (empty when it has none).
pub fn owner_of(caller: &Caller) -> String {
    match caller.key() {
        None => String::new(),
        Some(key) => format!("{}:{}", key.id, key.name.as_deref().unwrap_or("")),
    }
}

/// A discovery started for `owner`: its previous list is dropped. With
/// `Some(session)` a new empty list is bound to the owner; with `None` (a
/// plain discovery or a mapping run) none is.
pub fn started(root: &Path, owner: &str, session: Option<&str>, area: Option<&str>) {
    let mut s = store();
    let key = (root.to_path_buf(), owner.to_string());
    if let Some(old) = s.owners.remove(&key) {
        s.lists.retain(|e| !(e.root == key.0 && e.list.session == old));
    }
    let Some(session) = session else {
        return;
    };
    s.lists.retain(|e| !(e.root == key.0 && e.list.session == session));
    s.lists.push(Entry {
        root: key.0.clone(),
        list: KeptList { session: session.to_string(), started_area: area.map(str::to_string), ..KeptList::default() },
    });
    s.owners.insert(key, session.to_string());
    trim(&mut s);
}

/// Drops the oldest lists past `LISTS_MAX`, and the owners that held them.
fn trim(s: &mut Store) {
    while s.lists.len() > LISTS_MAX {
        s.lists.remove(0);
    }
    let held: Vec<(PathBuf, String)> = s.lists.iter().map(|e| (e.root.clone(), e.list.session.clone())).collect();
    s.owners.retain(|(root, _), session| held.iter().any(|(r, id)| r == root && id == session));
}

/// Keeps an action that worked in `session`'s list: the number it was given,
/// or `None` when the action is not kept. The list is made when no `started`
/// came first. Past `KEPT_MAX` the oldest is dropped and counted.
pub fn keep(
    root: &Path,
    session: &str,
    action: &Action,
    steps: Option<Vec<Action>>,
    area: Option<&str>,
    started_ms: u64,
    ended_ms: u64,
) -> Option<u32> {
    let mut s = store();
    let at = match s.lists.iter().position(|e| e.root == root && e.list.session == session) {
        Some(at) => at,
        None => {
            s.lists.push(Entry {
                root: root.to_path_buf(),
                list: KeptList { session: session.to_string(), ..KeptList::default() },
            });
            trim(&mut s);
            s.lists.iter().position(|e| e.root == root && e.list.session == session)?
        }
    };
    let list = &mut s.lists[at].list;
    let n = list.next();
    list.actions.push(KeptAction {
        n,
        action: action.clone(),
        steps,
        area: area.map(str::to_string),
        started_ms,
        ended_ms,
    });
    while list.actions.len() > KEPT_MAX {
        list.actions.remove(0);
        list.dropped += 1;
    }
    Some(n)
}

/// Whether an action, or the steps it ran, types a saved account's username
/// or password, or fills a target whose text names "password". Such an action
/// is never kept.
pub fn types_a_credential(action: &Action, steps: Option<&[Action]>, accounts: &[Account]) -> bool {
    let secrets: Vec<&str> = accounts
        .iter()
        .flat_map(|a| [a.username.trim(), a.password.trim()])
        .filter(|s| !s.is_empty())
        .collect();
    std::iter::once(action).chain(steps.into_iter().flatten()).flat_map(Action::each).any(|a| match a {
        Action::Fill { selector, value } => {
            secrets.contains(&value.trim())
                || serde_json::to_string(selector).is_ok_and(|t| t.to_ascii_lowercase().contains("password"))
        }
        _ => false,
    })
}

/// The list `session` has, if any.
pub fn list_of(root: &Path, session: &str) -> Option<KeptList> {
    store().lists.iter().find(|e| e.root == root && e.list.session == session).map(|e| e.list.clone())
}

/// The list `owner` holds (it outlives the discovery's end), else
/// `NOT_AN_EXPLORE`.
pub fn last_of(root: &Path, owner: &str) -> Result<KeptList, String> {
    let s = store();
    let session = s.owners.get(&(root.to_path_buf(), owner.to_string())).ok_or_else(|| NOT_AN_EXPLORE.to_string())?;
    s.lists
        .iter()
        .find(|e| e.root == root && &e.list.session == session)
        .map(|e| e.list.clone())
        .ok_or_else(|| NOT_AN_EXPLORE.to_string())
}

/// What `get_kept_actions` answers.
pub fn kept_answer(list: &KeptList) -> Value {
    json!({
        "session": list.session,
        "area": list.started_area,
        "actions": list.actions,
        "first": list.actions.first().map(|a| a.n),
        "cut": list.dropped > 0,
    })
}
