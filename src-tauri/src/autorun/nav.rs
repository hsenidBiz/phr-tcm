//! Module paths: how an unattended run gets from the application's home
//! page to a case's module screen, and the per-project switch that says
//! whether scripts may open pages by address.
//!
//! One file per project, beside the sign-in recipe:
//! `<autorun root>/projects/<slug>.nav.json`. It is kept on this machine
//! only and never sent to Azure DevOps. A project with no file, or with an
//! empty `modules` list, runs exactly as it did before module paths
//! existed.

use super::recipe::{origin_of, project_slug, SignInRecipe};
use super::CaseScript;
use crate::browser::actions::{execute_in, failed_by, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::locator::Target;
use crate::browser::page;
use crate::browser::timing::Timing;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn yes() -> bool {
    true
}

/// One module's recorded way in from the home page.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModulePath {
    /// Compared with a test case's Module field, trimmed and ignoring case.
    pub module: String,
    /// In order, the locators a run clicks: the same `Target` every script
    /// click uses.
    pub clicks: Vec<Target>,
    /// The path part of the address the recording ended on, with no query
    /// or fragment: `/hr/leave/apply`.
    pub arrived: String,
    /// When it was recorded, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub recorded: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NavFile {
    /// Whether a script may open a page by address. Absent means yes, so a
    /// project changes nothing until someone turns it off.
    #[serde(default = "yes")]
    pub direct_urls: bool,
    #[serde(default)]
    pub modules: Vec<ModulePath>,
}

impl Default for NavFile {
    fn default() -> Self {
        NavFile { direct_urls: true, modules: vec![] }
    }
}

/// A recorded module as the Module paths dialog shows it. Every click is
/// already in words (`link "Leave"`), so the webview never keeps a second
/// copy of how a locator reads.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ModuleView {
    pub module: String,
    pub clicks: Vec<String>,
    pub arrived: String,
    pub recorded: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct NavView {
    pub direct_urls: bool,
    pub modules: Vec<ModuleView>,
}

pub fn view(nav: &NavFile) -> NavView {
    NavView {
        direct_urls: nav.direct_urls,
        modules: nav
            .modules
            .iter()
            .map(|m| ModuleView {
                module: m.module.clone(),
                clicks: m.clicks.iter().map(Target::describe).collect(),
                arrived: m.arrived.clone(),
                recorded: m.recorded.clone(),
            })
            .collect(),
    }
}

/// Why a case in a project WITH paths is not run (design §5).
pub const NO_MODULE: &str = "This case has no Module - set one in Azure DevOps, or record a path for it.";
pub const NO_ACCOUNT: &str = "Choose an account when starting the run, or set Runs as on the script.";
const NO_PATH_START: &str = "No menu path recorded for module \"";

pub fn no_path(module: &str) -> String {
    format!("{NO_PATH_START}{}\" - record one in Auto Run, Module paths.", module.trim())
}

/// How two module names are compared: trimmed, case ignored.
pub fn module_key(module: &str) -> String {
    module.trim().to_lowercase()
}

pub fn nav_path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}.nav.json", project_slug(org, project)))
}

pub fn load_nav(root: &Path, org: &str, project: &str) -> Result<NavFile, String> {
    match std::fs::read_to_string(nav_path(root, org, project)) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("the module paths file is not readable: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NavFile::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn validate(nav: &NavFile) -> Result<(), String> {
    let mut seen = HashSet::new();
    for m in &nav.modules {
        let name = m.module.trim();
        if name.is_empty() {
            return Err("a module path needs the module's name".to_string());
        }
        if !seen.insert(module_key(name)) {
            return Err(format!("module \"{name}\" has two paths - keep one"));
        }
        if m.clicks.is_empty() {
            return Err(format!("module \"{name}\" has no clicks - record it again"));
        }
        for (i, click) in m.clicks.iter().enumerate() {
            click.validate().map_err(|e| format!("module \"{name}\", click {}: {e}", i + 1))?;
        }
        if !m.arrived.starts_with('/') {
            return Err(format!(
                "module \"{name}\": where it ends must be an address path such as /hr/leave/apply"
            ));
        }
    }
    Ok(())
}

/// Replaces the whole file. Validated first, then written to a temporary
/// file and renamed, so a reader never sees half a file and a refused save
/// leaves the earlier one exactly as it was.
pub fn save_nav(root: &Path, org: &str, project: &str, nav: &NavFile) -> Result<(), String> {
    validate(nav)?;
    if org.trim().is_empty() || project.trim().is_empty() {
        return Err("pick an organization and a project first".to_string());
    }
    let path = nav_path(root, org, project);
    std::fs::create_dir_all(path.parent().expect("projects folder")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(nav).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

pub fn find_path<'a>(nav: &'a NavFile, module: &str) -> Option<&'a ModulePath> {
    let key = module_key(module);
    if key.is_empty() {
        return None;
    }
    nav.modules.iter().find(|m| module_key(&m.module) == key)
}

/// Add a path, or replace the one already recorded for the same module
/// (Re-record). The module name is stored trimmed.
pub fn put_path(root: &Path, org: &str, project: &str, mut path: ModulePath) -> Result<NavFile, String> {
    path.module = path.module.trim().to_string();
    let mut nav = load_nav(root, org, project)?;
    let key = module_key(&path.module);
    match nav.modules.iter_mut().find(|m| module_key(&m.module) == key) {
        Some(slot) => *slot = path,
        None => nav.modules.push(path),
    }
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

pub fn remove_path(root: &Path, org: &str, project: &str, module: &str) -> Result<NavFile, String> {
    let mut nav = load_nav(root, org, project)?;
    let key = module_key(module);
    nav.modules.retain(|m| module_key(&m.module) != key);
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

pub fn set_direct_urls(root: &Path, org: &str, project: &str, allowed: bool) -> Result<NavFile, String> {
    let mut nav = load_nav(root, org, project)?;
    nav.direct_urls = allowed;
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

/// Where a case should be taken after sign-in, or why it cannot run.
/// `Ok(None)`: the project has no paths, and the case runs as it always
/// has. `module` is the case's Module field; `account` is the account that
/// applies to it (the script's own, else the run's). Checked in the
/// design's order: the Module, then a path for it, then an account.
pub fn route_for<'a>(
    nav: &'a NavFile,
    module: Option<&str>,
    account: Option<&str>,
) -> Result<Option<&'a ModulePath>, String> {
    if nav.modules.is_empty() {
        return Ok(None);
    }
    let module = module.map(str::trim).unwrap_or("");
    if module.is_empty() {
        return Err(NO_MODULE.to_string());
    }
    let path = find_path(nav, module).ok_or_else(|| no_path(module))?;
    if account.map_or(true, |a| a.trim().is_empty()) {
        return Err(NO_ACCOUNT.to_string());
    }
    Ok(Some(path))
}

/// The path part of an address: no scheme or host, no query, no fragment.
/// `/` when there is nothing after the host.
pub fn path_of(href: &str) -> String {
    let no_fragment = href.split('#').next().unwrap_or("");
    let no_query = no_fragment.split('?').next().unwrap_or("");
    let rest = match no_query.find("://") {
        Some(i) => &no_query[i + 3..],
        None => no_query,
    };
    match rest.find('/') {
        Some(i) => rest[i..].to_string(),
        None => "/".to_string(),
    }
}

/// Is the page at `href` the recipe's home page? Same origin and same path;
/// a query or fragment does not make it another page.
pub fn same_page(href: &str, start_url: &str) -> bool {
    origin_of(href).is_some() && origin_of(href) == origin_of(start_url) && path_of(href) == path_of(start_url)
}

/// Start of the sentence a case gets when its trip to the module fails.
pub const UNREACHED_PREFIX: &str = "Could not reach module \"";

/// Everything a run needs to take a signed-in browser to one module: where
/// home is, where `navigate` may go, and the recorded path.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub start_url: String,
    pub origins: Vec<String>,
    pub path: ModulePath,
}

impl Route {
    pub fn new(recipe: &SignInRecipe, path: ModulePath) -> Route {
        Route { start_url: recipe.start_url.clone(), origins: recipe.origins(), path }
    }
}

/// Where a trip to a module stopped.
#[derive(Debug, Clone, PartialEq)]
pub enum Where {
    Home,
    /// `n` counts from 1; `locator` is the click in words.
    Click { n: usize, locator: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathFailure {
    pub at: Where,
    pub reason: String,
    /// The browser failed, not the page: no picture is asked for.
    pub harness: bool,
}

impl PathFailure {
    /// The run's sentence (design §5).
    pub fn for_run(&self, module: &str) -> String {
        let module = module.trim();
        let reason = self.reason.trim_end_matches('.');
        match &self.at {
            Where::Home => format!("{UNREACHED_PREFIX}{module}\": the home page did not open - {reason}."),
            Where::Click { n, locator } => format!("{UNREACHED_PREFIX}{module}\": click {n}, {locator} - {reason}."),
        }
    }

    /// The Module paths dialog's shorter form (design §4).
    pub fn for_dialog(&self) -> String {
        match &self.at {
            Where::Home => format!("the home page did not open: {}", self.reason),
            Where::Click { n, locator } => format!("click {n}, {locator}: {}", self.reason),
        }
    }
}

/// Back to the recipe's home page unless the browser is already there.
/// This is the runner's own navigation: the "no direct addresses" rule is
/// about scripts and does not apply to it.
pub async fn go_home<D: Driver>(d: &mut D, start_url: &str, origins: &[String], timing: &Timing) -> ActionOutcome {
    let href = match page::eval_value(d, "location.href").await {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        // Right after a sign-in a redirect may still be in flight and the
        // page refuses to say where it is: not home yet, so go there.
        Err(e) if e.is_transient() => String::new(),
        Err(e) => return failed_by(e),
    };
    if same_page(&href, start_url) {
        return ActionOutcome::passed("already on the home page");
    }
    let out = execute_in(d, &Action::Navigate { url: start_url.to_string() }, timing, &Policy::only(origins.to_vec())).await;
    if out.ok {
        return ActionOutcome::passed("went to the home page");
    }
    // The navigate's own words name the address; this sentence reaches the
    // person, so it does not.
    let mut failed = ActionOutcome::failed(if out.harness {
        "the browser did not answer while the home page was opening"
    } else {
        "the home page did not load"
    });
    failed.harness = out.harness;
    failed
}

/// Home, then each recorded click with the runner's own click (so each
/// must find exactly one visible element), then wait up to `nav_ms` for
/// the address path to equal `arrived`. Ok carries the path it reached.
pub async fn go_to_module<D: Driver>(d: &mut D, route: &Route, timing: &Timing) -> Result<String, PathFailure> {
    let home = go_home(d, &route.start_url, &route.origins, timing).await;
    if !home.ok {
        return Err(PathFailure { at: Where::Home, reason: home.detail, harness: home.harness });
    }
    let policy = Policy::only(route.origins.clone());
    for (i, click) in route.path.clicks.iter().enumerate() {
        let out = execute_in(d, &Action::Click { selector: click.clone() }, timing, &policy).await;
        if !out.ok {
            return Err(PathFailure {
                at: Where::Click { n: i + 1, locator: click.describe() },
                reason: out.detail,
                harness: out.harness,
            });
        }
    }
    let at = match route.path.clicks.last() {
        Some(c) => Where::Click { n: route.path.clicks.len(), locator: c.describe() },
        None => Where::Home,
    };
    let deadline = Instant::now() + Duration::from_millis(timing.nav_ms);
    let mut last = String::new();
    loop {
        match page::eval_value(d, "location.href").await {
            Ok(v) => {
                last = path_of(v.as_str().unwrap_or(""));
                if last == route.path.arrived {
                    return Ok(last);
                }
            }
            // Between two documents the page refuses; that is an answer.
            Err(e) if e.is_transient() => {}
            Err(e) => {
                let o = failed_by(e);
                return Err(PathFailure { at, reason: o.detail, harness: o.harness });
            }
        }
        if Instant::now() >= deadline {
            let seen = if last.is_empty() { "an address it could not read" } else { last.as_str() };
            return Err(PathFailure {
                at,
                reason: format!("the page ended on {seen}, not {}", route.path.arrived),
                harness: false,
            });
        }
        tokio::time::sleep(Duration::from_millis(timing.poll_ms)).await;
    }
}

/// A trip to a module as the one outcome the run's "Go to X" line shows.
pub fn reached(module: &str, result: Result<String, PathFailure>) -> ActionOutcome {
    match result {
        Ok(_) => ActionOutcome::passed(format!("Go to {}", module.trim())),
        Err(f) => {
            let mut out = ActionOutcome::failed(f.for_run(module));
            out.harness = f.harness;
            out
        }
    }
}

/// How a mid-script `sign_in`'s one outcome joins the sign-in to the trip
/// back to the module that follows it.
pub const THEN: &str = "; then ";

/// The failed-trip sentence inside a mid-script `sign_in`'s outcome, from
/// `UNREACHED_PREFIX` on. Only meaningful for an outcome that belongs to a
/// `sign_in` action: anywhere else these words are the page's or the
/// script's (a dialog, a `check_text` value), never the runner's.
pub fn unreached_after_sign_in(detail: &str) -> Option<&str> {
    detail.find(&format!("{THEN}{UNREACHED_PREFIX}")).map(|i| &detail[i + THEN.len()..])
}

/// A case reason that is about the project's setup (paths, Module field,
/// account), not about the script. A prefix, never a search: a Failed
/// case's reason begins `step N:` and may quote the page, which can say
/// anything.
pub fn is_setup_problem(reason: &str) -> bool {
    reason.starts_with(UNREACHED_PREFIX)
        || reason == NO_MODULE
        || reason == NO_ACCOUNT
        || reason.starts_with(NO_PATH_START)
}

/// Start of the sentence a saved `navigate` gets while the switch is off.
pub const NO_ADDRESS_PREFIX: &str = "this project does not allow opening pages by address";

pub fn no_address(step: i32) -> String {
    format!(
        "{NO_ADDRESS_PREFIX}: a run starts on the case's module screen - use clicks instead of \"navigate\" (step {step})."
    )
}

/// While the switch is off, a script with any `navigate` (absolute or
/// relative) cannot be saved. Names the first case and step it finds.
pub fn check_no_addresses(nav: &NavFile, scripts: &[CaseScript]) -> Result<(), String> {
    if nav.direct_urls {
        return Ok(());
    }
    for sc in scripts {
        for step in &sc.steps {
            if step.actions.iter().any(|a| matches!(a, Action::Navigate { .. })) {
                return Err(format!("case {}: {}", sc.case_id, no_address(step.step_number)));
            }
        }
    }
    Ok(())
}

/// `check_no_addresses` against the project's own file: the one call every
/// save path makes (the Script editor, a JSON import, the assistant's
/// `save_autorun_script`).
pub fn refuse_addresses(root: &Path, org: &str, project: &str, scripts: &[CaseScript]) -> Result<(), String> {
    check_no_addresses(&load_nav(root, org, project)?, scripts)
}

/// What an assistant's guide gains while the switch is off. Empty while it
/// is on.
pub fn guide_section(nav: &NavFile) -> String {
    if nav.direct_urls {
        return String::new();
    }
    "## This project's runs start on the module screen\n\n\
     - The run signs in and goes to the case's module screen before step 1, by the menu path recorded in the app.\n\
     - The script starts there: its first action acts on the module screen.\n\
     - Never use `navigate`. This project refuses to save a script that opens a page by address; reach every other screen with clicks.\n\
     - A `sign_in` action lands on the home page, and the run brings the browser back to the module screen before the next action.\n"
        .to_string()
}

/// An outcome that means the run could not put the case where its steps
/// begin, or the script tried to open a page by address where that is not
/// allowed: the case is Blocked, not Failed.
///
/// Named differently from [`is_setup_problem`] on purpose: that one reads a
/// case's finished, formatted `reason` (which may carry a `step N:`
/// prefix, or be one of the exact sentences `NO_MODULE`/`NO_ACCOUNT`); this
/// one reads a single action's raw `detail` before any such prefix is
/// added, which is what `replay::propose` has while it is still deciding
/// the verdict.
pub fn is_route_problem(detail: &str) -> bool {
    detail.contains(UNREACHED_PREFIX) || detail.starts_with(NO_ADDRESS_PREFIX)
}
