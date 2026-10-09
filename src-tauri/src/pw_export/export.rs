//! The preview (which cases can be exported, and why not) and the
//! all-or-nothing write of raw specs, test-case sections and `index.json`
//! into a PHR-PLAYWRIGHT-AUTOMATION clone.
//!
//! This module only ever writes: `suites/<seg>/test-cases/<feature>.md`,
//! `suites/_generated/<file>.spec.ts` and `suites/_generated/index.json`.
//! It never touches `users.json`, `navigation.json`, `seed.spec.ts` or
//! `auth.setup.ts`, and never runs git, npm or Playwright.

use super::clone::{index_with, open, ClonedRepo};
use super::mapping::{self, ExportMap, Placement};
use super::test_case::{self, CaseDoc};
use super::translate::{self, RawSpecInput};
use crate::autorun::nav::{self, NavFile};
use crate::autorun::recipe::{self, SignInRecipe};
use crate::autorun::{accounts, components, runner, store, CaseScript, LocalRun};
use crate::browser::actions::Action;
use crate::browser::locator::Target;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TMP_SUFFIX: &str = ".tcm-export-tmp";
/// `suites/_generated/seed.spec.ts` is the repo's own: never a raw spec's name.
const SEED_SPEC: &str = "seed.spec.ts";
const MAX_RAW_NAME: usize = 60;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PreviewCase {
    pub case_id: i32,
    pub title: String,
    pub exportable: bool,
    pub reason: Option<String>,
    pub seg: Option<String>,
    pub user_key: Option<String>,
    pub add_user_command: Option<String>,
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
}

/// Everything read once per preview or write.
struct Ctx {
    clone: Result<ClonedRepo, String>,
    /// Each case's Module, as Azure DevOps has it: what Auto Run picks a
    /// script's area by when the script names none (`nav::route_for`).
    modules: BTreeMap<i32, String>,
    map: ExportMap,
    nav: NavFile,
    recipe: Option<SignInRecipe>,
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
            clone,
            modules: modules.clone(),
            map: mapping::load(root, org, project)?,
            nav: nav::load_nav(root, org, project)?,
            recipe: recipe::load_effective_recipe_if_any(root, org, project)?,
            env_id,
            accounts,
            runs: store::list_runs(root),
        })
    }
}

/// What a case needs to be written.
struct Ready {
    script: CaseScript,
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
    let module = ctx.modules.get(&id).map(|m| m.trim()).unwrap_or("");
    match nav::route_for(&ctx.nav, script.area.as_deref(), Some(module), script.account.as_deref())? {
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
fn load_expanded(root: &Path, org: &str, project: &str, id: i32) -> Result<Option<CaseScript>, String> {
    let Some(mut script) = store::load_script(root, id)? else {
        return Ok(None);
    };
    for step in &mut script.steps {
        let (expanded, _) = components::expand_step(root, org, project, &step.actions)
            .map_err(|(_, why)| format!("step {}: {why}", step.step_number))?;
        step.actions = expanded.into_iter().map(|(a, _)| a).collect();
    }
    Ok(Some(script))
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
    };
    let fail = |mut pc: PreviewCase, why: String| {
        pc.reason = Some(why);
        (pc, None)
    };

    let script = match script {
        Ok(s) => s,
        Err(why) => return fail(pc, why),
    };
    let Some(script) = script else {
        return fail(pc, "no script - record or write one in Auto Run".into());
    };
    pc.title = script.title.clone();

    // The person's verdict on the newest run that holds the case.
    let record = ctx.runs.iter().find_map(|r| r.cases.iter().find(|c| c.case_id == id));
    let verdict = record.map(|c| c.verdict.trim()).unwrap_or("");
    if verdict != "Passed" {
        let said = if verdict.is_empty() { "not reviewed" } else { verdict };
        return fail(pc, format!("latest run: {said}"));
    }

    let (area_name, area_clicks, routed) = match route(ctx, id, &script) {
        Ok(r) => r,
        Err(why) => return fail(pc, why),
    };
    if !routed && bare_return_to_area(&script) {
        return fail(pc, runner::NO_AREA_IN_RUN.into());
    }
    let placement = placed(&ctx.map, &area_name).cloned();
    let Some(placement) = placement else {
        return fail(pc, format!("area \"{area_name}\" is not placed yet - choose where it goes in the clone"));
    };
    if let Err(e) = placement.validate() {
        return fail(pc, format!("area \"{area_name}\" has a placement that is not valid: {e}"));
    }
    pc.seg = Some(placement.seg());

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
        md_path: format!("suites/{}/test-cases/{}.md", placement.seg(), placement.feature),
        feature_title: title_case(&placement.feature),
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
    let ready = Ready { script, placement, user_key, area_clicks, by_name, origins };
    (pc, Some(ready))
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
    let mut cases = Vec::new();
    // A guess only for an area one of these cases starts in and that is
    // not placed yet - not for every area the project has recorded.
    let mut suggested: BTreeMap<String, Placement> = BTreeMap::new();
    for &id in case_ids {
        let script = load_expanded(root, org, project, id);
        if let Some(Ok((name, _, _))) = script.as_ref().ok().and_then(Option::as_ref).map(|s| route(&ctx, id, s)) {
            if placed(&ctx.map, &name).is_none() && !suggested.keys().any(|k| k.eq_ignore_ascii_case(&name)) {
                let p = Placement::suggest(&name);
                suggested.insert(name, p);
            }
        }
        cases.push(evaluate(&ctx, id, script).0);
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

/// Every selected case, or the first reason one cannot be exported. Reads
/// only local files, so a caller can refuse before any network call.
fn eligible(ctx: &Ctx, root: &Path, org: &str, project: &str, case_ids: &[i32]) -> Result<Vec<(i32, Ready)>, String> {
    let mut ready: Vec<(i32, Ready)> = Vec::new();
    for &id in case_ids {
        if ready.iter().any(|(i, _)| *i == id) {
            continue;
        }
        let script = load_expanded(root, org, project, id);
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

pub fn ensure_exportable(
    root: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
) -> Result<(), String> {
    let ctx = Ctx::load(root, org, project, clone_path, modules)?;
    ctx.clone.as_ref().map_err(|e| e.clone())?;
    eligible(&ctx, root, org, project, case_ids).map(|_| ())
}

struct Out {
    rel: String,
    content: String,
}

pub fn write_with(
    root: &Path,
    org: &str,
    project: &str,
    case_ids: &[i32],
    clone_path: &str,
    modules: &BTreeMap<i32, String>,
    docs: &BTreeMap<i32, CaseDoc>,
) -> Result<ExportResult, String> {
    let ctx = Ctx::load(root, org, project, clone_path, modules)?;
    let repo = ctx.clone.as_ref().map_err(|e| e.clone())?;

    // Refuse everything unless every case is exportable.
    let ready = eligible(&ctx, root, org, project, case_ids)?;
    if let Some((id, _)) = ready.iter().find(|(id, _)| !docs.contains_key(id)) {
        return Err(format!("case {id} could not be read from Azure DevOps. Nothing was written."));
    }

    let recipe = ctx.recipe.as_ref().ok_or("no sign-in recipe for this project")?;

    let mut md_files: BTreeMap<String, String> = BTreeMap::new();
    let mut raw_files: Vec<Out> = Vec::new();
    let mut cases: Vec<(i32, String)> = Vec::new();
    let mut missing_navigation: Vec<String> = Vec::new();
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    let mut chosen_by: BTreeMap<String, i32> = BTreeMap::new();
    let mut index = repo.index.clone();

    for (id, r) in &ready {
        let id = *id;
        let p = &r.placement;
        let seg = p.seg();
        let md_rel = format!("suites/{seg}/test-cases/{}.md", p.feature);
        let nav_key = format!("{}/{}", p.module, p.feature);
        let captured = repo.navigation_keys.contains(&nav_key);
        if !captured && !missing_navigation.contains(&nav_key) {
            missing_navigation.push(nav_key);
        }
        let feature_title = title_case(&p.feature);

        let mut doc = docs[&id].clone();
        doc.side = p.side.clone();
        doc.navigation_captured = captured;
        doc.feature_title = feature_title.clone();

        // Several cases of one feature accumulate into one file in memory.
        if !md_files.contains_key(&md_rel) {
            let path = repo.root.join(&md_rel);
            let start = if path.is_file() {
                std::fs::read_to_string(&path).map_err(|e| format!("could not read {md_rel}: {e}"))?
            } else {
                test_case::new_file(&feature_title, &p.feature, &r.user_key)
            };
            md_files.insert(md_rel.clone(), start);
        }
        let current = md_files.get_mut(&md_rel).expect("inserted above");
        *current = test_case::splice(current, id, &test_case::section(&doc));

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
    let last_id = ready.last().map(|(i, _)| *i);
    if let (Some(id), Some((_, file))) = (last_id, cases.last()) {
        outs.push(Out { rel: "suites/_generated/index.json".into(), content: index_with(&index, id, file) });
    }

    commit(&repo.root, &outs)?;
    let user_keys = ready.iter().map(|(id, r)| (*id, r.user_key.clone())).collect();
    Ok(ExportResult { files: outs.into_iter().map(|o| o.rel).collect(), cases, user_keys, missing_navigation })
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
