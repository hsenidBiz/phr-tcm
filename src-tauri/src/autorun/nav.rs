//! Module paths: how an unattended run gets from the application's home
//! page to a case's module screen, and the per-project switch that says
//! whether scripts may open pages by address.
//!
//! One file per project, beside the sign-in recipe:
//! `<autorun root>/projects/<slug>.nav.json`. It is kept on this machine
//! only and never sent to Azure DevOps. A project with no file, or with an
//! empty `modules` list, runs exactly as it did before module paths
//! existed.

use super::recipe::{origin_of, project_slug};
use crate::browser::locator::Target;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
