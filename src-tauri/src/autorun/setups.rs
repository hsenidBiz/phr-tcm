//! Saved setups: a named range of the actions an explore session kept, with
//! a find rule, so a record a test needs can be handed back (an existing test
//! record) or built (in an explore session) days later, by name.
//!
//! The file is `projects/<slug>-setups.json` under the Auto Run folder, with
//! its own write lock and never the components file. A setup holds actions,
//! template names, a find rule, and for each create it made only the request
//! line (`METHOD /path`) and where the id sat in the answer. It never holds a
//! password, a login, a host, an address with a query string or a cookie: a
//! kept action that typed a login is never kept, a `sign_in` is refused, and
//! an address outside the sign-in recipe's sites, or one that carries a query
//! string, is refused at save (and an `api_request` with a query too).

use super::explore::{Change, ExploreSession};
use super::kept::{types_a_credential, KeptList};
use super::nav;
use super::recipe::project_slug;
use super::setup_find::{browser_with_prefix, check_find, starts_with_prefix, FindRule};
use crate::browser::actions::Action;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Said when a range made no create the app verified.
pub const NO_CREATE_IN_RANGE: &str = "These actions made no create the app verified: record what these actions create first, with record_explore_change.";

/// Said when a record a setup would build is not named with the test prefix.
pub const ONLY_TEST_BUILDS: &str = "A setup must build only records named with the test prefix.";

/// How far past the last action a save may have started and still belong to
/// it: the page's save follows the click that sent it.
const SAVE_GRACE_MS: u64 = 2000;

/// The shortest value an input may stand for.
const INPUT_MIN_CHARS: usize = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SetupInput {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub names_the_record: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SetupCreate {
    pub kind: String,
    /// The recorded name, with the inputs' values as `{{name}}` templates.
    pub name: String,
    /// `METHOD /path[?handler=]`.
    pub request: String,
    /// A JSON pointer to the id in the save's answer.
    pub id_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedSetup {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub area: Option<String>,
    pub inputs: Vec<SetupInput>,
    pub actions: Vec<Action>,
    pub find: FindRule,
    pub creates: Vec<SetupCreate>,
    /// Index into `creates`.
    pub returns: usize,
    pub version: u32,
    pub changes: u32,
    pub saved_at: u64,
    #[serde(default)]
    pub last_used_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SetupFile {
    pub setups: Vec<SavedSetup>,
}

pub fn setups_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}-setups.json", project_slug(org, project)))
}

/// The file as the person can find it under the Auto Run folder; a refusal
/// names this, never a full path.
fn setups_file_name(org: &str, project: &str) -> String {
    format!("projects/{}-setups.json", project_slug(org, project))
}

fn unreadable(org: &str, project: &str) -> String {
    format!("the setups file {} could not be read", setups_file_name(org, project))
}

/// Every read-change-write of the file holds this.
fn write_lock() -> &'static Mutex<()> {
    static L: Mutex<()> = Mutex::new(());
    &L
}

/// A missing file is an empty one; a file that does not read is an error
/// that names it, and nothing ever overwrites it.
pub fn load_setups(root: &Path, org: &str, project: &str) -> Result<SetupFile, String> {
    match std::fs::read_to_string(setups_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|_| unreadable(org, project))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SetupFile::default()),
        Err(_) => Err(unreadable(org, project)),
    }
}

/// Load, change and write back under the lock; a change that leaves the
/// file as it was writes nothing, and a change that refuses writes nothing
/// either.
fn update_with<T>(
    root: &Path,
    org: &str,
    project: &str,
    change: impl FnOnce(&mut SetupFile) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut file = load_setups(root, org, project)?;
    let before = file.clone();
    let out = change(&mut file)?;
    if file == before {
        return Ok(out);
    }
    let path = setups_path(root, org, project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    crate::autorun::roots::write_file(root, &path, &text)?;
    Ok(out)
}

fn key(name: &str) -> String {
    nav::module_key(name)
}

pub fn find_setup<'a>(f: &'a SetupFile, name: &str) -> Option<&'a SavedSetup> {
    let k = key(name);
    f.setups.iter().find(|s| key(&s.name) == k)
}

pub fn no_such_setup(name: &str) -> String {
    format!("there is no setup called \"{}\" - list_setups lists them", name.trim())
}

/// A value for an input that must be unique: the test prefix, the setup's
/// name as a slug, and 7 base-36 characters of the clock.
pub fn fresh_value(prefix: &str, setup: &str, now_ms: u64) -> String {
    let mut slug = String::new();
    let mut gap = false;
    for c in setup.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !slug.is_empty() {
                slug.push('-');
            }
            gap = false;
            slug.push(c);
        } else {
            gap = true;
        }
    }
    slug.truncate(30);
    let slug = slug.trim_end_matches('-');
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut stamp = now_ms % 36u64.pow(7);
    let mut tail = [b'0'; 7];
    for slot in tail.iter_mut().rev() {
        *slot = DIGITS[usize::try_from(stamp % 36).unwrap_or(0)];
        stamp /= 36;
    }
    format!("{prefix}{slug}-{}", String::from_utf8_lossy(&tail))
}

// ------------------------------------------------------------- the save ask

#[derive(Deserialize, Debug, Clone)]
pub struct InputAsked {
    pub action: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub names_the_record: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ReturnsAsked {
    pub entry: u32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct SaveAsked {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub from: u32,
    #[serde(default)]
    pub to: u32,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub inputs: Vec<InputAsked>,
    #[serde(default)]
    pub find: FindRule,
    #[serde(default)]
    pub returns: Option<ReturnsAsked>,
    #[serde(default)]
    pub why: Option<String>,
    /// Remove the setup called `name` instead of saving one.
    #[serde(default)]
    pub remove: bool,
}

/// What a save is checked against.
pub struct SaveFrom<'a> {
    pub kept: &'a KeptList,
    pub session: &'a ExploreSession,
    pub prefix: &'a str,
    /// `policy.allows(url)`: an address inside the sign-in recipe's sites.
    pub allowed: &'a dyn Fn(&str) -> bool,
    /// Whether the map has seen these actions' locators in this area.
    pub seen: &'a dyn Fn(Option<&str>, &[Action]) -> Result<(), String>,
    pub area_known: &'a dyn Fn(&str) -> bool,
    /// The saved accounts: no action a setup keeps types one's login.
    pub accounts: &'a [crate::autorun::accounts::Account],
    pub now: u64,
}

/// Where an input's value was replaced by its template.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replaced {
    pub input: String,
    /// The kept action's number.
    pub action: u32,
    /// Dotted path in the action's JSON, e.g. `value`, `selector.name`.
    pub field: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub setup: SavedSetup,
    pub replaced: Vec<Replaced>,
    /// The kept list had lost its oldest actions to the cap.
    pub cut: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetupSaved {
    pub saved: String,
    pub version: u32,
    pub changes: u32,
}

// ------------------------------------------------------------ build_setup

fn is_login_name(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "username" | "password")
}

fn valid_setup_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=60).contains(&n) && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
}

fn valid_input_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=40).contains(&n) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `s` with each input's value (longest first, in one left-to-right pass, so
/// a template just written is never matched again) replaced by `{{name}}`.
/// The indexes of the inputs that matched.
fn replace_values(s: &str, inputs: &[(String, String)]) -> (String, Vec<usize>) {
    let mut order: Vec<usize> = (0..inputs.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(inputs[*i].1.len()));
    let (mut out, mut hit) = (String::with_capacity(s.len()), Vec::new());
    let mut at = 0;
    while at < s.len() {
        let rest = &s[at..];
        match order.iter().find(|i| !inputs[**i].1.is_empty() && rest.starts_with(inputs[**i].1.as_str())) {
            Some(i) => {
                out.push_str(&format!("{{{{{}}}}}", inputs[*i].0));
                if !hit.contains(i) {
                    hit.push(*i);
                }
                at += inputs[*i].1.len();
            }
            None => {
                let c = rest.chars().next().unwrap_or(' ');
                out.push(c);
                at += c.len_utf8();
            }
        }
    }
    hit.sort_unstable();
    (out, hit)
}

fn replace_in_value(
    v: &mut Value,
    path: &mut Vec<String>,
    inputs: &[(String, String)],
    n: u32,
    out: &mut Vec<Replaced>,
) {
    match v {
        Value::String(s) => {
            let (text, hit) = replace_values(s, inputs);
            if !hit.is_empty() {
                *s = text;
                for i in hit {
                    out.push(Replaced { input: inputs[i].0.clone(), action: n, field: path.join(".") });
                }
            }
        }
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                // The action's own tag is not a value anyone typed.
                if k == "kind" {
                    continue;
                }
                path.push(k.clone());
                replace_in_value(val, path, inputs, n, out);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, val) in items.iter_mut().enumerate() {
                path.push(i.to_string());
                replace_in_value(val, path, inputs, n, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Whether a create's name template comes out named with the test prefix: it
/// starts with the prefix itself, or with the value of an input that is
/// `unique` or names the record (those values are prefix-checked at save and
/// at run).
fn named_with_prefix(template: &str, inputs: &[SetupInput], prefix: &str) -> bool {
    if starts_with_prefix(template, prefix) {
        return true;
    }
    let t = template.trim_start();
    inputs.iter().any(|i| (i.unique || i.names_the_record) && t.starts_with(&format!("{{{{{}}}}}", i.name)))
}

/// Every problem with one action a setup would keep, in a build or a find
/// rule. `what` names it ("Action 3"). The same checks both ways: no sign-in,
/// no component, only the recipe's sites and never a local file or an address
/// with a query string, no `api_request` query, a valid action, no login.
fn action_problems(what: &str, a: &Action, from: &SaveFrom) -> Vec<String> {
    let mut bad = Vec::new();
    for inner in a.each() {
        match inner {
            Action::SignIn { .. } => bad.push(format!("{what} signs in: a setup never signs in.")),
            Action::UseComponent { .. } => bad.push(format!("{what} runs a component whose steps were not kept.")),
            Action::Navigate { url } | Action::OpenTab { url, .. } => {
                if url.trim().to_ascii_lowercase().starts_with("file:") {
                    bad.push(format!("{what} goes to a local file: a setup goes to http or https only."));
                } else if !(from.allowed)(url) {
                    bad.push(format!("{what} goes to an address outside the sign-in recipe's sites."));
                }
                if url.contains('?') {
                    bad.push(format!(
                        "{what} goes to an address with a query string: a setup never keeps one, so reach that page by clicking instead."
                    ));
                }
            }
            Action::ApiRequest { query, .. } if !query.is_empty() => bad.push(format!(
                "{what} sends a request with a query: a setup never keeps one, so reach that page by clicking instead."
            )),
            _ => {}
        }
    }
    if let Err(why) = a.validate() {
        bad.push(format!("{what}: {why}"));
    }
    if types_a_credential(a, None, from.accounts) {
        bad.push(format!("{what} types a login: a setup never types one."));
    }
    bad
}

/// Each target an action carries, as JSON.
fn targets_of(a: &Action) -> Vec<Value> {
    a.each()
        .into_iter()
        .filter_map(|a| serde_json::to_value(a).ok())
        .filter_map(|v| v.get("selector").cloned())
        .collect()
}

/// Builds a setup from the asked range, collecting every problem before
/// answering.
pub fn build_setup(asked: &SaveAsked, from: &SaveFrom) -> Result<Built, Vec<String>> {
    let mut bad: Vec<String> = Vec::new();
    let kept = from.kept;
    let prefix = from.prefix;

    // 1. Name and description.
    let name = asked.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if !valid_setup_name(&name) {
        bad.push("A setup's name is 1 to 60 letters, digits, spaces, - or _.".to_string());
    }
    let description = asked.description.trim().to_string();
    if description.is_empty() {
        bad.push("A setup needs a description: what it makes, in a sentence.".to_string());
    }

    // 2. The range.
    let first = kept.actions.first().map(|a| a.n);
    let last = kept.actions.last().map(|a| a.n);
    let check_number =|n: u32, bad: &mut Vec<String>| -> bool {
        match (first, last) {
            (Some(first), _) if n < first => {
                bad.push(format!("Action {n} is no longer kept: the list keeps the last 200, from action {first}."));
                false
            }
            (Some(_), Some(last)) if n <= last => true,
            _ => {
                bad.push(format!("Action {n} was not kept: get_kept_actions lists them."));
                false
            }
        }
    };
    let mut range: Option<(usize, usize)> = None;
    if asked.from == 0 || asked.to == 0 {
        bad.push("from and to are action numbers from get_kept_actions, starting at 1.".to_string());
    } else if asked.from > asked.to {
        bad.push("from must not come after to.".to_string());
    } else {
        let from_ok = check_number(asked.from, &mut bad);
        let to_ok = check_number(asked.to, &mut bad);
        // A `to` past the end is read as the last action, so the rest of the
        // ask is still checked and the person hears everything at once.
        let to_n = if to_ok { Some(asked.to) } else { last.filter(|last| asked.to > *last) };
        if let (true, Some(to_n), Some(first)) = (from_ok, to_n, first) {
            let at = |n: u32| usize::try_from(n - first).unwrap_or(usize::MAX);
            range = Some((at(asked.from), at(to_n)));
        }
    }
    let in_range: &[crate::autorun::kept::KeptAction] = match range {
        Some((a, b)) => &kept.actions[a..=b],
        None => &[],
    };

    // 3. The actions, in order.
    let mut flat: Vec<(u32, Action)> = Vec::new();
    for k in in_range {
        match &k.steps {
            Some(steps) => flat.extend(steps.iter().map(|a| (k.n, a.clone()))),
            None => flat.push((k.n, k.action.clone())),
        }
    }
    for (n, a) in &flat {
        bad.extend(action_problems(&format!("Action {n}"), a, from));
    }

    // 4. The area.
    let area = asked
        .area
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_string)
        .or_else(|| in_range.first().and_then(|k| k.area.clone()))
        .or_else(|| kept.started_area.clone());
    if let Some(area) = &area {
        if !(from.area_known)(area) {
            bad.push(nav::unrecorded_area(area));
        }
    }

    // 5. Inputs.
    let mut inputs: Vec<SetupInput> = Vec::new();
    let mut values: Vec<(String, String)> = Vec::new();
    for i in &asked.inputs {
        let input_name = i.name.trim().to_string();
        if !valid_input_name(&input_name) {
            bad.push(format!("An input's name is 1 to 40 letters, digits or underscores: {}.", i.name.trim()));
            continue;
        }
        if is_login_name(&input_name) {
            bad.push(format!("{input_name} is a login field: a setup never types a login."));
            continue;
        }
        if inputs.iter().any(|x| x.name == input_name) {
            bad.push(format!("The input {input_name} is declared twice."));
            continue;
        }
        let Some(k) = in_range.iter().find(|k| k.n == i.action) else {
            if range.is_some() {
                bad.push(format!("The input {input_name} takes its value from action {}, which is not in these actions.", i.action));
            }
            continue;
        };
        let Action::Fill { value, .. } = &k.action else {
            bad.push(format!("The input {input_name} takes its value from action {}, which is not a fill.", i.action));
            continue;
        };
        if value.trim().chars().count() < INPUT_MIN_CHARS {
            bad.push(format!(
                "The value of {input_name} is too short to stand for an input: at least {INPUT_MIN_CHARS} characters."
            ));
            continue;
        }
        if (i.unique || i.names_the_record) && !starts_with_prefix(value, prefix) {
            bad.push(format!("{input_name} must start with the test prefix {prefix}."));
            continue;
        }
        inputs.push(SetupInput {
            name: input_name.clone(),
            description: i.description.trim().to_string(),
            unique: i.unique,
            names_the_record: i.names_the_record,
        });
        values.push((input_name, value.clone()));
    }
    if inputs.iter().filter(|i| i.names_the_record).count() > 1 {
        bad.push("Only one input can name the record.".to_string());
    }
    // An address is never built from an input.
    for (n, a) in &flat {
        for inner in a.each() {
            if let Action::Navigate { url } | Action::OpenTab { url, .. } = inner {
                if values.iter().any(|(_, v)| url.contains(v.as_str())) {
                    bad.push(format!("An address cannot come from an input: action {n}."));
                }
            }
        }
    }
    let mut replaced: Vec<Replaced> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();
    for (n, a) in &flat {
        let mut v = match serde_json::to_value(a) {
            Ok(v) => v,
            Err(_) => continue,
        };
        replace_in_value(&mut v, &mut Vec::new(), &values, *n, &mut replaced);
        match serde_json::from_value::<Action>(v) {
            Ok(a) => actions.push(a),
            Err(_) => bad.push(format!("Action {n} could not be kept as a template.")),
        }
    }

    // 6. Creates the range made.
    let mut creates: Vec<(u32, SetupCreate)> = Vec::new();
    if let Some((a, b)) = range {
        let start = kept.actions[a].started_ms;
        let mut end = kept.actions[b].ended_ms.saturating_add(SAVE_GRACE_MS);
        if let Some(next) = kept.actions.get(b + 1) {
            end = end.min(next.started_ms);
        }
        let mut unreadable_place = false;
        for e in &from.session.changes {
            let Change::Created { kind, id, name, verified: true } = &e.change else {
                continue;
            };
            let Some(proof) = &e.proof else {
                continue;
            };
            if proof.saved_ms < start || proof.saved_ms >= end {
                continue;
            }
            let Some(id_at) = proof.id_at.clone() else {
                bad.push(format!(
                    "The save that made {kind} {id} answered in a form the app cannot read again, so it cannot be built from: record it, or save without it."
                ));
                unreadable_place = true;
                continue;
            };
            let (template, _) = replace_values(name, &values);
            if !name.trim().is_empty() && !named_with_prefix(&template, &inputs, prefix) {
                bad.push(format!(
                    "{ONLY_TEST_BUILDS} The {kind} that entry {} recorded is not: name it from a unique input that starts with {prefix}, or start its name with {prefix}.",
                    e.n
                ));
            }
            creates.push((
                e.n,
                SetupCreate { kind: kind.clone(), name: template, request: e.request.clone().unwrap_or_default(), id_at },
            ));
        }
        if creates.is_empty() && !unreadable_place {
            bad.push(NO_CREATE_IN_RANGE.to_string());
        }
    }
    let mut returns = 0;
    if let Some(r) = &asked.returns {
        match creates.iter().position(|(n, _)| *n == r.entry) {
            Some(at) => returns = at,
            None if !creates.is_empty() => bad.push(format!("Entry {} is not a create these actions made.", r.entry)),
            None => {}
        }
    }

    // 7. The find rule.
    bad.extend(check_find(&asked.find));
    if let Some(rule) = &asked.find.browser {
        match browser_with_prefix(rule, prefix) {
            Ok(filled) => {
                for (i, a) in filled.iter().enumerate() {
                    bad.extend(action_problems(&format!("The browser find rule's action {}", i + 1), a, from));
                }
                let used: Vec<Value> = kept
                    .actions
                    .iter()
                    .flat_map(|k| std::iter::once(&k.action).chain(k.steps.iter().flatten()))
                    .flat_map(targets_of)
                    .collect();
                for (i, a) in filled.iter().enumerate() {
                    let targets = targets_of(a);
                    if targets.is_empty() || targets.iter().all(|t| used.contains(t)) {
                        continue;
                    }
                    if (from.seen)(area.as_deref(), std::slice::from_ref(a)).is_err() {
                        bad.push(format!(
                            "The browser find rule's action {} uses a locator this session never used and the map never saw.",
                            i + 1
                        ));
                    }
                }
            }
            Err(why) => bad.push(why),
        }
    }

    if !bad.is_empty() {
        return Err(bad);
    }
    Ok(Built {
        setup: SavedSetup {
            name,
            description,
            area,
            inputs,
            actions,
            find: asked.find.clone(),
            creates: creates.into_iter().map(|(_, c)| c).collect(),
            returns,
            version: 0,
            changes: 0,
            saved_at: from.now,
            last_used_at: 0,
        },
        replaced,
        cut: kept.dropped > 0,
    })
}

/// Writes the setup: a new name is version 1; a taken name (by the same key
/// the components use) needs `why` and becomes the next version.
pub fn save(root: &Path, org: &str, project: &str, built: Built, why: Option<&str>) -> Result<SetupSaved, String> {
    let mut setup = built.setup;
    update_with(root, org, project, |file| {
        let k = key(&setup.name);
        match file.setups.iter().position(|s| key(&s.name) == k) {
            Some(at) => {
                if why.map_or(true, |w| w.trim().is_empty()) {
                    return Err(format!("{} is already saved: say why it changes in \"why\".", setup.name));
                }
                let old = &file.setups[at];
                setup.version = old.version + 1;
                setup.changes = old.changes + 1;
                setup.last_used_at = old.last_used_at;
                let out = SetupSaved { saved: setup.name.clone(), version: setup.version, changes: setup.changes };
                file.setups[at] = setup;
                Ok(out)
            }
            None => {
                setup.version = 1;
                setup.changes = 0;
                let out = SetupSaved { saved: setup.name.clone(), version: 1, changes: 0 };
                file.setups.push(setup);
                Ok(out)
            }
        }
    })
}

/// Takes the setup called `name` out of the file; the answer is the name it
/// was saved under. A name the file does not hold is `no_such_setup`.
pub fn remove(root: &Path, org: &str, project: &str, name: &str) -> Result<String, String> {
    update_with(root, org, project, |file| {
        let k = key(name);
        match file.setups.iter().position(|s| key(&s.name) == k) {
            Some(at) => Ok(file.setups.remove(at).name),
            None => Err(no_such_setup(name)),
        }
    })
}

fn rfc3339(ms: u64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(i64::try_from(ms).ok()?)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// What `list_setups` answers: each setup's name, what it builds, its inputs,
/// which find rules it has and when it was saved and last used. Never the
/// actions, the SQL text or any value from an earlier run.
pub fn list_answer(f: &SetupFile) -> Value {
    let mut sorted: Vec<&SavedSetup> = f.setups.iter().collect();
    sorted.sort_by_key(|s| key(&s.name));
    let setups: Vec<Value> = sorted
        .into_iter()
        .map(|s| {
            let mut find = Vec::new();
            if s.find.database.is_some() {
                find.push("database");
            }
            if s.find.browser.is_some() {
                find.push("browser");
            }
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "area": s.area,
                "inputs": s.inputs.iter().map(|i| serde_json::json!({
                    "name": i.name,
                    "description": i.description,
                    "unique": i.unique,
                    "names_the_record": i.names_the_record,
                })).collect::<Vec<_>>(),
                "find": find,
                "saved_at": rfc3339(s.saved_at),
                "last_used": if s.last_used_at == 0 { None } else { rfc3339(s.last_used_at) },
                "version": s.version,
            })
        })
        .collect();
    serde_json::json!({ "setups": setups })
}

/// A setup was used - a record found, or one built: its `last_used_at` is
/// `now`. A name the file does not hold, or one that is already stamped
/// `now`, changes nothing.
pub fn stamp_used(root: &Path, org: &str, project: &str, name: &str, now: u64) -> Result<(), String> {
    update_with(root, org, project, |file| {
        let k = key(name);
        if let Some(s) = file.setups.iter_mut().find(|s| key(&s.name) == k) {
            s.last_used_at = now;
        }
        Ok(())
    })
}
