//! Areas (once "module paths"): how an unattended run gets from the
//! application's home page to the screen a case starts on, and the
//! per-project switch that says whether scripts may open pages by address.
//! A script that names an `area` goes there; one that names none goes to
//! the area named like its test case's Module.
//!
//! One file per project, beside the sign-in recipe:
//! `<autorun root>/projects/<slug>.nav.json`. It is kept on this machine
//! only and never sent to Azure DevOps. A project with no file, or with an
//! empty `modules` list, runs exactly as it did before module paths
//! existed.

use super::accounts::Account;
use super::recipe::{origin_of, project_slug, RecipeStep, SignInRecipe};
use super::CaseScript;
use crate::browser::actions::{execute_in, failed_by, Action, ActionOutcome, Policy};
use crate::browser::cdp::Driver;
use crate::browser::expect::{expect, Check};
use crate::browser::locator::Target;
use crate::browser::page;
use crate::browser::timing::Timing;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn yes() -> bool {
    true
}

/// One area's recorded way in from the home page. An area is a named place
/// inside a test-case Module (PMS has Cycle Setup, Manage Cycle...); a
/// module can have any number of them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModulePath {
    /// The area's name, unique per project ignoring case: what a script's
    /// `area` names. Empty in a file written before areas existed; loading
    /// it gives the area its module's name, so a case with no `area` still
    /// goes where it went.
    #[serde(default)]
    pub area: String,
    /// The test-case Module the area belongs to, for grouping. An area
    /// named like a case's Module - or a module's only area, when none is -
    /// is where a script with no `area` goes.
    pub module: String,
    /// In order, the locators a run clicks: the same `Target` every script
    /// click uses.
    pub clicks: Vec<Target>,
    /// The path part of the address the recording ended on, with no query
    /// or fragment: `/hr/leave/apply`.
    pub arrived: String,
    /// When it was recorded, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub recorded: String,
    /// The path part of the page the recording's first click was made on.
    /// A trip that starts right after a sign-in which left the browser
    /// there clicks straight on, instead of going home through a reload.
    /// Empty for a path recorded before this was kept: it goes home as it
    /// always did.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start: String,
}

impl ModulePath {
    /// The area's name, trimmed: its own, or - for a path that has none
    /// yet - its module's.
    pub fn name(&self) -> &str {
        match self.area.trim() {
            "" => self.module.trim(),
            a => a,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NavFile {
    /// Whether a script may open a page by address. Absent means yes, so a
    /// project changes nothing until someone turns it off.
    #[serde(default = "yes")]
    pub direct_urls: bool,
    #[serde(default)]
    pub modules: Vec<ModulePath>,
    /// The project's own save words, beside the built-in ones
    /// (`browser::save_guard`): a no-save script's browser stops a writing
    /// request whose path holds any of them. Kept trimmed and lowercased.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_words: Vec<String>,
}

impl Default for NavFile {
    fn default() -> Self {
        NavFile { direct_urls: true, modules: vec![], save_words: vec![] }
    }
}

/// A recorded area as the Areas dialog shows it. Every click is already in
/// words (`link "Leave"`), so the webview never keeps a second copy of how
/// a locator reads.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ModuleView {
    pub area: String,
    pub module: String,
    pub clicks: Vec<String>,
    pub arrived: String,
    pub recorded: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct NavView {
    pub direct_urls: bool,
    pub modules: Vec<ModuleView>,
    /// The project's own save words, which the Setup tab can remove.
    pub save_words: Vec<String>,
    /// The built-in ones, shown beside them and never removable.
    pub built_in_save_words: Vec<String>,
}

pub fn view(nav: &NavFile) -> NavView {
    NavView {
        direct_urls: nav.direct_urls,
        modules: nav
            .modules
            .iter()
            .map(|m| ModuleView {
                area: m.name().to_string(),
                module: m.module.clone(),
                clicks: m.clicks.iter().map(Target::describe).collect(),
                arrived: m.arrived.clone(),
                recorded: m.recorded.clone(),
            })
            .collect(),
        save_words: nav.save_words.clone(),
        built_in_save_words: crate::browser::save_guard::SAVE_WORDS.iter().map(|w| w.to_string()).collect(),
    }
}

/// Why a case in a project WITH paths is not run (design §5).
pub const NO_MODULE: &str = "This case has no Module - set one in Azure DevOps, or record a path for it.";
pub const NO_ACCOUNT: &str = "Choose an account when starting the run, or set Runs as on the script.";
const NO_PATH_START: &str = "No menu path recorded for module \"";
const UNRECORDED_AREA_START: &str = "the area \"";

pub fn no_path(module: &str) -> String {
    format!("{NO_PATH_START}{}\" - record one in Auto Run, Areas.", module.trim())
}

/// Why a case with no `area` is not run when its module has several areas
/// and none is named like it: there is no telling which one it means.
pub fn no_default_area(module: &str, areas: &[&str]) -> String {
    let names = areas.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", ");
    format!(
        "{NO_PATH_START}{}\" by its own name - its areas are {names}: set the script's area to one of them, \
         or record an area named \"{}\" in Auto Run, Areas.",
        module.trim(),
        module.trim()
    )
}

/// Why a case whose script names an area the project has not recorded is
/// not run, and the start of why such a script is not saved (spec §9).
pub fn unrecorded_area(area: &str) -> String {
    format!("{UNRECORDED_AREA_START}{}\" is not recorded - record it in Auto Run, Areas", area.trim())
}

/// How two module (or area) names are compared: trimmed, case ignored.
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
            let nav: NavFile =
                serde_json::from_str(s).map_err(|e| format!("the areas file is not readable: {e}"))?;
            Ok(as_areas(nav, org, project, &nav_path(root, org, project)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NavFile::default()),
        Err(e) => Err(e.to_string()),
    }
}

/// A file as areas: each path without an area name takes its module's
/// (a file written before areas existed), and of two whose names differ
/// only in case the first is kept and the other logged and left out - an
/// old file could hold "Leave" and "LEAVE" as two modules, and two areas
/// one name apart only in case would make a script's `area` ambiguous.
/// The file is read on every run, check and save, so what was left out is
/// logged once per file per process, not once per read.
fn as_areas(mut nav: NavFile, org: &str, project: &str, file: &Path) -> NavFile {
    let mut kept: Vec<ModulePath> = Vec::with_capacity(nav.modules.len());
    let mut dropped: Vec<String> = Vec::new();
    for mut m in std::mem::take(&mut nav.modules) {
        m.area = m.name().to_string();
        let key = module_key(&m.area);
        match kept.iter().find(|k| !key.is_empty() && module_key(&k.area) == key) {
            Some(first) => dropped.push(format!(
                "Auto Run areas for {org} / {project}: \"{}\" differs from \"{}\" only in case - kept the first, left this one out",
                m.area, first.area
            )),
            None => kept.push(m),
        }
    }
    if !dropped.is_empty() && first_warning_for(file) {
        for line in dropped {
            crate::applog::warn(line);
        }
    }
    nav.modules = kept;
    nav
}

/// True the first time it is asked about this file in this process.
fn first_warning_for(file: &Path) -> bool {
    static WARNED: std::sync::Mutex<Option<HashSet<PathBuf>>> = std::sync::Mutex::new(None);
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    warned.get_or_insert_with(HashSet::new).insert(file.to_path_buf())
}

pub fn validate(nav: &NavFile) -> Result<(), String> {
    let mut seen = HashSet::new();
    for m in &nav.modules {
        if m.module.trim().is_empty() {
            return Err("an area needs the name of the module it belongs to".to_string());
        }
        let name = m.name();
        if !seen.insert(module_key(name)) {
            return Err(format!("area \"{name}\" has two paths - keep one"));
        }
        if m.clicks.is_empty() {
            return Err(format!("area \"{name}\" has no clicks - record it again"));
        }
        for (i, click) in m.clicks.iter().enumerate() {
            click.validate().map_err(|e| format!("area \"{name}\", click {}: {e}", i + 1))?;
        }
        if !m.arrived.starts_with('/') {
            return Err(format!("area \"{name}\": where it ends must be an address path such as /hr/leave/apply"));
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

/// The area of this name, trimmed and ignoring case.
pub fn find_area<'a>(nav: &'a NavFile, area: &str) -> Option<&'a ModulePath> {
    let key = module_key(area);
    if key.is_empty() {
        return None;
    }
    nav.modules.iter().find(|m| module_key(m.name()) == key)
}

/// Where a case with no `area` goes: the area named like `module`, or -
/// when nothing is named like it - the module's only area. Recording one
/// area under a module and naming it for its screen ("Manage Cycle" under
/// Performance) is the obvious first thing to do, and every script that
/// names no area should still find it (2026-10-01).
pub fn find_path<'a>(nav: &'a NavFile, module: &str) -> Option<&'a ModulePath> {
    find_area(nav, module).or_else(|| match areas_of(nav, module).as_slice() {
        [only] => Some(*only),
        _ => None,
    })
}

/// Every area recorded under `module`, in the file's order.
fn areas_of<'a>(nav: &'a NavFile, module: &str) -> Vec<&'a ModulePath> {
    let key = module_key(module);
    if key.is_empty() {
        return vec![];
    }
    nav.modules.iter().filter(|m| module_key(&m.module) == key).collect()
}

/// Whether `area` may be recorded under `module`: refused when another
/// module already holds an area of that name (names are unique per
/// project, ignoring case). The same name under the same module is a
/// Re-record and is fine. Asked before a recording opens a browser, and
/// again when the path is saved.
pub fn check_area_free(nav: &NavFile, area: &str, module: &str) -> Result<(), String> {
    match find_area(nav, area) {
        Some(slot) if module_key(&slot.module) != module_key(module) => Err(format!(
            "an area named \"{}\" is already recorded under {} - choose another name",
            slot.name(),
            slot.module.trim()
        )),
        _ => Ok(()),
    }
}

/// Add an area, or replace the one already recorded under the same name
/// and module, in any case (Re-record). A new area whose name is taken by
/// one under ANOTHER module is refused: names are unique per project. An
/// area with no name is named after its module. Both are stored trimmed.
pub fn put_path(root: &Path, org: &str, project: &str, mut path: ModulePath) -> Result<NavFile, String> {
    path.module = path.module.trim().to_string();
    path.area = path.name().to_string();
    let mut nav = load_nav(root, org, project)?;
    check_area_free(&nav, &path.area, &path.module)?;
    let key = module_key(&path.area);
    match nav.modules.iter_mut().find(|m| module_key(m.name()) == key) {
        Some(slot) => *slot = path,
        None => nav.modules.push(path),
    }
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

/// Remove the area of this name, ignoring case. The module's other areas
/// stay.
pub fn remove_path(root: &Path, org: &str, project: &str, area: &str) -> Result<NavFile, String> {
    let mut nav = load_nav(root, org, project)?;
    let key = module_key(area);
    nav.modules.retain(|m| module_key(m.name()) != key);
    save_nav(root, org, project, &nav)?;
    Ok(nav)
}

/// Replace the project's own save words. Each is checked
/// (`save_guard::check_words`) before the file is touched, so a refused
/// list leaves the words already saved exactly as they were.
pub fn set_save_words(root: &Path, org: &str, project: &str, words: &[String]) -> Result<NavFile, String> {
    let words = crate::browser::save_guard::check_words(words)?;
    let mut nav = load_nav(root, org, project)?;
    nav.save_words = words;
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
///
/// A script that names an `area` goes there, and a name the project has
/// not recorded refuses the case - even in a project with no areas at all.
/// With no `area` (or a blank one) it is exactly as before areas existed:
/// `Ok(None)` when the project has no paths, and the case runs as it always
/// has; otherwise the area named like the case's `module`, or its only
/// area when none is (`find_path`), checked in the
/// design's order - the Module, then a path for it. Either way an
/// `account` must apply (the script's own, else the run's).
pub fn route_for<'a>(
    nav: &'a NavFile,
    area: Option<&str>,
    module: Option<&str>,
    account: Option<&str>,
) -> Result<Option<&'a ModulePath>, String> {
    let area = area.map(str::trim).unwrap_or("");
    let path = if !area.is_empty() {
        find_area(nav, area).ok_or_else(|| unrecorded_area(area))?
    } else {
        if nav.modules.is_empty() {
            return Ok(None);
        }
        let module = module.map(str::trim).unwrap_or("");
        if module.is_empty() {
            return Err(NO_MODULE.to_string());
        }
        find_path(nav, module).ok_or_else(|| match areas_of(nav, module).as_slice() {
            [] => no_path(module),
            several => no_default_area(module, &several.iter().map(|m| m.name()).collect::<Vec<_>>()),
        })?
    };
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

/// Where home is and how to leave it ready: the recipe's start address,
/// where `navigate` may go, and - for a home reached by address, which is
/// a fresh load - the marker that says the page is signed in and the
/// `after_sign_in` steps a fresh load has to have put back.
#[derive(Debug, Clone, PartialEq)]
pub struct Home {
    pub start_url: String,
    pub origins: Vec<String>,
    pub signed_in: Target,
    pub after_sign_in: Vec<RecipeStep>,
}

impl Home {
    pub fn of(recipe: &SignInRecipe) -> Home {
        Home {
            start_url: recipe.start_url.clone(),
            origins: recipe.origins(),
            signed_in: recipe.signed_in.clone(),
            after_sign_in: recipe.after_sign_in.clone(),
        }
    }
}

/// Everything a run needs to take a signed-in browser to one module: its
/// home, and the recorded path.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub home: Home,
    pub path: ModulePath,
}

impl Route {
    pub fn new(recipe: &SignInRecipe, path: ModulePath) -> Route {
        Route { home: Home::of(recipe), path }
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
            // `go_home`'s sentences already name the home page, and a path
            // with no clicks fails its arrival check here too: the reason
            // stands alone.
            Where::Home => format!("{UNREACHED_PREFIX}{module}\": {reason}."),
            Where::Click { n, locator } => format!("{UNREACHED_PREFIX}{module}\": click {n}, {locator} - {reason}."),
        }
    }

    /// The Areas dialog's shorter form (design §4).
    pub fn for_dialog(&self) -> String {
        match &self.at {
            Where::Home => self.reason.clone(),
            Where::Click { n, locator } => format!("click {n}, {locator}: {}", self.reason),
        }
    }
}

/// Back to the recipe's home page unless the browser is already there.
/// This is the runner's own navigation: the "no direct addresses" rule is
/// about scripts and does not apply to it.
///
/// Going there is a fresh load, and a fresh load can undo `after_sign_in`
/// (PeoplesHR's recipe starts on its login page, which sends a signed-in
/// visitor on to a home page whose menu is drawn closed). So once there,
/// it waits for the signed-in marker - the redirect may still be in flight
/// when the first load fires - and runs `after_sign_in` again. Already
/// home means nothing was reloaded: the steps are not run twice, since a
/// toggle run twice closes what it opened.
pub async fn go_home<D: Driver>(d: &mut D, home: &Home, timing: &Timing) -> ActionOutcome {
    let href = match page::eval_value(d, "location.href").await {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        // Right after a sign-in a redirect may still be in flight and the
        // page refuses to say where it is: not home yet, so go there.
        Err(e) if e.is_transient() => String::new(),
        Err(e) => return failed_by(e),
    };
    if same_page(&href, &home.start_url) {
        return ActionOutcome::passed("already on the home page");
    }
    load_home(d, home, timing).await
}

/// Load the recipe's home page whether or not the browser is on it - a
/// fresh load - and leave it ready: the signed-in marker waited for and
/// `after_sign_in` run again, as `go_home` does once it has to go.
pub async fn load_home<D: Driver>(d: &mut D, home: &Home, timing: &Timing) -> ActionOutcome {
    let policy = Policy::only(home.origins.clone());
    let out = execute_in(d, &Action::Navigate { url: home.start_url.clone() }, timing, &policy).await;
    if !out.ok {
        // The navigate's own words name the address; this sentence reaches
        // the person, so it does not.
        let mut failed = ActionOutcome::failed(if out.harness {
            "the browser did not answer while the home page was opening"
        } else {
            "the home page did not load"
        });
        failed.harness = out.harness;
        return failed;
    }
    if home.after_sign_in.is_empty() {
        return ActionOutcome::passed("went to the home page");
    }
    let marker = expect(d, &home.signed_in, Check::Visible, timing.nav_ms, timing.poll_ms).await;
    if !marker.ok {
        let mut failed = ActionOutcome::failed(if marker.harness {
            "the browser did not answer while the home page was opening"
        } else {
            "the home page opened but never showed it was signed in"
        });
        failed.harness = marker.harness;
        return failed;
    }
    match super::signin::run_after_sign_in(d, &home.after_sign_in, timing, &policy).await {
        Ok(()) => ActionOutcome::passed("went to the home page"),
        Err((n, why, harness)) => {
            // A step's own words can name an address (a `navigate` in
            // after_sign_in); this sentence reaches the person, so they go
            // to the log instead.
            crate::applog::warn(format!("going home: after_sign_in step {n} stopped: {why}"));
            let mut failed = ActionOutcome::failed(format!("went to the home page, but after_sign_in step {n} stopped"));
            failed.harness = harness;
            failed
        }
    }
}

/// What happened just before a trip to a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripFrom {
    /// A sign-in has just finished: the browser sits where it left it, with
    /// after_sign_in done.
    SignIn,
    /// Anything else - a browser already in use, whose page may be a module
    /// screen however its address reads.
    Elsewhere,
}

/// Is the browser on the page the path's recording began on? Only asked
/// right after a sign-in, and only of a path that knows that page.
async fn on_the_starting_page<D: Driver>(d: &mut D, route: &Route) -> bool {
    if route.path.start.is_empty() {
        return false;
    }
    match page::eval_value(d, "location.href").await {
        Ok(v) => {
            let href = v.as_str().unwrap_or("");
            origin_of(href).is_some()
                && origin_of(href) == origin_of(&route.home.start_url)
                && path_of(href) == route.path.start
        }
        Err(_) => false,
    }
}

/// Home, then each recorded click with the runner's own click (so each
/// must find exactly one visible element), then wait up to `nav_ms` for
/// the address path to equal `arrived`. Ok carries the path it reached.
///
/// Right after a sign-in that left the browser on the page the recording
/// began on, there is no going home: that would reload the application -
/// through its login page, for a recipe that starts there - only to land on
/// the same page and run after_sign_in a second time (PeoplesHR,
/// 2026-10-01). Anywhere else it goes home, since an address that reads
/// like home can still be showing a module screen.
pub async fn go_to_module<D: Driver>(
    d: &mut D,
    route: &Route,
    from: TripFrom,
    timing: &Timing,
) -> Result<String, PathFailure> {
    let home = if from == TripFrom::SignIn && on_the_starting_page(d, route).await {
        ActionOutcome::passed("already where the path begins")
    } else {
        go_home(d, &route.home, timing).await
    };
    if !home.ok {
        return Err(PathFailure { at: Where::Home, reason: home.detail, harness: home.harness });
    }
    let policy = Policy::only(route.home.origins.clone());
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
        d.idle(Duration::from_millis(timing.poll_ms)).await;
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

/// Added to a failed trip's sentence when the second go failed too.
pub const TRIED_TWICE: &str = " Tried twice, reloading the start page between.";

/// What the page was doing, from the page log, written to the application
/// log - what a bug report ships - under `who`: a header with `detail`,
/// then one line per entry. Never into the run, where a long list would
/// bury the sentence the person reads. `false` when the log was empty and
/// nothing was written.
pub fn log_page<D: Driver>(d: &D, who: &str, detail: &str) -> bool {
    let lines = d.page_log();
    if lines.is_empty() {
        return false;
    }
    crate::applog::warn(format!("{who}: {detail} What the page was doing:"));
    for line in lines {
        crate::applog::warn(format!("{who}, page: {line}"));
    }
    true
}

/// A run's trip to the module, as the one outcome its "Go to X" line
/// shows, gone once more when it fails (spec §5): the page log of the
/// first go is written under `who` (see `log_page`), the start address is
/// loaded afresh - which waits for it to settle, signed in and with
/// `after_sign_in` done - and the trip is made once more from there. A
/// second failure says `TRIED_TWICE`. One reload, then the answer: never
/// a loop. A browser that stopped answering is not reloaded - it would not
/// answer that either.
pub async fn reach_module<D: Driver>(
    d: &mut D,
    route: &Route,
    from: TripFrom,
    timing: &Timing,
    who: &str,
) -> ActionOutcome {
    let module = route.path.name();
    let first = reached(module, go_to_module(d, route, from, timing).await);
    if first.ok || first.harness {
        return first;
    }
    log_page(d, &format!("{who}, first try"), &first.detail);
    let reload = load_home(d, &route.home, timing).await;
    let mut second = if reload.ok {
        // Right after a fresh load the browser sits where a sign-in leaves
        // it: on the path's own first page, there is no going home again.
        reached(module, go_to_module(d, route, TripFrom::SignIn, timing).await)
    } else {
        reached(module, Err(PathFailure { at: Where::Home, reason: reload.detail, harness: reload.harness }))
    };
    if !second.ok {
        second.detail.push_str(TRIED_TWICE);
    }
    second
}

/// How a mid-script `sign_in`'s one outcome joins the sign-in to the trip
/// back to the module that follows it, when the trip went well.
pub const THEN: &str = "; then ";

/// How the same outcome reads when the trip back failed: the runner's own
/// sentence first, then the sign-in in brackets closed by `)`. First, so
/// that whether the trip failed is read from where the words sit and never
/// searched for - a sign-in's words can carry a page's dialog, and a page
/// can say anything.
pub const AFTER_SIGN_IN: &str = " (after the sign-in: ";

/// The failed-trip sentence a mid-script `sign_in`'s outcome begins with,
/// when its trip back to the module failed. Only meaningful for an outcome
/// that belongs to a `sign_in` action: anywhere else these words are the
/// page's or the script's (a dialog, a `check_text` value), never the
/// runner's. A failed sign-in's own words always begin with the runner's
/// ("the sign-in page did not open", "sign-in stopped at step"...), so a
/// lookalike further in never counts.
pub fn unreached_after_sign_in(detail: &str) -> Option<&str> {
    detail.starts_with(UNREACHED_PREFIX).then_some(detail)
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
        || reason.starts_with(UNRECORDED_AREA_START)
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
            if step.actions.iter().flat_map(Action::each).any(|a| matches!(a, Action::Navigate { .. } | Action::OpenTab { .. })) {
                return Err(format!("case {}: {}", sc.case_id, no_address(step.step_number)));
            }
        }
    }
    Ok(())
}

/// Every area a script names must be one the project has recorded: a run
/// would refuse the case anyway, so the script is refused when it is saved,
/// with the names it could have used. No `area`, or a blank one, is "the
/// case's Module" and always passes. Names the first case it finds.
pub fn check_areas(nav: &NavFile, scripts: &[CaseScript]) -> Result<(), String> {
    for sc in scripts {
        let Some(area) = sc.area_name() else {
            continue;
        };
        if find_area(nav, area).is_none() {
            let names: Vec<&str> = nav.modules.iter().map(ModulePath::name).collect();
            let recorded = if names.is_empty() {
                "no areas are recorded yet".to_string()
            } else {
                format!("recorded areas: {}", names.join(", "))
            };
            return Err(format!("case {}: {} ({recorded})", sc.case_id, unrecorded_area(area)));
        }
    }
    Ok(())
}

/// The project's rules for a script, against its own file: no address
/// while the switch is off (`check_no_addresses`), only recorded areas
/// (`check_areas`), marks within their limits (`marks::check_saved`), and
/// preconditions that name a flow, a stage and a value the project has
/// (`preconditions::check_saved`), and a setup and fixture placeholders
/// naming fixtures and outputs the project has (`setup::check_saved`).
/// The one call every save path makes (the
/// Script editor, a JSON import, the assistant's `save_autorun_script`, a
/// repair included).
pub fn check_project_rules(root: &Path, org: &str, project: &str, scripts: &[CaseScript]) -> Result<(), String> {
    let nav = load_nav(root, org, project)?;
    check_no_addresses(&nav, scripts)?;
    check_areas(&nav, scripts)?;
    super::marks::check_saved(scripts)?;
    super::preconditions::check_saved(root, org, project, scripts)?;
    super::setup::check_saved_all(root, org, project, scripts)
}

/// What an assistant's guide gains for this project: the module-screen
/// rule while the switch is off, and the recorded areas while there are
/// any. Empty when neither applies.
pub fn guide_section(nav: &NavFile) -> String {
    let mut out = String::new();
    if !nav.direct_urls {
        out.push_str(
            "## This project's runs start on the module screen\n\n\
             - The run signs in and goes to the case's module screen before step 1, by the menu path recorded in the app.\n\
             - The script starts there: its first action acts on the module screen.\n\
             - Never use `navigate`. This project refuses to save a script that opens a page by address; reach every other screen with clicks.\n\
             - A `sign_in` action lands on the home page, and the run brings the browser back to the module screen before the next action.\n",
        );
    }
    if !nav.modules.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&areas_section(nav));
    }
    out
}

/// The recorded areas, one line each: name - module - the address path it
/// lands on.
fn areas_section(nav: &NavFile) -> String {
    let mut out = String::from(
        "## This project's areas\n\n\
         Before step 1 a run signs in and goes to an area by the menu path recorded for it in the app. \
         Each line is one area: its name, the test case Module it belongs to, and the address path it lands on.\n\n",
    );
    for m in &nav.modules {
        out.push_str(&format!("- {} - {} - {}\n", m.name(), m.module.trim(), m.arrived));
    }
    out.push_str(
        "\n- A script with no `area` goes to its module's default area: the area named like the case's Module, or, when none is, the module's only area.\n\
         - Set `area` on the script, to one of the names above, whenever the case's screen is not its module's default area.\n\
         - A script that names an area not listed here is refused when it is saved.\n",
    );
    out
}

/// `check_path`'s sentence when the browser stopped answering mid sign-in.
pub const SIGN_IN_BROWSER_SILENT: &str =
    "the sign-in did not work: the browser did not respond - try again, and see Settings, Logs if it keeps happening";
/// `check_path`'s sentence when the sign-in itself failed.
pub const SIGN_IN_FAILED: &str =
    "the sign-in did not work: check the account and the sign-in recipe, and see Settings, Logs for the details";

/// The check a path must pass before it is saved, and what Try runs: sign
/// in as `account` in a fresh browser, go home, click each click, and land
/// on `arrived`. Ok carries the path reached; Err is the dialog's sentence.
///
/// The sign-in is cleared with `lease` first (`Held::hold`): an account
/// something else holds is refused at once, with the sentence that says
/// who. The caller keeps `lease` until the check's browser is closed.
///
/// A failed sign-in's own words can name the application's address (a
/// navigate that would not load says which), and the dialog names none: the
/// person gets one of two fixed sentences, chosen by whether the browser or
/// the sign-in failed, and the words go to the log.
pub async fn check_path<D: Driver>(
    d: &mut D,
    root: &Path,
    recipe: &SignInRecipe,
    account: &Account,
    path: &ModulePath,
    timing: &Timing,
    lease: &mut super::lease::Held,
) -> Result<String, String> {
    lease.hold(root, &account.key).await?;
    let signed = super::signin::sign_in(d, root, recipe, account, timing).await;
    if !signed.ok {
        // `sign_in` already hides the password in its detail; hiding it
        // again here keeps the log safe even if that ever changes.
        crate::applog::warn(format!(
            "module path check: signing in as {} did not work: {}",
            account.key,
            super::signin::redact(&signed.detail, account)
        ));
        return Err(if signed.harness { SIGN_IN_BROWSER_SILENT } else { SIGN_IN_FAILED }.to_string());
    }
    go_to_module(d, &Route::new(recipe, path.clone()), TripFrom::SignIn, timing).await.map_err(|f| f.for_dialog())
}
