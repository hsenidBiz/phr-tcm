//! The preview (which cases can be exported, and why not) and the
//! all-or-nothing write of raw specs, test-case sections and `index.json`
//! into a PHR-PLAYWRIGHT-AUTOMATION clone.
//!
//! This module only ever writes: `suites/<seg>/test-cases/<feature>.md`,
//! `suites/_generated/<file>.spec.ts` and `suites/_generated/index.json`,
//! plus the TCM-owned `docs/` files (the protected baseline, and the
//! TCM-tagged entries of `docs/app-quirks.md`).
//! It never touches `users.json`, `navigation.json`, `seed.spec.ts` or
//! `auth.setup.ts`, and never runs git, npm or Playwright.

use super::app_quirks::{merge_app_quirks, quirk_entries, APP_QUIRKS_REL};
use super::clone::{index_with, open, ClonedRepo};
use super::handover::{baseline_doc, handover_for, HandoverInput, BASELINE_DOC_REL};
use super::mapping::{self, ExportMap, Placement};
use super::test_case::{self, CaseDoc};
use super::translate::{self, RawSpecInput};
use crate::autorun::nav::{self, NavFile};
use crate::autorun::quirks::{self, Quirk};
use crate::autorun::recipe::{self, SignInRecipe};
use crate::autorun::{accounts, components, runner, setup, store, CaseScript, LocalRun};
use crate::browser::actions::Action;
use crate::browser::locator::Target;
use crate::pw_repo::presence::{self, Presence};
use crate::pw_repo::{layout, structure};
use crate::pw_repo::sync;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TMP_SUFFIX: &str = ".tcm-export-tmp";
/// `suites/_generated/seed.spec.ts` is the repo's own: never a raw spec's name.
const SEED_SPEC: &str = "seed.spec.ts";
const MAX_RAW_NAME: usize = 60;
const INDEX_REL: &str = "suites/_generated/index.json";

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PreviewCase {
    pub case_id: i32,
    pub title: String,
    pub exportable: bool,
    pub reason: Option<String>,
    pub seg: Option<String>,
    pub user_key: Option<String>,
    pub add_user_command: Option<String>,
    /// The repo already has this case (`pw_repo::presence`): the export
    /// skips it unless the person ticks it to replace it on purpose.
    pub in_repo: bool,
    /// Where the repo has it, short: "in competencies.md and specs/competencies.spec.ts".
    pub repo_note: Option<String>,
    /// The Sync record has an export of it by this app.
    pub from_app: bool,
    /// The repo has it under `## Excluded <id>`: the team left it out on
    /// purpose, so it is never exported and cannot be ticked. `repo_note`
    /// then says "Excluded in the repo (in <file>)".
    pub excluded: bool,
    /// Auto Run has a script for it. The dialog lists only cases that do:
    /// one without a script has nothing to export.
    pub has_script: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Preview {
    /// The active environment id: the key the dialog edits in `map.accounts`.
    pub environment: String,
    pub clone_ok: bool,
    pub clone_problem: Option<String>,
    pub user_keys: Vec<String>,
    pub areas: Vec<String>,
    pub accounts: Vec<String>,
    pub map: ExportMap,
    /// A first guess, from its name (`Placement::suggest`), for each area
    /// that one of the listed cases starts in and that is not placed yet;
    /// the person confirms it by saving.
    pub suggested: BTreeMap<String, Placement>,
    pub cases: Vec<PreviewCase>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ExportResult {
    /// Relative to the clone, forward slashes.
    pub files: Vec<String>,
    /// Case id and the raw spec file it is in.
    pub cases: Vec<(i32, String)>,
    /// Case id and the clone's user key its raw spec is run as (the seed's user).
    pub user_keys: Vec<(i32, String)>,
    /// `<module>/<feature>` pairs the clone's navigation.json lacks.
    pub missing_navigation: Vec<String>,
    /// Case id and the test-case file its section is in.
    pub sections: Vec<(i32, String)>,
    /// Set when the files were written but the Sync record could not be.
    pub record_error: Option<String>,
    /// Cases asked for but left out, each with why: only Sync's Add new
    /// leaves any out (`add_new_with`); every other write is all or nothing.
    pub skipped: Vec<(i32, String)>,
}

/// An export of nothing.
pub fn nothing_written() -> ExportResult {
    ExportResult {
        files: vec![],
        cases: vec![],
        user_keys: vec![],
        missing_navigation: vec![],
        sections: vec![],
        record_error: None,
        skipped: vec![],
    }
}

/// Everything read once per preview or write.
struct Ctx {
    /// Auto Run's store and the project: where a shared draft's current
    /// output is read from (`resolved_script`).
    root: PathBuf,
    org: String,
    project: String,
    clone: Result<ClonedRepo, String>,
    /// Each case's Module, as Azure DevOps has it: what Auto Run picks a
    /// script's area by when the script names none (`nav::route_for`).
    modules: BTreeMap<i32, String>,
    map: ExportMap,
    nav: NavFile,
    recipe: Option<SignInRecipe>,
    /// The project's quirks. A file that cannot be read stops a WRITE (the
    /// quirks would silently not travel), not a preview.
    quirks: Result<Vec<Quirk>, String>,
    env_id: String,
    /// `(key, username)` only; a password is never read out of an Account.
    accounts: Vec<(String, String)>,
    runs: Vec<LocalRun>,
}

impl Ctx {
    fn load(
        root: &Path,
        org: &str,
        project: &str,
        clone_path: &str,
        modules: &BTreeMap<i32, String>,
    ) -> Result<Ctx, String> {
        let env_id = crate::environments::active_id(root)?;
        let accounts = accounts::load_accounts_for(root, &env_id)?
            .into_iter()
            .map(|a| (a.key, a.username))
            .collect();
        let clone = if clone_path.trim().is_empty() {
            Err("no Playwright clone folder is chosen yet".to_string())
        } else {
            open(Path::new(clone_path.trim()))
        };
        Ok(Ctx {
            root: root.to_path_buf(),
            org: org.to_string(),
            project: project.to_string(),
            clone,
            modules: modules.clone(),
            map: mapping::load(root, org, project)?,
            nav: nav::load_nav(root, org, project)?,
            recipe: recipe::load_effective_recipe_if_any(root, org, project)?,
            quirks: quirks::load_quirks(root, org, project),
            env_id,
            accounts,
            runs: store::list_runs(root),
        })
    }
}

/// A shared draft (an API Templates fixture) a script reads a value from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedDraft {
    /// The fixture's id, as `{{fixture.<id>.<output>}}` names it.
    pub fixture: String,
    pub name: String,
}

/// Why a case that builds its own draft (a `setup`) is not exported;
/// `setup_stays_in_tcm` puts the fixture in front of it.
pub const SETUP_STAYS_IN_TCM: &str = "Playwright has no fixture mechanism - stays TCM-only";

/// The reason a script with a setup on `fixture` is never exported.
pub fn setup_stays_in_tcm(fixture: &str) -> String {
    format!("builds its own draft with TCM fixture `{fixture}`; {SETUP_STAYS_IN_TCM}")
}

/// What a case needs to be written.
struct Ready {
    /// The script as the raw spec is written from it: components expanded
    /// and every shared draft's value put in (`resolved_script`).
    script: CaseScript,
    /// The shared drafts the script reads, for the hand-over's Preconditions.
    shared_drafts: Vec<SharedDraft>,
    placement: Placement,
    user_key: String,
    area_clicks: Vec<Target>,
    /// Every area a `return_to_area` names, under the script's own spelling.
    by_name: BTreeMap<String, Vec<Target>>,
    origins: Vec<String>,
}

/// A case with no Module in a project with no areas: Auto Run runs it, but
/// nothing says where in the clone it goes.
const NO_PLACE: &str =
    "this project has no areas recorded and the case has no Module, so nothing says where it goes in the clone - set its Module in Azure DevOps";

/// Where the case starts, chosen exactly as an unattended run chooses it -
/// the script's own area, else the one named like the case's Module
/// (`nav::route_for`) - as the name it is placed by, the menu clicks, and
/// whether a recorded area was found. A project with no areas runs the case
/// from home with no menu clicks, and it is placed by its Module instead.
fn route(ctx: &Ctx, id: i32, script: &CaseScript) -> Result<(String, Vec<Target>, bool), String> {
    route_with(&ctx.nav, ctx.modules.get(&id).map(String::as_str).unwrap_or(""), script)
}

/// `route` for one case's Module.
fn route_with(nav_file: &NavFile, module: &str, script: &CaseScript) -> Result<(String, Vec<Target>, bool), String> {
    let module = module.trim();
    match nav::route_for(nav_file, script.area.as_deref(), Some(module), script.account.as_deref())? {
        Some(a) => Ok((a.name().to_string(), a.clicks.clone(), true)),
        None if module.is_empty() => Err(NO_PLACE.into()),
        None => Ok((module.to_string(), Vec::new(), false)),
    }
}

/// A `return_to_area` that names no area: it goes back to the case's own.
fn bare_return_to_area(script: &CaseScript) -> bool {
    script
        .steps
        .iter()
        .flat_map(|s| s.actions.iter().flat_map(|a| a.each()))
        .any(|a| matches!(a, Action::ReturnToArea { .. }) && a.area_named().is_none())
}

/// Where an area (or Module) is placed: its own entry, else one spelled
/// the same but for case and outer spaces.
fn placed<'m>(map: &'m ExportMap, name: &str) -> Option<&'m Placement> {
    map.areas
        .get(name)
        .or_else(|| map.areas.iter().find(|(k, _)| k.trim().eq_ignore_ascii_case(name.trim())).map(|(_, v)| v))
}

/// The names the dialog places: the recorded areas, or - in a project with
/// none - the selected cases' Modules, which is what such cases are placed by.
fn placement_keys(ctx: &Ctx, case_ids: &[i32]) -> Vec<String> {
    if !ctx.nav.modules.is_empty() {
        return ctx.nav.modules.iter().map(|m| m.name().to_string()).collect();
    }
    let mut out: Vec<String> = Vec::new();
    for id in case_ids {
        if let Some(m) = ctx.modules.get(id).map(|m| m.trim()).filter(|m| !m.is_empty()) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(m)) {
                out.push(m.to_string());
            }
        }
    }
    out
}

/// An index.json file name is used only if it is a bare `*.spec.ts` name.
fn bare_spec_name(f: &str) -> bool {
    !f.is_empty()
        && f.ends_with(".spec.ts")
        && f != ".spec.ts"
        && !f.contains(['/', '\\', ':'])
        && !f.contains("..")
        && !Path::new(f).is_absolute()
}

fn shell_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

fn title_case(kebab: &str) -> String {
    kebab
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kebab_name(title: &str, id: i32) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(MAX_RAW_NAME);
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out = format!("case-{id}");
    }
    out
}

/// The saved script with every `use_component` expanded into its
/// component's actions, the way the runner expands it before a run - the
/// raw spec is the script as it runs. `Err` is the reason the case cannot
/// be exported.
fn load_expanded(root: &Path, org: &str, project: &str, id: i32, runs: &[LocalRun]) -> Result<Option<CaseScript>, String> {
    let Some(script) = store::load_script(root, id)? else {
        return Ok(None);
    };
    let mut script = with_run_account(script, runs);
    for step in &mut script.steps {
        let (expanded, _) = components::expand_step(root, org, project, &step.actions)
            .map_err(|(_, why)| format!("step {}: {why}", step.step_number))?;
        step.actions = expanded.into_iter().map(|(a, _)| a).collect();
    }
    Ok(Some(script))
}

/// Every `{{name}}` still in `script`'s steps' values, in order.
fn placeholders_in(script: &CaseScript) -> Vec<String> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.extend(crate::api_templates::exec::placeholders(s)),
            serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            serde_json::Value::Object(map) => map.values().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&serde_json::to_value(&script.steps).unwrap_or_default(), &mut out);
    out
}

/// `script` with every `{{fixture.<id>.<output>}}` replaced by that shared
/// draft's current output (its newest successful run), and the drafts it
/// read. `Err` names the draft that has no current output, or any
/// placeholder TCM has no value for: a raw spec never carries one, since
/// Playwright would type it as text.
fn resolve_shared(root: &Path, org: &str, project: &str, script: CaseScript) -> Result<(CaseScript, Vec<SharedDraft>), String> {
    let left_over = |names: Vec<String>| {
        names.into_iter().next().map(|n| {
            format!("a step still holds {{{{{n}}}}}, which TCM has no value to put in - fix the script in Auto Run")
        })
    };
    // Anything but a shared draft's value is not ours to fill.
    if let Some(why) = left_over(placeholders_in(&script).into_iter().filter(|n| !n.starts_with("fixture.")).collect()) {
        return Err(why);
    }
    let fixtures = setup::fixture_values(root, org, project, &script.steps);
    let drafts = fixtures.iter().map(|(id, f)| SharedDraft { fixture: id.clone(), name: f.name.clone() }).collect();
    let resolved = setup::resolve(&script, &fixtures, &BTreeMap::new())?;
    match left_over(placeholders_in(&resolved)) {
        Some(why) => Err(why),
        None => Ok((resolved, drafts)),
    }
}

/// `script` as the export writes it: every shared draft's value put in
/// (`resolve_shared`).
pub fn resolved_script(root: &Path, org: &str, project: &str, script: CaseScript) -> Result<CaseScript, String> {
    resolve_shared(root, org, project, script).map(|(s, _)| s)
}

/// The account case `id` ran as on its newest run, when that run's verdict
/// is Passed - the account the case was proven with. A script with no Runs
/// as has only ever run under the account chosen when its run started, so
/// this is the only record of who it runs as.
pub fn run_account(runs: &[LocalRun], id: i32) -> Option<String> {
    let record = runs.iter().find_map(|r| r.cases.iter().find(|c| c.case_id == id))?;
    if record.verdict.trim() != "Passed" {
        return None;
    }
    record.account.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(str::to_string)
}

/// `script` with a missing Runs as filled from its newest passing run
/// (`run_account`); a script's own Runs as always wins.
fn with_run_account(mut script: CaseScript, runs: &[LocalRun]) -> CaseScript {
    if script.account.as_deref().map_or(true, |a| a.trim().is_empty()) {
        script.account = run_account(runs, script.case_id);
    }
    script
}

/// What an export reads of a case, fingerprinted (`sync::fingerprint`):
/// loaded once for a project, then asked about each case.
pub struct Fingerprinter {
    map: ExportMap,
    nav: NavFile,
    env_id: String,
    /// Read once: a script with no Runs as takes its newest passing run's.
    runs: Vec<LocalRun>,
}

impl Fingerprinter {
    pub fn load(root: &Path, org: &str, project: &str) -> Result<Fingerprinter, String> {
        Ok(Fingerprinter {
            map: mapping::load(root, org, project)?,
            nav: nav::load_nav(root, org, project)?,
            env_id: crate::environments::active_id(root)?,
            runs: store::list_runs(root),
        })
    }

    /// The fingerprint of case `id` as an export would read it now, placed
    /// by `module` when its script names no area: its title, its steps with
    /// every component expanded and every shared draft's value filled in,
    /// where it starts (the area and its menu clicks) and who it runs as (the
    /// account and the repo user it maps to). `Ok(None)` when it has no script. A script whose components
    /// cannot be expanded, or whose area cannot be found, is fingerprinted
    /// with that reason in place of what could not be read, so it differs
    /// from any fingerprint taken while it could.
    pub fn of(&self, root: &Path, org: &str, project: &str, id: i32, module: &str) -> Result<Option<sync::Fingerprint>, String> {
        let Some(saved) = store::load_script(root, id)? else {
            return Ok(None);
        };
        let saved = with_run_account(saved, &self.runs);
        // What the raw spec holds: components expanded, shared drafts filled in.
        let expanded = load_expanded(root, org, project, id, &self.runs)
            .and_then(|s| s.map(|s| resolved_script(root, org, project, s)).transpose());
        let (steps, title) = match expanded {
            Ok(Some(s)) => (serde_json::to_value(&s.steps).unwrap_or_default(), s.title),
            Ok(None) => return Ok(None),
            Err(why) => (serde_json::Value::String(format!("not expandable: {why}")), saved.title.clone()),
        };
        let (area, clicks) = match route_with(&self.nav, module, &saved) {
            Ok((name, clicks, _)) => (name, serde_json::to_value(&clicks).unwrap_or_default()),
            Err(why) => (format!("no area: {why}"), serde_json::Value::Null),
        };
        let account = saved.account.as_deref().map(str::trim).unwrap_or("").to_string();
        let user_key = self.map.accounts.get(&self.env_id).and_then(|m| m.get(&account)).cloned();
        Ok(Some(sync::fingerprint(&sync::FingerprintInput { title, steps, area, area_clicks: clicks, account, user_key })))
    }
}

fn evaluate(ctx: &Ctx, id: i32, script: Result<Option<CaseScript>, String>) -> (PreviewCase, Option<Ready>) {
    let mut pc = PreviewCase {
        case_id: id,
        title: String::new(),
        exportable: false,
        reason: None,
        seg: None,
        user_key: None,
        add_user_command: None,
        in_repo: false,
        repo_note: None,
        from_app: false,
        excluded: false,
        has_script: false,
    };
    let fail = |mut pc: PreviewCase, why: String| {
        pc.reason = Some(why);
        (pc, None)
    };

    let script = match script {
        Ok(s) => s,
        // A script that is there but cannot be read is still a script.
        Err(why) => {
            pc.has_script = true;
            return fail(pc, why);
        }
    };
    let Some(script) = script else {
        return fail(pc, "no script - record or write one in Auto Run".into());
    };
    pc.has_script = true;
    pc.title = script.title.clone();

    // The person's verdict on the newest run that holds the case.
    if let Some(why) = verdict_fails(&ctx.runs, id) {
        return fail(pc, why);
    }

    // A case that builds its own draft cannot be written for Playwright.
    if let Some(setup) = &script.setup {
        return fail(pc, setup_stays_in_tcm(&setup.fixture));
    }
    // The values the shared drafts hold now go into the spec.
    let (script, shared_drafts) = match resolve_shared(&ctx.root, &ctx.org, &ctx.project, script) {
        Ok(r) => r,
        Err(why) => return fail(pc, why),
    };

    let (area_name, area_clicks, routed) = match route(ctx, id, &script) {
        Ok(r) => r,
        Err(why) => return fail(pc, why),
    };
    if !routed && bare_return_to_area(&script) {
        return fail(pc, runner::NO_AREA_IN_RUN.into());
    }
    // The case's own folder, when it has one, else its area's.
    let placement = match ctx.map.cases.get(&id) {
        Some(own) => {
            if let Err(e) = own.validate() {
                return fail(pc, format!("this case's own folder is not valid: {e}"));
            }
            own.clone()
        }
        None => {
            let Some(placement) = placed(&ctx.map, &area_name).cloned() else {
                return fail(pc, format!("area \"{area_name}\" is not placed yet - choose where it goes in the clone"));
            };
            if let Err(e) = placement.validate() {
                return fail(pc, format!("area \"{area_name}\" has a placement that is not valid: {e}"));
            }
            placement
        }
    };
    pc.seg = Some(placement.seg().to_string());

    // Account.
    let Some(account) = script.account.as_deref().map(str::trim).filter(|a| !a.is_empty()) else {
        return fail(pc, "script has no account".into());
    };
    let Some(user_key) = ctx.map.accounts.get(&ctx.env_id).and_then(|m| m.get(account)).cloned() else {
        return fail(pc, format!("account \"{account}\" is not mapped to a repo user"));
    };
    pc.user_key = Some(user_key.clone());
    let repo = match &ctx.clone {
        Ok(r) => r,
        Err(problem) => return fail(pc, problem.clone()),
    };
    if let Some((_, f)) = repo.index.iter().find(|(k, _)| *k == id.to_string()) {
        if f.eq_ignore_ascii_case(SEED_SPEC) {
            return fail(pc, format!("index.json maps case {id} to \"{f}\", the repo's seed spec, which the export never writes"));
        }
        if !bare_spec_name(f) {
            return fail(pc, format!("index.json maps case {id} to \"{f}\", which is not a plain .spec.ts file name"));
        }
    }
    if !repo.user_keys.contains(&user_key) {
        let username = ctx
            .accounts
            .iter()
            .find(|(k, _)| k == account)
            .map(|(_, u)| u.clone())
            .unwrap_or_else(|| "<username>".to_string());
        pc.add_user_command =
            Some(format!(
                "npm run users -- add {} --username {} --password <password> --apply",
                shell_quote(&user_key),
                shell_quote(&username)
            ));
        return fail(pc, format!("repo user \"{user_key}\" is not in the clone's users.json"));
    }

    // Translation.
    let Some(recipe) = &ctx.recipe else {
        return fail(pc, "no sign-in recipe for this project".into());
    };
    let origins = recipe.origins();
    if let Err(e) = translate::check(&script, &origins) {
        return fail(pc, e.0);
    }
    // The same names Auto Run resolves (nav::find_area), under the script's spelling.
    let mut by_name: BTreeMap<String, Vec<Target>> = BTreeMap::new();
    for step in &script.steps {
        for a in step.actions.iter().flat_map(|a| a.each()) {
            if let Some(n) = a.area_named() {
                match nav::find_area(&ctx.nav, n) {
                    Some(m) => {
                        by_name.insert(n.to_string(), m.clicks.clone());
                    }
                    None => return fail(pc, format!("return_to_area names the area \"{n}\", which is not recorded in Auto Run")),
                }
            }
        }
    }
    // Run the real translation so the preview agrees with the write. The
    // paths and step texts only reach comments.
    let trial = translate::raw_spec(&RawSpecInput {
        script: &script,
        md_path: format!("suites/{}/test-cases/{}.md", placement.seg(), placement.feature()),
        feature_title: title_case(placement.feature()),
        after_sign_in: &recipe.after_sign_in,
        area_clicks: &area_clicks,
        area_clicks_by_name: &by_name,
        step_texts: &BTreeMap::new(),
        origins: &origins,
    });
    if let Err(e) = trial {
        return fail(pc, e.0);
    }

    pc.exportable = true;
    let ready = Ready { script, shared_drafts, placement, user_key, area_clicks, by_name, origins };
    (pc, Some(ready))
}

/// The repo user key case `id`'s script account maps to in the active
/// environment, by the export's own mapping (what `evaluate` uses), if any.
/// Reads only local files. `runs` is `store::list_runs`, read once by the
/// caller: a script with no Runs as takes its newest passing run's account.
pub fn mapped_user_key(root: &Path, org: &str, project: &str, id: i32, runs: &[LocalRun]) -> Option<String> {
    let env_id = crate::environments::active_id(root).ok()?;
    let map = mapping::load(root, org, project).ok()?;
    let script = with_run_account(store::load_script(root, id).ok()??, runs);
    let account = script.account.as_deref().map(str::trim).filter(|a| !a.is_empty())?;
    map.accounts.get(&env_id).and_then(|m| m.get(account)).cloned()
}

/// `modules` is each case's Module (case id to value), as the Auto Run
/// screen's rows have it.
pub fn preview_with(
    root: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
) -> Result<Preview, String> {
    // A failure here is not the clone's: it is the caller's error.
    let ctx = Ctx::load(root, org, project, clone_path, modules)?;
    let found = ctx.clone.as_ref().map(presence::scan_repo).unwrap_or_default();
    let mut cases = Vec::new();
    // A guess only for an area one of these cases starts in and that is
    // not placed yet - not for every area the project has recorded.
    let mut suggested: BTreeMap<String, Placement> = BTreeMap::new();
    let features: Vec<Vec<String>> = ctx
        .clone
        .as_ref()
        .map(|c| crate::pw_repo::layout::feature_dirs(&c.root.join("suites")))
        .unwrap_or_default();
    for &id in case_ids {
        let script = load_expanded(root, org, project, id, &ctx.runs);
        if let Some(Ok((name, _, _))) = script.as_ref().ok().and_then(Option::as_ref).map(|s| route(&ctx, id, s)) {
            if placed(&ctx.map, &name).is_none() && !suggested.keys().any(|k| k.eq_ignore_ascii_case(&name)) {
                if let Some(p) = Placement::suggest(&name, &features) {
                    suggested.insert(name, p);
                }
            }
        }
        let mut pc = evaluate(&ctx, id, script).0;
        pc.in_repo = found.has(id);
        pc.excluded = found.excluded(id);
        pc.repo_note = found.excluded_note(id).or_else(|| found.note(id));
        if pc.excluded {
            pc.exportable = false;
        }
        cases.push(pc);
    }
    Ok(Preview {
        environment: ctx.env_id.clone(),
        clone_ok: ctx.clone.is_ok(),
        clone_problem: ctx.clone.as_ref().err().cloned(),
        user_keys: ctx.clone.as_ref().map(|c| c.user_keys.clone()).unwrap_or_default(),
        suggested,
        areas: placement_keys(&ctx, case_ids),
        accounts: ctx.accounts.iter().map(|(k, _)| k.clone()).collect(),
        map: ctx.map.clone(),
        cases,
    })
}

/// `preview_with`, with each case the repo has marked as this app's export
/// or not, from the Sync record in `sync_dir`.
pub fn preview_recorded(
    root: &Path,
    sync_dir: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
) -> Result<Preview, String> {
    let mut p = preview_with(root, org, project, case_ids, clone_path, modules)?;
    let record = sync::load(&sync::record_path(sync_dir, clone_path, org, project))?;
    for c in p.cases.iter_mut() {
        c.from_app = c.in_repo && record.cases.contains_key(&c.case_id);
    }
    Ok(p)
}

/// The guard on writing over what the repo has: every case in `case_ids`
/// the repo already has (`pw_repo::presence`) must be in `replace`, the
/// cases the person chose to replace on purpose. What the webview ticks by
/// default is not enough on its own. A case the repo has under
/// `## Excluded <id>` is refused even in `replace`.
pub fn refuse_present(found: &Presence, case_ids: &[i32], replace: &[i32]) -> Result<(), String> {
    for &id in case_ids {
        if let Some(note) = found.excluded_note(id) {
            return Err(format!("case {id} cannot be exported: {note}. Nothing was written."));
        }
        if found.has(id) && !replace.contains(&id) {
            let note = found.note(id).unwrap_or_default();
            return Err(format!(
                "case {id} is already in the repo ({note}). Tick it to replace it on purpose. Nothing was written."
            ));
        }
    }
    Ok(())
}

/// `case_ids` split into the ones that could be exported on their own and
/// the rest, each with why not: the repo already has it, or the reason the
/// preview gives. Reads only local files.
pub fn split_exportable(
    root: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
) -> Result<(Vec<i32>, Vec<(i32, String)>), String> {
    let ctx = Ctx::load(root, org, project, clone_path, modules)?;
    let repo = ctx.clone.as_ref().map_err(|e| e.clone())?;
    let found = presence::scan_repo(repo);
    let (mut ok, mut skipped): (Vec<i32>, Vec<(i32, String)>) = (Vec::new(), Vec::new());
    for &id in case_ids {
        if ok.contains(&id) || skipped.iter().any(|(s, _)| *s == id) {
            continue;
        }
        if let Some(note) = found.excluded_note(id) {
            skipped.push((id, note));
            continue;
        }
        if let Some(note) = found.note(id) {
            skipped.push((id, format!("already in the repo ({note})")));
            continue;
        }
        match evaluate(&ctx, id, load_expanded(root, org, project, id, &ctx.runs)) {
            (_, Some(_)) => ok.push(id),
            (pc, None) => skipped.push((id, pc.reason.unwrap_or_default())),
        }
    }
    Ok((ok, skipped))
}

/// Sync's Add new: of `case_ids` (the new cases, `sync::add_new`), the ones
/// that can be exported on their own are written together, all or nothing
/// as one write with nothing to replace (`write_recorded`); every other one
/// is left out and listed in `skipped` with why, so one case that cannot go
/// does not hold back the rest. `unread` is the cases Azure DevOps could
/// not give: each is left out the same way, as is one missing from `docs`.
#[allow(clippy::too_many_arguments)]
pub fn add_new_with(
    root: &Path,
    sync_dir: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
    docs: &BTreeMap<i32, CaseDoc>,
    unread: &[i32],
) -> Result<ExportResult, String> {
    let checked: Vec<i32> = case_ids.iter().copied().filter(|id| !unread.contains(id)).collect();
    let (ok, mut skipped) = split_exportable(root, org, project, &checked, clone_path, modules)?;
    let (ok, missing): (Vec<i32>, Vec<i32>) = ok.into_iter().partition(|id| docs.contains_key(id));
    skipped.extend(unread.iter().copied().chain(missing).map(|id| (id, NOT_READ.to_string())));
    skipped.sort_by_key(|(id, _)| case_ids.iter().position(|c| c == id));
    let mut result = if ok.is_empty() {
        nothing_written()
    } else {
        write_recorded(root, sync_dir, org, project, &ok, clone_path, modules, docs, &[])?
    };
    result.skipped = skipped;
    Ok(result)
}

/// Why Add new left out a case Azure DevOps could not give.
pub const NOT_READ: &str = "it could not be read from Azure DevOps. Settings, Logs has the details";

/// Of the new cases, the ones worth reading from Azure DevOps for Add new:
/// those with a script whose newest run Passed. The rest cannot be exported
/// whatever Azure DevOps says (`evaluate` checks the same first), so they
/// are not read and `add_new_with` gives their reason.
pub fn worth_reading(root: &Path, case_ids: &[i32]) -> Vec<i32> {
    let runs = store::list_runs(root);
    case_ids
        .iter()
        .copied()
        .filter(|&id| matches!(store::load_script(root, id), Ok(Some(_))) && verdict_fails(&runs, id).is_none())
        .collect()
}

/// Why the newest run that holds case `id` keeps it from being exported:
/// `None` when the person marked it Passed.
fn verdict_fails(runs: &[LocalRun], id: i32) -> Option<String> {
    let record = runs.iter().find_map(|r| r.cases.iter().find(|c| c.case_id == id));
    let verdict = record.map(|c| c.verdict.trim()).unwrap_or("");
    if verdict == "Passed" {
        return None;
    }
    let said = if verdict.is_empty() { "not reviewed" } else { verdict };
    Some(format!("latest run: {said}"))
}

/// Every selected case, or the first reason one cannot be exported. Reads
/// only local files, so a caller can refuse before any network call.
fn eligible(ctx: &Ctx, root: &Path, org: &str, project: &str, case_ids: &[i32]) -> Result<Vec<(i32, Ready)>, String> {
    let mut ready: Vec<(i32, Ready)> = Vec::new();
    for &id in case_ids {
        if ready.iter().any(|(i, _)| *i == id) {
            continue;
        }
        let script = load_expanded(root, org, project, id, &ctx.runs);
        match evaluate(ctx, id, script) {
            (_, Some(r)) => ready.push((id, r)),
            (pc, None) => {
                return Err(format!(
                    "case {id} cannot be exported: {}. Nothing was written.",
                    pc.reason.unwrap_or_default()
                ))
            }
        }
    }
    Ok(ready)
}

/// `replace` is the cases the person chose to replace (`refuse_present`).
#[allow(clippy::too_many_arguments)]
pub fn ensure_exportable(
    root: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
    replace: &[i32],
) -> Result<(), String> {
    let ctx = Ctx::load(root, org, project, clone_path, modules)?;
    let repo = ctx.clone.as_ref().map_err(|e| e.clone())?;
    refuse_present(&presence::scan_repo(repo), case_ids, replace)?;
    eligible(&ctx, root, org, project, case_ids).map(|_| ())
}

struct Out {
    rel: String,
    content: String,
}

/// Writes `case_ids`, all or nothing. A case the repo already has is
/// refused unless it is in `replace` (`refuse_present`).
#[allow(clippy::too_many_arguments)]
pub fn write_with(
    root: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
    docs: &BTreeMap<i32, CaseDoc>,
    replace: &[i32],
) -> Result<ExportResult, String> {
    let ctx = Ctx::load(root, org, project, clone_path, modules)?;
    let repo = ctx.clone.as_ref().map_err(|e| e.clone())?;
    let found = presence::scan_repo(repo);
    refuse_present(&found, case_ids, replace)?;

    // Refuse everything unless every case is exportable.
    let ready = eligible(&ctx, root, org, project, case_ids)?;
    if let Some((id, _)) = ready.iter().find(|(id, _)| !docs.contains_key(id)) {
        return Err(format!("case {id} could not be read from Azure DevOps. Nothing was written."));
    }

    let recipe = ctx.recipe.as_ref().ok_or("no sign-in recipe for this project")?;

    let mut md_files: BTreeMap<String, String> = BTreeMap::new();
    let mut raw_files: Vec<Out> = Vec::new();
    let mut cases: Vec<(i32, String)> = Vec::new();
    let mut sections: Vec<(i32, String)> = Vec::new();
    let mut missing_navigation: Vec<String> = Vec::new();
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    let mut chosen_by: BTreeMap<String, i32> = BTreeMap::new();
    let mut index = repo.index.clone();

    for (id, r) in &ready {
        let id = *id;
        // A case the repo already has is rewritten where its section is, not
        // where the saved placement says: the repo may have moved the feature.
        let existing: Vec<&String> = found
            .cases
            .get(&id)
            .map(|f| f.sections.iter().filter(|p| structure::feature_test_cases(p)).collect())
            .unwrap_or_default();
        let here: Option<(String, Vec<String>)> = match existing.as_slice() {
            [] => None,
            [one] => layout::test_case_file(one).map(|path| ((*one).clone(), path)),
            several => {
                let names: Vec<&str> = several.iter().map(|p| p.as_str()).collect();
                return Err(format!(
                    "Case {id} has a section in more than one test-case file: {}. Nothing was written.",
                    names.join(", ")
                ));
            }
        };
        let (md_rel, folders, feature, nav_key) = match here {
            Some((rel, path)) => {
                let feature = path.last().cloned().unwrap_or_default();
                let key = layout::navigation_key(&path);
                (rel, path.join("/"), feature, key)
            }
            None => {
                let p = &r.placement;
                (
                    format!("suites/{}/test-cases/{}.md", p.seg(), p.feature()),
                    p.seg().to_string(),
                    p.feature().to_string(),
                    p.navigation_key(),
                )
            }
        };
        let captured = repo.navigation_keys.contains(&nav_key);
        if !captured && !missing_navigation.contains(&nav_key) {
            missing_navigation.push(nav_key);
        }
        let feature_title = title_case(&feature);

        let mut doc = docs[&id].clone();
        doc.folder = folders;
        doc.navigation_captured = captured;
        doc.feature_title = feature_title.clone();
        doc.handover = handover_for(&HandoverInput {
            script: &r.script,
            shared_drafts: &r.shared_drafts,
            protected: &ctx.nav.protected,
        });

        // Several cases of one feature accumulate into one file in memory.
        if !md_files.contains_key(&md_rel) {
            let path = repo.root.join(&md_rel);
            let start = if path.is_file() {
                std::fs::read_to_string(&path).map_err(|e| format!("could not read {md_rel}: {e}"))?
            } else {
                test_case::new_file(&feature_title, &feature, &r.user_key)
            };
            md_files.insert(md_rel.clone(), start);
        }
        let current = md_files.get_mut(&md_rel).expect("inserted above");
        *current = test_case::splice(current, id, &test_case::section(&doc));
        sections.push((id, md_rel.clone()));

        // The raw file: the index's own for this id, else a fresh unique name.
        let key = id.to_string();
        let file = match index.iter().find(|(k, _)| *k == key) {
            Some((_, f)) => f.clone(),
            None => {
                let base = kebab_name(&r.script.title, id);
                let taken = |n: &str| {
                    n.eq_ignore_ascii_case(SEED_SPEC)
                        || repo.generated_files.iter().any(|g| g == n)
                        || repo.index.iter().any(|(_, f)| f == n)
                        || chosen.contains(n)
                };
                let mut name = format!("{base}.spec.ts");
                let mut n = 2;
                while taken(&name) {
                    name = format!("{base}-{n}.spec.ts");
                    n += 1;
                }
                name
            }
        };
        if let Some(other) = chosen_by.insert(file.clone(), id) {
            if other != id {
                return Err(format!("cases {other} and {id} would both be written to {file}. Nothing was written."));
            }
        }
        chosen.insert(file.clone());

        let step_texts: BTreeMap<i32, String> =
            doc.steps.iter().enumerate().map(|(i, (action, _))| (i as i32 + 1, action.clone())).collect();
        let spec = translate::raw_spec(&RawSpecInput {
            script: &r.script,
            md_path: md_rel,
            feature_title,
            after_sign_in: &recipe.after_sign_in,
            area_clicks: &r.area_clicks,
            area_clicks_by_name: &r.by_name,
            step_texts: &step_texts,
            origins: &r.origins,
        })
        .map_err(|e| format!("case {id} cannot be exported: {}. Nothing was written.", e.0))?;
        raw_files.push(Out { rel: format!("suites/_generated/{file}"), content: spec });

        match index.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = file.clone(),
            None => index.push((key, file.clone())),
        }
        cases.push((id, file));
    }

    // Order matters for the rename phase: test-case files, then raw specs,
    // then index.json, so the index never points at a file that is missing.
    let mut outs: Vec<Out> = md_files.into_iter().map(|(rel, content)| Out { rel, content }).collect();
    outs.extend(raw_files);
    // The protected baseline, whole, before index.json. An empty baseline
    // writes nothing and leaves an earlier export's file alone.
    if let Some(content) = baseline_doc(&ctx.nav.protected) {
        outs.push(Out { rel: BASELINE_DOC_REL.into(), content });
    }
    // TCM's quirks and the recipe's pop-ups and loading indicators, merged
    // into the repo's own catalogue before index.json. Nothing to say, or
    // nothing changed, writes nothing.
    let quirks = ctx.quirks.as_ref().map_err(|e| format!("{e}. Nothing was written."))?;
    let case_ids: Vec<i32> = ready.iter().map(|(id, _)| *id).collect();
    let entries = quirk_entries(quirks, &case_ids, Some(recipe));
    let quirks_path = repo.root.join(APP_QUIRKS_REL);
    let existing_quirks = if quirks_path.is_file() {
        Some(std::fs::read_to_string(&quirks_path).map_err(|e| format!("could not read {APP_QUIRKS_REL}: {e}"))?)
    } else {
        None
    };
    if let Some(content) = merge_app_quirks(existing_quirks.as_deref(), &entries) {
        outs.push(Out { rel: APP_QUIRKS_REL.into(), content });
    }
    // index.json only when an entry is new or moved: a re-export of cases
    // it already maps leaves the repo's file as it is, byte for byte.
    let last_id = ready.last().map(|(i, _)| *i);
    if let (Some(id), Some((_, file)), true) = (last_id, cases.last(), index != repo.index) {
        outs.push(Out { rel: INDEX_REL.into(), content: index_with(&index, id, file) });
    }

    commit(&repo.root, &outs)?;
    let user_keys = ready.iter().map(|(id, r)| (*id, r.user_key.clone())).collect();
    Ok(ExportResult {
        files: outs.into_iter().map(|o| o.rel).collect(),
        cases,
        user_keys,
        missing_navigation,
        sections,
        record_error: None,
        skipped: vec![],
    })
}

/// `write_with`, then the Sync record (`pw_repo::sync`) for every case
/// written: its saved script's fingerprint as it was read before the write,
/// the time, and its files. `sync_dir` is the records' folder. A refused
/// write records nothing. A record that cannot be saved after the files
/// were written does not undo them: it is logged and said in
/// `record_error`.
#[allow(clippy::too_many_arguments)]
pub fn write_recorded(
    root: &Path,
    sync_dir: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
    docs: &BTreeMap<i32, CaseDoc>,
    replace: &[i32],
) -> Result<ExportResult, String> {
    // Taken before the write: a script changed while it runs shows as out of date.
    let fingerprints = Fingerprinter::load(root, org, project)?;
    let mut saved: BTreeMap<i32, (String, sync::Fingerprint)> = BTreeMap::new();
    for &id in case_ids {
        let module = modules.get(&id).map(String::as_str).unwrap_or("");
        if let (Ok(Some(s)), Ok(Some(fp))) = (store::load_script(root, id), fingerprints.of(root, org, project, id, module)) {
            saved.insert(id, (s.title, fp));
        }
    }
    let mut result = write_with(root, org, project, case_ids, clone_path, modules, docs, replace)?;
    let at = crate::applog::iso_stamp();
    let entries: Vec<(i32, sync::Exported)> = result
        .cases
        .iter()
        .filter_map(|(id, file)| {
            let (title, fingerprint) = saved.get(id)?;
            let mut files: Vec<String> = result.sections.iter().filter(|(i, _)| i == id).map(|(_, f)| f.clone()).collect();
            files.push(format!("suites/_generated/{file}"));
            if result.files.iter().any(|f| f == INDEX_REL) {
                files.push(INDEX_REL.into());
            }
            Some((
                *id,
                sync::Exported {
                    title: title.clone(),
                    fingerprint: fingerprint.clone(),
                    exported_at: at.clone(),
                    files,
                    module: modules.get(id).cloned().unwrap_or_default(),
                    user_key: result.user_keys.iter().find(|(i, _)| i == id).map(|(_, k)| k.clone()).unwrap_or_default(),
                },
            ))
        })
        .collect();
    if let Err(e) = sync::record(&sync::record_path(sync_dir, clone_path, org, project), entries) {
        crate::applog::warn(format!("pw-export: the Sync record was not saved: {e}"));
        result.record_error =
            Some("The files were written, but the Sync record could not be saved, so Sync may not show these cases as current. Settings → Logs has the details.".into());
    }
    Ok(result)
}


fn tmp_of(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(TMP_SUFFIX);
    target.with_file_name(name)
}

/// All-or-nothing as far as the file system allows: every file is first
/// written beside its target as `<name>.tcm-export-tmp` (a failure there
/// removes every temp and every directory this call created that is still
/// empty), then the temps are renamed over their targets in the given order.
/// A failure DURING the rename phase removes the temps still left but cannot
/// undo renames already made, so earlier files may be in place; the order
/// (test-case files, raw specs, index.json last) keeps the index from ever
/// naming a file that does not exist.
fn commit(root: &Path, outs: &[Out]) -> Result<(), String> {
    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut created: Vec<PathBuf> = Vec::new();

    let cleanup = |temps: &[(PathBuf, PathBuf)], created: &mut Vec<PathBuf>| {
        for (tmp, _) in temps {
            let _ = std::fs::remove_file(tmp);
        }
        created.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for d in created.iter() {
            let _ = std::fs::remove_dir(d); // only succeeds while empty
        }
    };

    for o in outs {
        let target = root.join(&o.rel);
        if let Some(parent) = target.parent() {
            let mut missing: Vec<PathBuf> = Vec::new();
            let mut p = parent;
            while !p.exists() {
                missing.push(p.to_path_buf());
                match p.parent() {
                    Some(up) => p = up,
                    None => break,
                }
            }
            if let Err(e) = std::fs::create_dir_all(parent) {
                created.extend(missing);
                cleanup(&temps, &mut created);
                return Err(format!("could not create {}: {e}. Nothing was changed.", parent.display()));
            }
            created.extend(missing);
        }
        let tmp = tmp_of(&target);
        if let Err(e) = std::fs::write(&tmp, &o.content) {
            let _ = std::fs::remove_file(&tmp);
            cleanup(&temps, &mut created);
            return Err(format!("could not write {}: {e}. Nothing was changed.", o.rel));
        }
        temps.push((tmp, target));
    }

    // A locked or read-only target fails here, before anything is replaced.
    for (_, target) in &temps {
        if target.is_file() {
            if let Err(e) = std::fs::OpenOptions::new().write(true).open(target) {
                cleanup(&temps, &mut created);
                return Err(format!("could not write {}: {e}. Nothing was changed.", target.display()));
            }
        }
    }

    for (i, (tmp, target)) in temps.iter().enumerate() {
        if let Err(e) = std::fs::rename(tmp, target) {
            cleanup(&temps[i..], &mut created);
            return Err(format!(
                "could not replace {}: {e}. Files before it may already have been replaced.",
                target.display()
            ));
        }
    }
    Ok(())
}
