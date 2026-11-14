//! API templates, fixtures and flows: listing, saving, proving and running them.

use super::*;

/// The browsers a real prove or run opens: one, headless - nobody watches
/// a template run.
pub(crate) fn real_template_browsers(which: crate::browser::launch::Browser) -> crate::commands::autorun_replay::RealBrowsers {
    crate::commands::autorun_replay::RealBrowsers::new(which, false)
}

/// The origin this project signs in at in the active environment - the
/// environment's address, else the saved recipe's - or None when there is
/// neither.
pub(crate) fn recipe_origin(root: &std::path::Path, org: &str, project: &str) -> Option<String> {
    crate::autorun::recipe::load_effective_recipe(root, org, project)
        .ok()
        .and_then(|r| crate::autorun::recipe::origin_of(&r.start_url))
}

/// The API template guide, with this project's account keys and origin.
/// Answers without a data root too - with the format alone and a sentence
/// saying what is not set up yet.
pub(crate) fn api_template_guide(ctx: &BridgeContext) -> String {
    let Some(root) = crate::autorun::store::configured_root() else {
        return crate::api_templates::guide::text(&[], None);
    };
    // Keys only: the guide never sees a username or a password.
    let keys: Vec<String> = crate::autorun::accounts::load_accounts(&root)
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.key)
        .collect();
    let origin = recipe_origin(&root, &ctx.org, &ctx.project);
    let mut out = crate::api_templates::guide::text(&keys, origin.as_deref());
    match crate::environments::active(&root) {
        Ok(env) => out.push_str(&crate::api_templates::guide::active_environment_line(&env.name)),
        Err(e) => crate::applog::warn(format!("Guide: the active environment could not be read: {e}")),
    }
    out.push_str(&crate::test_files::guide_section(&project_test_files(&root, ctx)));
    // The project's quirks - the same list, and the same section, the Auto
    // Run guide ends with: active notes only, each with its evidence.
    if !ctx.org.trim().is_empty() && !ctx.project.trim().is_empty() {
        let quirks = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();
        let section = crate::autorun::quirks::quirks_section(&quirks);
        if !section.is_empty() {
            out.push('\n');
            out.push_str(&section);
        }
    }
    out
}

/// How many templates one answer of the list carries when no `limit` is
/// given, and the most it ever carries.
pub const TEMPLATE_PAGE: usize = 25;
pub const TEMPLATE_PAGE_MAX: usize = 100;
/// The most characters a list answer in full carries; over it the answer is
/// the index, which says how to narrow.
pub const TEMPLATE_ANSWER_CAP: usize = 40_000;
/// A run's `detail` is cut to this many characters in a list answer.
pub(crate) const RUN_DETAIL_CUT: usize = 300;

/// A run record as a list answer shows it: its `detail` cut to
/// `RUN_DETAIL_CUT` characters plus `...`.
pub(crate) fn last_run_cut(r: &crate::api_templates::store::RunRecord) -> serde_json::Value {
    let mut v = serde_json::json!(r);
    if let Some(d) = r.detail.as_deref().filter(|d| d.chars().count() > RUN_DETAIL_CUT) {
        let cut: String = d.chars().take(RUN_DETAIL_CUT).collect();
        v["detail"] = serde_json::json!(format!("{cut}..."));
    }
    v
}

/// The one word for a state on the active environment.
pub(crate) fn here_word(here: &crate::api_templates::here::Here) -> &'static str {
    use crate::api_templates::here::Here;
    match here {
        Here::Confirmed { .. } => "confirmed",
        Here::Unconfirmed => "unconfirmed",
        Here::Failed { .. } => "failed",
    }
}

/// The keys every template and fixture row carries for its state on the
/// active environment: `here`, `here_note` (null when confirmed with no
/// strike), and `failed_here` or `last_failure_here` with the step and
/// detail of the failure.
pub(crate) fn put_here(row: &mut serde_json::Value, here: &crate::api_templates::here::Here) {
    use crate::api_templates::here::Here;
    use crate::api_templates::share::{FAILED_HERE_FOR_ASSISTANT, LAST_RUN_FAILED_HERE, UNPROVEN_FOR_ASSISTANT};
    let note = match here {
        Here::Confirmed { last_failure: None } => None,
        Here::Confirmed { last_failure: Some(_) } => Some(LAST_RUN_FAILED_HERE),
        Here::Unconfirmed => Some(UNPROVEN_FOR_ASSISTANT),
        Here::Failed { .. } => Some(FAILED_HERE_FOR_ASSISTANT),
    };
    row["here"] = serde_json::json!(here_word(here));
    row["here_note"] = serde_json::json!(note);
    match here {
        Here::Failed { failure } => row["failed_here"] = serde_json::json!(failure),
        Here::Confirmed { last_failure: Some(failure) } => row["last_failure_here"] = serde_json::json!(failure),
        _ => {}
    }
}

/// The saved templates for this project, filtered and paged.
///
/// Arguments, all optional, from the query: `module` (a case-insensitive
/// substring), `search` (a case-insensitive substring of the id, the
/// title, or a param or output name), `flow` (that flow's templates, and
/// that flow alone), `offset` and `limit` (25 by default, 100 at most),
/// and `id` (that one template in full, with the flow it is on).
/// `with_flow=false` leaves the flows and the Test files out of an answer
/// in full: asked for one template by `id`, the flow's every stage and the
/// file list were most of the answer, and the same each time.
///
/// A filtered result of at most `limit` templates comes back in full:
/// what each one takes and gives back, the flow stage it performs,
/// whether it is confirmed, unconfirmed or failed on the active
/// environment (`here`, with `here_note` saying what to do about it; `proven`
/// is whether it was ever proven on any environment), its newest run (null before its first - the
/// prove that saved it is history, not a run), every flow with its stages
/// and the templates on each, and the project's Test files. A larger one
/// is a compact index: per template the id, title, module, effect, proven
/// state and stage, and per flow its id, title and stage count, with a
/// `note` saying how to narrow it. Every answer carries `paging`. Owner's
/// report: a project's whole list in full was 210K characters, too big to
/// read. A flow file that no longer parses is left out (and logged by
/// `flow_store::list`), never the whole answer.
pub(crate) fn api_template_list(ctx: &BridgeContext, target: &str) -> (u16, String) {
    use crate::api_templates::{flow_store, gate, store};
    let arg = |key: &str| q(target, key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let number = |key: &str, min: usize| -> Result<Option<usize>, (u16, String)> {
        match arg(key) {
            None => Ok(None),
            Some(v) => match v.parse::<usize>() {
                Ok(n) if n >= min => Ok(Some(n)),
                _ => Err((400, format!("\"{key}\" is a whole number, {min} or more"))),
            },
        }
    };
    // With `id`, the other filters and the paging are ignored, so an offset
    // or limit sent beside it is not refused either.
    let by_id = arg("id").is_some();
    let offset = match number("offset", 0) {
        _ if by_id => 0,
        Ok(n) => n.unwrap_or(0),
        Err(refused) => return refused,
    };
    let limit = match number("limit", 1) {
        _ if by_id => TEMPLATE_PAGE,
        Ok(n) => n.unwrap_or(TEMPLATE_PAGE).min(TEMPLATE_PAGE_MAX),
        Err(refused) => return refused,
    };
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let saved = match store::list(&root, &ctx.org, &ctx.project) {
        Ok(saved) => saved,
        Err(e) => {
            crate::applog::warn(format!("api templates: the list could not be read: {e}"));
            return (500, "the saved templates could not be read - see Settings, Logs".to_string());
        }
    };
    let mut flows = flow_store::list(&root, &ctx.org, &ctx.project).unwrap_or_else(|e| {
        crate::applog::warn(format!("api template flows: the list could not be read: {e}"));
        Vec::new()
    });

    // Which templates this answer is about.
    let lower = |s: &str| s.to_lowercase();
    let matched: Vec<&store::SavedTemplate> = if let Some(id) = arg("id") {
        let Some(one) = saved.iter().find(|s| s.template.id == id) else {
            return (
                404,
                format!("no template called \"{id}\" is saved for this project - list_api_templates shows the ones that are"),
            );
        };
        let on = one.template.stage.as_ref().map(|r| r.flow.clone());
        flows.retain(|f| Some(&f.id) == on.as_ref());
        vec![one]
    } else {
        let flow = arg("flow");
        if let Some(flow) = &flow {
            if !flows.iter().any(|f| &f.id == flow) {
                return (
                    404,
                    format!("no flow called \"{flow}\" is saved for this project - list_api_templates shows the ones that are"),
                );
            }
            flows.retain(|f| &f.id == flow);
        }
        let module = arg("module").map(|m| lower(&m));
        let search = arg("search").map(|s| lower(&s));
        saved
            .iter()
            .filter(|s| {
                let t = &s.template;
                let hit = |text: &str, part: &str| lower(text).contains(part);
                module.as_deref().is_none_or(|m| hit(&t.module, m))
                    && flow.as_ref().is_none_or(|f| t.stage.as_ref().is_some_and(|r| &r.flow == f))
                    && search.as_deref().is_none_or(|q| {
                        hit(&t.id, q)
                            || hit(&t.title, q)
                            || t.params.iter().any(|p| hit(&p.name, q))
                            || t.outputs.iter().any(|o| hit(o, q))
                    })
            })
            .collect()
    };
    let total = matched.len();
    let full = total <= limit;
    let page: Vec<&store::SavedTemplate> = matched.into_iter().skip(offset).take(limit).collect();
    // A narrowed answer in full names the flows its templates are on (a
    // requested `flow` is already the only one), not every flow with every
    // stage - that bulk is what the filters are for.
    let narrowed = !by_id && arg("flow").is_none() && (arg("module").is_some() || arg("search").is_some());
    if full && narrowed {
        flows.retain(|f| page.iter().any(|s| s.template.stage.as_ref().is_some_and(|r| r.flow == f.id)));
    }

    let build = |full: bool| -> serde_json::Value {
        let flow_rows: Vec<serde_json::Value> = flows
            .iter()
            .map(|f| {
                // Never "failed": a flow is confirmed or not (Ruling 8).
                let here = here_word(&crate::api_templates::here::flow_here(f, &saved));
                if !full {
                    return serde_json::json!({ "id": f.id, "title": f.title, "stage_count": f.stages.len(), "here": here });
                }
                let stages: Vec<serde_json::Value> = f
                    .stages
                    .iter()
                    .map(|s| {
                        let on: Vec<&str> =
                            gate::templates_on(&saved, &f.id, &s.id).iter().map(|t| t.template.id.as_str()).collect();
                        serde_json::json!({
                            "id": s.id,
                            "title": s.title,
                            "requires": s.requires,
                            "optional": s.optional,
                            "creates": s.creates,
                            "templates": on,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "id": f.id,
                    "title": f.title,
                    "module": f.module,
                    "subject": f.subject,
                    "here": here,
                    "stages": stages,
                })
            })
            .collect();
        let rows: Vec<serde_json::Value> = page
            .iter()
            .map(|s| {
                let t = &s.template;
                if !full {
                    let mut row = serde_json::json!({
                        "id": t.id,
                        "title": t.title,
                        "module": t.module,
                        "effect": t.effect,
                        "proven": t.proven.is_some(),
                        "stage": t.stage,
                    });
                    // The index carries the state, not the sentence: the
                    // note below says it once.
                    row["here"] = serde_json::json!(here_word(&s.here));
                    return row;
                }
                let mut row = serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "module": t.module,
                    "effect": t.effect,
                    "params": t.params,
                    "outputs": t.outputs,
                    "stage": t.stage,
                    // Proven on SOME environment (the fixture save gate); `here`
                    // is the state on the active one.
                    "proven": t.proven.is_some(),
                    "last_run": s.runs.iter().find(|r| r.mode == store::MODE_RUN).map(last_run_cut),
                });
                put_here(&mut row, &s.here);
                row
            })
            .collect();
        let mut paging = serde_json::json!({ "total": total, "offset": offset, "returned": rows.len() });
        if offset + rows.len() < total {
            paging["next_offset"] = serde_json::json!(offset + rows.len());
        }
        let mut answer = serde_json::json!({ "templates": rows, "flows": flow_rows, "paging": paging });
        let with_flow = !matches!(arg("with_flow").as_deref(), Some("false") | Some("0"));
        if full && !with_flow {
            if let Some(map) = answer.as_object_mut() {
                map.remove("flows");
            }
            answer["left_out"] = serde_json::json!("the flows and the Test files (flow: false)");
        } else if full {
            // Names and sizes only: what a template's `files` may name.
            let test_files: Vec<serde_json::Value> = project_test_files(&root, ctx)
                .into_iter()
                .map(|f| serde_json::json!({ "name": f.name, "size": f.size }))
                .collect();
            answer["test_files"] = serde_json::json!(test_files);
        } else {
            let mut note = format!(
                "This is the index of {total} templates. Narrow it with module, search or flow until at most {limit} match, or pass id, for params, outputs, the newest run and the test files. Page with offset and limit (at most {TEMPLATE_PAGE_MAX})."
            );
            // The full answer says it on each row; the index says it once.
            use crate::api_templates::here::Here;
            if page.iter().any(|s| matches!(s.here, Here::Unconfirmed)) {
                note.push_str(&format!(
                    " A template whose here is unconfirmed is {}.",
                    crate::api_templates::share::UNPROVEN_FOR_ASSISTANT
                ));
            }
            if page.iter().any(|s| matches!(s.here, Here::Failed { .. })) {
                note.push_str(&format!(
                    " A template whose here is failed is {}.",
                    crate::api_templates::share::FAILED_HERE_FOR_ASSISTANT
                ));
            }
            answer["note"] = serde_json::json!(note);
        }
        answer
    };
    let mut answer = build(full);
    if full && !by_id {
        let n = answer.to_string().len();
        if n > TEMPLATE_ANSWER_CAP {
            answer = build(false);
            answer["note"] = serde_json::json!(format!(
                "The answer in full would be {n} characters, more than one answer carries ({TEMPLATE_ANSWER_CAP}): this is the index of {total} templates. Pass id for one template in full, narrow with module, search or flow, or page with a smaller limit."
            ));
        }
    }
    (200, answer.to_string())
}

/// Every saved template for this project, for naming the ones on a stage.
/// One that cannot be listed leaves that part of a sentence short rather
/// than refusing the call, and is logged.
pub(crate) fn saved_templates(root: &std::path::Path, org: &str, project: &str) -> Vec<crate::api_templates::store::SavedTemplate> {
    crate::api_templates::store::list(root, org, project).unwrap_or_else(|e| {
        crate::applog::warn(format!("api templates: the list could not be read: {e}"));
        Vec::new()
    })
}

/// An argument that may arrive as JSON or as a JSON string - the shape
/// every sibling tool on this server takes its payload in. A string that
/// does not parse is handed on as the string, so the complaint about it
/// names what was actually sent.
pub(crate) fn json_arg(v: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => {
            Some(serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.clone())))
        }
        Some(other) => Some(other.clone()),
    }
}

/// What a prove and a run both carry besides the template: the account
/// key, the param values, and the browser.
pub(crate) struct TemplateCall {
    pub(crate) account: String,
    pub(crate) values: serde_json::Map<String, serde_json::Value>,
    pub(crate) browser: crate::browser::launch::Browser,
}

pub(crate) fn template_call(v: &serde_json::Value, shape: &str) -> Result<TemplateCall, (u16, String)> {
    let account = match v.get("account") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return Err((
                400,
                format!(
                    "this call needs an \"account\" - one of the account keys get_guide (topic api_templates) lists. Expected {shape}."
                ),
            ))
        }
    };
    let values = match json_arg(v.get("values")) {
        None => serde_json::Map::new(),
        Some(serde_json::Value::Object(m)) => m,
        Some(_) => return Err((400, "\"values\" is an object of param names to values".to_string())),
    };
    let browser = match v.get("browser") {
        None | Some(serde_json::Value::Null) => crate::browser::launch::Browser::Edge,
        Some(serde_json::Value::String(s)) if matches!(s.trim().to_ascii_lowercase().as_str(), "edge" | "chrome") => {
            crate::browser::launch::Browser::from_name(s)
        }
        Some(_) => return Err((400, "\"browser\" is \"edge\" or \"chrome\"".to_string())),
    };
    Ok(TemplateCall { account, values, browser })
}

pub(crate) const PROVE_SHAPE: &str = "{ \"template\": <the draft>, \"account\": \"<account key>\", \"values\": { <param>: <value> }, \"replace\"?: true, \"why\"?: \"<reason>\", \"browser\"?: \"edge\" | \"chrome\" }";
pub(crate) const RUN_SHAPE: &str = "{ \"id\": \"<template id>\", \"account\": \"<account key>\", \"values\": { <param>: <value> }, \"browser\"?: \"edge\" | \"chrome\" }";

/// `POST /api-template-prove`: run a draft and save it only if every step
/// passed - and, for a template on a flow, only if its own stage is done
/// afterwards. The route arm with the browser factory (`open`) and the
/// flow database (`open_db`, asked for only by a template on a flow)
/// handed in, so a test reaches all of it but a real browser and server.
/// The default caller's: a refusal for an account the Auto Run browser holds
/// asks main's browser (`api_template_prove_for`).
pub async fn api_template_prove<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    api_template_prove_for(Some(SlotNo::MAIN), ctx, body, open, open_db, timing).await
}

/// `api_template_prove` for a caller whose own Auto Run browser is slot
/// `asker` (none: it has no browser). A proof refused its account because the
/// Auto Run browser holds it says so only when THAT browser holds it.
pub async fn api_template_prove_for<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    asker: Option<SlotNo>,
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    let (kept, explore) = match asker {
        Some(n) => {
            let slot = crate::commands::autorun::session_slot(n).lock().await;
            let discovery = slot.as_ref().and_then(|s| s.discovery.as_ref());
            (
                discovery.map(|d| d.kept.clone()).unwrap_or_default(),
                discovery.and_then(|d| d.explore_session()).map(str::to_string),
            )
        }
        None => Default::default(),
    };
    api_template_prove_with(kept, explore, asker, ctx, body, open, open_db, timing).await
}

/// Fills `{{var.name}}` in the strings of `values` from `kept` and adds each
/// used kept value as a `var.<name>` entry, which the run hides from its
/// records and answers. A name that is not kept is refused naming it.
pub fn fill_kept_values(
    values: serde_json::Map<String, serde_json::Value>,
    kept: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<serde_json::Map<String, serde_json::Value>, (u16, String)> {
    use crate::api_templates::exec::{placeholders, substitute, VAR_PREFIX};
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.extend(placeholders(s)),
            serde_json::Value::Array(a) => a.iter().for_each(|i| walk(i, out)),
            serde_json::Value::Object(m) => m.values().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut names = Vec::new();
    values.values().for_each(|v| walk(v, &mut names));
    let mut vars: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for full in names.into_iter().filter(|n| n.starts_with(VAR_PREFIX)) {
        match kept.get(&full[VAR_PREFIX.len()..]) {
            Some(v) => {
                vars.insert(full, v.clone());
            }
            None => return Err((400, crate::api_templates::one_off::var_not_kept(&full))),
        }
    }
    let mut out = serde_json::Map::new();
    for (k, v) in values {
        out.insert(k, substitute(&v, &vars));
    }
    for (full, v) in vars {
        out.insert(full, v);
    }
    Ok(out)
}

/// `api_template_prove_for` with the asker's kept values handed in.
pub async fn api_template_prove_with<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    kept: std::collections::BTreeMap<String, serde_json::Value>,
    explore: Option<String>,
    asker: Option<SlotNo>,
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::runner::{Mode, RunRequest};
    use crate::api_templates::{store, valid_id, ApiTemplate};
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {PROVE_SHAPE}.")),
    };
    let Some(draft) = json_arg(v.get("template")) else {
        return (400, format!("this call needs a \"template\". Expected {PROVE_SHAPE}."));
    };
    let template: ApiTemplate = match serde_json::from_value(draft) {
        Ok(t) => t,
        Err(e) => return (400, format!("that is not a template: {e} - call get_guide (topic api_templates) for the format")),
    };
    let call = match template_call(&v, PROVE_SHAPE) {
        Ok(c) => c,
        Err(refused) => return refused,
    };
    let call_values = match fill_kept_values(call.values, &kept) {
        Ok(values) => values,
        Err(refused) => return refused,
    };
    let replace = v["replace"].as_bool().unwrap_or(false);
    let why = v["why"].as_str().map(str::to_string);
    let existing = match store::load(&root, &ctx.org, &ctx.project, &template.id) {
        Ok(found) => found,
        // A saved file that no longer reads still stands for a template of
        // that id, so replacing it needs the same reason a readable one
        // does. An invalid id is one of `check`'s own problems.
        Err(e) if valid_id(&template.id) => {
            crate::applog::warn(format!("api template {}: the saved copy could not be read: {e}", template.id));
            Some(template.clone())
        }
        Err(_) => None,
    };
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account: call.account,
        values: call_values,
        mode: Mode::Prove { replace, why },
        template,
        step_limit: None,
        keep: Default::default(),
    };
    // Whatever a sentence built outside the run quotes is hidden too.
    let hide = crate::api_templates::exec::hidden_values(&req.values);
    let (status, out) =
        run_api_template_request(ctx, &root, req, existing.as_ref(), call.browser, asker, explore, open, open_db, timing).await;
    (status, crate::api_templates::exec::hide_text(&out, &hide))
}

/// `POST /api-template-run`: run a saved template and return its outputs,
/// or the failing step and what had been created. As
/// `api_template_prove`, with the browser factory and the flow database
/// handed in. The default caller's: a refusal for an account the Auto Run
/// browser holds asks main's browser (`api_template_run_for`).
pub async fn api_template_run<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    api_template_run_for(Some(SlotNo::MAIN), ctx, body, open, open_db, timing).await
}

/// `api_template_run` for a caller whose own Auto Run browser is slot
/// `asker` (none: it has no browser), as `api_template_prove_for`.
pub async fn api_template_run_for<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    asker: Option<SlotNo>,
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let (req, which) = match run_request_from(ctx, &root, body) {
        Ok(read) => read,
        Err(refused) => return refused,
    };
    run_api_template_request(ctx, &root, req, None, which, asker, None, open, open_db, timing).await
}

/// What `api_template_run` reads of its call: the saved template, the
/// account, the values and the browser asked for, as a run request. Every
/// refusal is a 400 naming what was wrong.
pub(crate) fn run_request_from(
    ctx: &BridgeContext,
    root: &std::path::Path,
    body: &str,
) -> Result<(crate::api_templates::runner::RunRequest, crate::browser::launch::Browser), (u16, String)> {
    use crate::api_templates::runner::{Mode, RunRequest};
    use crate::api_templates::store;
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return Err((400, format!("that is not readable JSON: {e}. Expected {RUN_SHAPE}."))),
    };
    let id = match v.get("id") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err((400, format!("this call needs an \"id\". Expected {RUN_SHAPE}."))),
    };
    let call = template_call(&v, RUN_SHAPE)?;
    let template = match store::load(root, &ctx.org, &ctx.project, &id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err((
                400,
                format!("no template called \"{id}\" is saved for this project - list_api_templates shows the ones that are"),
            ))
        }
        Err(e) => return Err((400, e)),
    };
    let req = RunRequest {
        org: ctx.org.clone(),
        project: ctx.project.clone(),
        account: call.account,
        values: call.values,
        mode: Mode::Run,
        template,
        step_limit: None,
        keep: Default::default(),
    };
    Ok((req, call.browser))
}

/// Said beside a failed prove that sent writes: the steps are named in
/// `writes_that_ran`.
pub const PROVE_WROTE: &str = "this prove failed after these write steps went out: what they created or changed is still there";

/// The POST steps of `template` that went out in `report`, each as its name,
/// its path with the values and what the run captured filled in (and its
/// `?handler=`), and its status. A step went out when it has a status, or a
/// postback or a plain step timed out after it was sent. A kept value
/// (`var.*`) is never filled in: its placeholder stays, so no request line
/// holds one.
pub(crate) fn sent_post_steps<'a>(
    template: &'a crate::api_templates::ApiTemplate,
    report: &crate::api_templates::runner::RunReport,
    vars: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Vec<(&'a str, String, Option<u16>)> {
    use crate::api_templates::exec::{substitute_str, VAR_PREFIX};
    use crate::api_templates::Method;
    let mut with_captures: std::collections::BTreeMap<String, serde_json::Value> =
        vars.iter().filter(|(k, _)| !k.starts_with(VAR_PREFIX)).map(|(k, v)| (k.clone(), v.clone())).collect();
    with_captures.extend(report.created.clone());
    template
        .steps
        .iter()
        .filter(|s| s.method() == Method::Post)
        .filter_map(|s| {
            // A postback that timed out after the post went out has no status but may have saved.
            let ran = report.steps.iter().find(|r| {
                r.name == s.name
                    && (r.status.is_some()
                        || r.detail.contains(crate::api_templates::postback::MAY_HAVE_SAVED)
                        || r.detail.contains(crate::api_templates::runner::SENT_NO_ANSWER)
                        || r.detail.contains(crate::api_templates::runner::MAYBE_SENT_NO_ANSWER))
            })?;
            let path = substitute_str(&s.address(), &with_captures);
            // The `handler` says which save the page's address is.
            let path = match s.query.iter().find(|(k, _)| k.eq_ignore_ascii_case("handler")) {
                Some((_, handler)) => format!("{path}?handler={}", substitute_str(handler.trim(), &with_captures)),
                None => path,
            };
            Some((s.name.as_str(), path, ran.status))
        })
        .collect()
}

/// Each POST step of `template` that went out in `report`, as
/// `("POST", path)`: what `explore::note_writes` takes.
pub fn sent_posts(
    template: &crate::api_templates::ApiTemplate,
    report: &crate::api_templates::runner::RunReport,
    vars: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Vec<(String, String)> {
    sent_post_steps(template, report, vars).into_iter().map(|(_, path, _)| ("POST".to_string(), path)).collect()
}

/// A failed prove's report as the answer: when write steps went out it also
/// names them (`writes_that_ran`, with `PROVE_WROTE`), and - for an asker with
/// an explore session - notes each as a save and keeps it on the change list
/// as a `sent` change (`on_change_list`). With no write sent, the report as it
/// is. No address, host or kept value goes into it.
///
/// `sentence` is the plain answer of a prove whose steps all passed but which
/// saved nothing (a stage check refused it, or the save failed): with no write
/// sent it is the answer as it is; with writes it becomes `message` beside them.
pub fn failed_prove_answer(
    root: &std::path::Path,
    explore: Option<&str>,
    template: &crate::api_templates::ApiTemplate,
    values: &serde_json::Map<String, serde_json::Value>,
    report: &crate::api_templates::runner::RunReport,
    sentence: Option<&str>,
) -> String {
    use crate::autorun::explore as ex;
    let mut v = serde_json::to_value(report).unwrap_or_else(|_| serde_json::json!({ "ok": report.ok }));
    let vars: std::collections::BTreeMap<String, serde_json::Value> =
        crate::api_templates::with_defaults(template, values).into_iter().collect();
    let hide = crate::api_templates::exec::hidden_values(values);
    let sent: Vec<(&str, String, Option<u16>)> = sent_post_steps(template, report, &vars)
        .into_iter()
        .map(|(name, path, status)| (name, crate::api_templates::exec::hide_text(&path, &hide), status))
        .collect();
    if sent.is_empty() {
        return sentence.map_or_else(|| v.to_string(), str::to_string);
    }
    if let Some(sentence) = sentence {
        v["message"] = serde_json::json!(sentence);
    }
    v["writes_that_ran"] = serde_json::json!(sent
        .iter()
        .map(|(name, path, status)| serde_json::json!({
            "step": name,
            "request": ex::request_line("POST", path),
            "status": status,
        }))
        .collect::<Vec<_>>());
    v["note"] = serde_json::json!(PROVE_WROTE);
    if let Some(session) = explore {
        let posts: Vec<(String, String)> = sent.iter().map(|(_, path, _)| ("POST".to_string(), path.clone())).collect();
        let mut kept = ex::note_writes(root, session, &posts).map_err(|why| {
            crate::applog::warn(format!("Auto Run explore session: a save could not be noted: {why}"));
        });
        for (_, path, _) in &sent {
            let line = ex::request_line("POST", path);
            if let Err(why) = ex::record_prove_write(root, session, &template.id, &line) {
                crate::applog::warn(format!("Auto Run explore session: a failed prove's write could not be kept: {why}"));
                kept = Err(());
            }
        }
        v["on_change_list"] = serde_json::json!(kept.is_ok());
    }
    v.to_string()
}

/// Said beside the report of a saved template that ran in the assistant's
/// own explore session instead of a browser of its own.
pub const RAN_IN_YOUR_SESSION: &str = "Ran in your explore session's browser, as its account.";
/// Said beside such a run when the template saves: what it created or
/// changed is not on the change list until the assistant records it.
pub const RUN_REMINDER: &str = "Record what this run created or changed with record_explore_change.";

/// `/api-template-run` for a saved template, tried before the usual run: a
/// template runs in the explore session's own browser, as the account that
/// session holds, instead of signing in to a browser of its own. `None` -
/// the usual run goes ahead - unless the slot holds an explore session
/// whose lease holds the call's account and the template is on no flow.
/// Then, in order: a template with a POST step in a session that must not
/// save is refused before anything is sent; the one-at-a-time slot, the
/// environment and the run's own checks; the run from the page the
/// template's token page opens; the run on the template's history and the
/// tab told; each POST step that was sent noted on the session's list of
/// saves. The answer is the run's report with `"ran_in"` beside it (and
/// `"record_change"` when it posts).
#[allow(clippy::too_many_arguments)]
pub async fn run_saved_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    ctx: &BridgeContext,
    root: &std::path::Path,
    req: &crate::api_templates::runner::RunRequest,
    timing: &crate::browser::timing::Timing,
    limit: std::time::Duration,
    retry_pauses: &[std::time::Duration],
) -> Option<(u16, String)> {
    use crate::api_templates::runner::{claim, preflight, run_in_page, TokenPage};
    use crate::api_templates::store::{self, RunRecord};
    use crate::api_templates::Method;
    use crate::autorun::explore as ex;
    if !ctx.api_writes || req.template.stage.is_some() {
        return None;
    }
    let browser = slot.as_mut()?;
    let parts = browser.parts();
    let state = parts.discovery.as_ref()?;
    let session = state.explore_session()?.to_string();
    if !parts.lease.holds(root, &req.account) {
        return None;
    }
    let id = req.template.id.as_str();
    let (org, project) = (req.org.as_str(), req.project.as_str());
    let vars: std::collections::BTreeMap<String, serde_json::Value> =
        crate::api_templates::with_defaults(&req.template, &req.values).into_iter().collect();
    let posts = req.template.steps.iter().any(|s| s.method() == Method::Post);
    if posts && discovery_must_not_save(root, state) {
        let first = req.template.steps.iter().find(|s| s.method() == Method::Post)?;
        let path = crate::api_templates::exec::substitute_str(&first.address(), &vars);
        return Some((409, crate::browser::save_guard::blocked("POST", &path)));
    }
    let Some(_claim) = claim() else {
        return Some((409, API_TEMPLATE_BUSY.to_string()));
    };
    if let Err(moved) = assistant_may_still_write(ctx, root) {
        return Some((409, moved));
    }
    if let Err(problems) = preflight(root, req, None) {
        return Some((400, problems.join("\n")));
    }
    let how = TokenPage::Open(req.template.antiforgery.page.clone());
    let out = run_in_page(parts.driver, root, req, timing, how, limit, retry_pauses).await;
    let report = &out.report;

    let record = RunRecord {
        at: crate::applog::stamp(),
        mode: req.mode.label().to_string(),
        account: req.account.clone(),
        ok: report.ok,
        failed_step: report.failed.clone(),
        detail: (!report.ok).then(|| report.message()),
        outputs: if report.ok { report.outputs.clone() } else { report.created.clone() },
    };
    match store::append_run(root, org, project, id, record) {
        Ok(()) => templates_changed(id),
        Err(e) => crate::applog::warn(format!("api template {id}: the run could not be added to its history: {e}")),
    }

    if matches!(req.mode, crate::api_templates::runner::Mode::Run) {
        note_assistant_template_run(root, org, project, &req.template, report);
    }

    // Each POST step that was sent, by its path with the values and what the
    // run captured filled in.
    let sent = sent_posts(&req.template, report, &vars);
    if let Err(why) = ex::note_writes(root, &session, &sent) {
        crate::applog::warn(format!("Auto Run explore session: a save could not be noted: {why}"));
    }

    let mut v = serde_json::to_value(report).unwrap_or_else(|_| serde_json::json!({ "ok": report.ok }));
    v["ran_in"] = serde_json::json!(RAN_IN_YOUR_SESSION);
    if posts {
        v["record_change"] = serde_json::json!(RUN_REMINDER);
    }
    Some((if report.ok { 200 } else { 502 }, v.to_string()))
}

/// What the assistant's run of a template says about it on this environment:
/// confirmed when it worked, otherwise the failure (the retry rule decides
/// whether a confirmed one flips). Lands only when the template that ran is
/// the saved one.
pub(crate) fn note_assistant_template_run(
    root: &std::path::Path,
    org: &str,
    project: &str,
    ran: &crate::api_templates::ApiTemplate,
    report: &crate::api_templates::runner::RunReport,
) {
    use crate::api_templates::here::{note_template_run, HereFailure, TrySource};
    // A run that never reached the site (no account lease, no browser) says
    // nothing about the template here.
    if report.unreached {
        return;
    }
    let failure = HereFailure { at: crate::applog::stamp(), step: report.failed.clone(), detail: Some(report.message()) };
    note_template_run(root, org, project, ran, report.ok, Some(failure), TrySource::Assistant);
}

/// `POST /api-template-run` with the caller's own browser (`n`, when it has
/// one): the explore session's browser when it can run the template
/// (`run_saved_in`), and `None` when the usual run should go ahead - at once
/// for a caller with no browser.
pub(crate) async fn run_saved_in_session(ctx: &BridgeContext, n: Option<SlotNo>, body: &str) -> Option<(u16, String)> {
    let n = n?;
    if !ctx.api_writes || unattended_run_is_using_the_browser().is_some() {
        return None;
    }
    let root = autorun_root().ok()?;
    let (req, _) = run_request_from(ctx, &root, body).ok()?;
    let mut slot = crate::commands::autorun::session_slot(n).lock().await;
    let answer = run_saved_in(
        &mut slot,
        ctx,
        &root,
        &req,
        &crate::browser::timing::Timing::supervised(),
        crate::api_templates::runner::RUN_LIMIT,
        &crate::api_templates::runner::RETRY_PAUSES,
    )
    .await?;
    Some(let_go_if_silent(&mut slot, answer).await)
}

/// An assistant's free text as one app-log line: every run of whitespace
/// (newlines included) one space, and at most 200 characters.
pub(crate) fn one_short_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// What a prove and a run share once the call is read: the one-at-a-time
/// slot, taken first so no environment switch can come between the checks
/// and the run; every check together; for a template on a flow, the gate -
/// the stages before its own, asked of the database - before the browser
/// opens; the run, then the bookkeeping - a proven template saved with its
/// evidence (on a flow, only once its own stage checks done), the run
/// appended to the template's history, and the tab told. 200 with the
/// report when it passed, 502 with the report when it did not.
///
/// `open_db` is called at most once, and never for a template on no flow:
/// those run exactly as they did before flows existed, database or not.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_api_template_request<B: crate::api_templates::held::Keeps, D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    root: &std::path::Path,
    req: crate::api_templates::runner::RunRequest,
    existing: Option<&crate::api_templates::ApiTemplate>,
    which: crate::browser::launch::Browser,
    asker: Option<SlotNo>,
    explore: Option<String>,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::flow::{check_stage_ref, creating_stage};
    use crate::api_templates::gate::{self, CheckFor, StageState};
    use crate::api_templates::runner::{claim, preflight, run_template, stage_flow, Mode};
    use crate::api_templates::store::{self, RunRecord};
    use crate::api_templates::{ApiTemplate, Proven};
    // The slot first, before the template, its flow or the database is
    // looked at: holding it is what refuses an environment switch, so none
    // can land between those checks and the run. Every return below drops
    // it, as the run's end does.
    let Some(_claim) = claim() else {
        return (409, API_TEMPLATE_BUSY.to_string());
    };
    // Judged on the context it arrived with; looked at again now that no
    // switch can land (`still_allowed`).
    if let Err(moved) = assistant_may_still_write(ctx, root) {
        return (409, moved);
    }
    if let Err(problems) = preflight(root, &req, existing) {
        return (400, problems.join("\n"));
    }
    // A delete template is only proven on a draft the tests made: its `id`
    // must be a `present` entry of its kind in the record, in the active
    // environment. The proof that deletes it marks it deleted.
    let deletes = match (&req.mode, req.template.effect) {
        (Mode::Prove { .. }, crate::api_templates::Effect::Delete) => {
            match crate::autorun::cleanup::proof_subject(root, &req.template, &req.values) {
                Ok(entry) => Some(entry),
                Err(sentence) => return (400, sentence),
            }
        }
        _ => None,
    };
    let id = req.template.id.as_str();
    let (org, project) = (req.org.as_str(), req.project.as_str());

    // The flow preflight has just found. Gone in between - replaced by
    // another call - is refused as preflight would have refused it.
    let on_flow = match &req.template.stage {
        None => None,
        Some(r) => match stage_flow(root, org, project, &req.template) {
            Some(f) => Some((f, r.id.clone())),
            None => return (400, check_stage_ref(&req.template, None).join("\n")),
        },
    };
    // The creating stage has no record to gate on; its prove still needs
    // the database afterwards, so that is asked for before the browser too,
    // rather than after the steps have already written.
    let mut db = None;
    if let Some((f, stage_id)) = &on_flow {
        let creates = creating_stage(f).is_some_and(|s| &s.id == stage_id);
        if !creates || matches!(req.mode, Mode::Prove { .. }) {
            let d = match open_db(ctx) {
                Ok(d) => d,
                Err(refused) => return refused,
            };
            if !creates {
                let value = req.values.get(&f.subject.name).cloned().unwrap_or(serde_json::Value::Null);
                let templates = saved_templates(root, org, project);
                if let Err(sentence) = gate::gate(&d, f, stage_id, &value, &templates, id).await {
                    return (400, sentence);
                }
            }
            db = Some(d);
        }
    }

    let mut browsers = open(which);
    let report = run_template(&mut browsers, root, &req, timing).await;
    // A browser the run kept signed in (`api_templates::held`) was taken
    // out of `browsers` first, so this ends only one it did not keep.
    drop(browsers);
    // Every step passed, so the entry is gone from the application, whether
    // or not the template is saved below.
    if let (true, Some(entry)) = (report.ok, &deletes) {
        crate::autorun::cleanup::proof_deleted(root, entry);
    }

    // A proven template on a flow must have done its own stage: the
    // subject from the values, or - creating it - from what was captured.
    // The flow is read again first: one replaced or removed while the steps
    // ran decides what is saved, and a stage it no longer has is not.
    let mut incomplete = None;
    if let (true, Mode::Prove { .. }, Some((_, stage_id)), Some(d)) = (report.ok, &req.mode, &on_flow, &db) {
        let reloaded = stage_flow(root, org, project, &req.template);
        match reloaded.as_ref().and_then(|f| f.stages.iter().find(|s| &s.id == stage_id).map(|s| (f, s))) {
            None => {
                let flow_id = req.template.stage.as_ref().map(|r| r.flow.as_str()).unwrap_or_default();
                let gone = match &reloaded {
                    None => format!("this template's flow {flow_id} is no longer saved"),
                    Some(_) => format!("stage \"{stage_id}\" is no longer in flow {flow_id}"),
                };
                crate::applog::info(format!("api template {id}: every step passed but {gone}, so it was not saved"));
                incomplete =
                    Some(format!("every step passed, but {gone}, so the template was not saved; {}", report.message()));
            }
            Some((f, stage)) => {
                let name = &f.subject.name;
                let value = if stage.creates { report.outputs.get(name) } else { req.values.get(name) }
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                // A creating stage's subject is what a step captured: one of
                // the wrong type is said as that, never as a check that could
                // not run - a retry would only create another record with the
                // same capture.
                let wrong_type = if stage.creates { gate::validate_value(f, &value).err() } else { None };
                if let Some(e) = wrong_type {
                    let kind = f.subject.kind.word();
                    crate::applog::info(format!(
                        "api template {id}: every step passed but the captured {name} is not a {kind}, so it was not saved"
                    ));
                    incomplete = Some(format!(
                        "every step passed, but the captured {name} is not a {kind} ({e}), so the template was not saved; {}",
                        report.message()
                    ));
                } else {
                    let why = CheckFor { purpose: "prove", template: Some(id) };
                    // A check that could not run - a database error, a
                    // timeout, no row count - is never "not done": either way
                    // nothing is saved, but each says what it is.
                    match gate::stage_state(d, f, stage, &value, &why).await {
                        StageState::Done => {}
                        StageState::NotDone => {
                            crate::applog::info(format!(
                                "api template {id}: every step passed but stage {} is not done, so it was not saved",
                                stage.id
                            ));
                            incomplete = Some(format!(
                                "every step passed, but {} is still not done for {name} {}, so the template was not saved; {}",
                                stage.title,
                                gate::shown(&value),
                                report.message()
                            ));
                        }
                        StageState::CouldNotRun => {
                            crate::applog::info(format!(
                                "api template {id}: every step passed but the check for stage {} could not be run, so it was not saved",
                                stage.id
                            ));
                            incomplete = Some(format!(
                                "every step passed, but the check for {} could not be run - see the activity folder in Settings, Logs, so the template was not saved; {}",
                                stage.title,
                                report.message()
                            ));
                        }
                    }
                }
            }
        }
    }

    let mut changed = false;
    let mut not_saved = None;
    if let (true, Mode::Prove { replace, why }, None) = (report.ok, &req.mode, &incomplete) {
        let proven = Proven {
            at: crate::applog::stamp(),
            origin: recipe_origin(root, org, project).unwrap_or_default(),
            account: req.account.clone(),
            outputs: report.outputs.clone(),
            // The run held the template slot, so no switch happened since
            // it signed in: the active environment is the one it ran in.
            environment: crate::environments::active(root).ok().map(|e| e.name),
            via: None,
        };
        let t = ApiTemplate { proven: Some(proven), ..req.template.clone() };
        match store::save_proven(root, org, project, &t) {
            Ok(()) => {
                changed = true;
                if existing.is_some() && *replace {
                    crate::applog::info(format!("api template {id} replaced: {}", one_short_line(why.as_deref().unwrap_or(""))));
                }
            }
            Err(e) => {
                crate::applog::warn(format!("api template {id}: the proven template could not be saved: {e}"));
                not_saved = Some(format!(
                    "{} - but the template could not be saved; see Settings, Logs",
                    report.message()
                ));
            }
        }
    }
    // Every run gets a line in its template's history, and so does the
    // prove that saved it. A failed prove saved nothing - over an existing
    // id the saved template is still the one proven before - so it gets
    // no line: the assistant already has the failure, and a red "last
    // run" on a template that never ran would be a lie.
    // A run of the saved version confirms it here or records its failure; a
    // failed prove only lands when the draft is the saved copy. A prove that
    // saved is already confirmed by `save_proven`.
    if matches!(req.mode, Mode::Run) || (matches!(req.mode, Mode::Prove { .. }) && !report.ok) {
        note_assistant_template_run(root, org, project, &req.template, &report);
    }
    let saved_by_this_prove = changed;
    if matches!(req.mode, Mode::Run) || saved_by_this_prove {
        let record = RunRecord {
            at: crate::applog::stamp(),
            mode: req.mode.label().to_string(),
            account: req.account.clone(),
            ok: report.ok,
            failed_step: report.failed.clone(),
            detail: (!report.ok).then(|| report.message()),
            // What the run actually left behind: its outputs, or - when it
            // stopped - everything it had captured by then.
            outputs: if report.ok { report.outputs.clone() } else { report.created.clone() },
        };
        match store::append_run(root, org, project, id, record) {
            Ok(()) => changed = true,
            Err(e) => crate::applog::warn(format!("api template {id}: the run could not be added to its history: {e}")),
        }
    }
    if changed {
        templates_changed(id);
    }
    // A prove whose steps ran wrote to the site even when nothing was saved:
    // its answer names those writes, and an explore session lists them.
    let is_prove = matches!(req.mode, Mode::Prove { .. });
    if let Some(sentence) = incomplete {
        let said = if is_prove {
            failed_prove_answer(root, explore.as_deref(), &req.template, &req.values, &report, Some(&sentence))
        } else {
            sentence
        };
        return (502, said);
    }
    if let Some(sentence) = not_saved {
        let said = if is_prove {
            failed_prove_answer(root, explore.as_deref(), &req.template, &req.values, &report, Some(&sentence))
        } else {
            sentence
        };
        return (500, said);
    }
    let mut text = serde_json::to_string(&report).unwrap_or_default();
    // A prove that failed after its write steps went out names them, and an
    // asker in an explore session gets them on its change list.
    if !report.ok && is_prove {
        text = failed_prove_answer(root, explore.as_deref(), &req.template, &req.values, &report, None);
    }
    if !report.ok && held_by_the_auto_run_browser(&text, &req.account) {
        // A template run is not the caller's own browser's, so the hint asks
        // the caller's: a caller with no browser has none to free.
        if let Some(n) = asker {
            let holds = crate::commands::autorun::assistant_browser_holds(n, &req.account).await;
            text = template_report_hint(text, &req.account, holds);
        }
    }
    (if report.ok { 200 } else { 502 }, text)
}

/// Said beside a template run refused its account because the Auto Run
/// browser holds it, when the assistant's replay opened that browser.
pub const FREE_IT_WITH_RELEASE: &str =
    "The Auto Run browser your replay left open holds this account: close it with reset_autorun_browser and `release: true`, then run the template again.";

/// Does a template run's report say its account `key` was refused because
/// the Auto Run browser holds it (the lease's own sentence,
/// `lease::in_use_by_browser`)?
pub(crate) fn held_by_the_auto_run_browser(report_json: &str, key: &str) -> bool {
    report_json.contains(&crate::autorun::lease::in_use_by_browser(key))
}

/// A failed template run's report, with `FREE_IT_WITH_RELEASE` beside it as
/// `"free_it"` when its account `key` was refused because the Auto Run
/// browser holds it and that browser is the assistant's own, holding this
/// very account (`assistant_holds`). Otherwise the report as it was.
pub fn template_report_hint(report_json: String, key: &str, assistant_holds: bool) -> String {
    if !assistant_holds || !held_by_the_auto_run_browser(&report_json, key) {
        return report_json;
    }
    match serde_json::from_str::<serde_json::Value>(&report_json) {
        Ok(mut v @ serde_json::Value::Object(_)) => {
            v["free_it"] = serde_json::json!(FREE_IT_WITH_RELEASE);
            v.to_string()
        }
        _ => report_json,
    }
}

pub(crate) const FIXTURE_SAVE_SHAPE: &str = "{ \"fixture\": { \"id\": \"<fixture id>\", \"name\": \"<name>\", \"account\": \"<account key>\", \"steps\": [ { \"template\": \"<template id>\", \"params\": { <param>: \"<text>\" } } ], \"outputs\"?: { <name>: \"{{steps.<n>.<output>}}\" }, \"creates\"?: [ { \"kind\", \"id\", \"name\" } ] } }";
pub(crate) const FIXTURE_RUN_SHAPE: &str = "{ \"id\": \"<fixture id>\", \"browser\"?: \"edge\" | \"chrome\" }";

/// The browser a call asks for: Edge unless it says `"chrome"`.
pub(crate) fn browser_arg(v: &serde_json::Value) -> Result<crate::browser::launch::Browser, (u16, String)> {
    match v.get("browser") {
        None | Some(serde_json::Value::Null) => Ok(crate::browser::launch::Browser::Edge),
        Some(serde_json::Value::String(s)) if matches!(s.trim().to_ascii_lowercase().as_str(), "edge" | "chrome") => {
            Ok(crate::browser::launch::Browser::from_name(s))
        }
        Some(_) => Err((400, "\"browser\" is \"edge\" or \"chrome\"".to_string())),
    }
}

/// Said beside a summary list of fixtures: how to read one in full.
pub const FIXTURES_IN_SHORT: &str =
    "each fixture in short - call list_api_templates with kind fixtures and id for one in full (its steps, current outputs and last run), or full: true for all";

/// `GET /api-template-fixtures`: the project's saved fixtures. By default a
/// summary of each - its id, name, output names, what it creates (kind and
/// name) and its last run's status - since the whole of every fixture
/// outgrows what an assistant can read. `?id=` answers that one fixture in
/// full (`fixture`), and `?full=true` every one in full, each with its
/// current outputs (from its newest successful run) and its last run. A
/// read: it answers with the API templates switch off.
pub(crate) fn api_fixture_list(ctx: &BridgeContext, target: &str) -> (u16, String) {
    use crate::api_templates::fixture_store;
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let saved = match fixture_store::list(&root, &ctx.org, &ctx.project) {
        Ok(s) => s,
        Err(e) => {
            crate::applog::warn(format!("api fixtures: the list could not be read: {e}"));
            return (500, "the fixtures could not be listed - see Settings, Logs".to_string());
        }
    };
    let in_full = |s: &crate::api_templates::fixture_store::SavedFixture| {
        let f = &s.fixture;
        let mut row = serde_json::json!({
            "id": f.id,
            "name": f.name,
            "account": f.account,
            "steps": f.steps,
            "outputs": f.outputs,
            "creates": f.creates,
            "current_outputs": s.runs.iter().find(|r| r.ok).map(|r| &r.outputs),
            "last_run": s.runs.first(),
        });
        put_here(&mut row, &s.here);
        row
    };
    if let Some(id) = q(target, "id").map(|i| i.trim().to_string()).filter(|i| !i.is_empty()) {
        return match saved.iter().find(|s| s.fixture.id == id) {
            Some(s) => (200, serde_json::json!({ "fixture": in_full(s) }).to_string()),
            None => {
                let ids: Vec<&str> = saved.iter().map(|s| s.fixture.id.as_str()).collect();
                let known = if ids.is_empty() { "none is saved".to_string() } else { format!("the saved ones are {}", ids.join(", ")) };
                (404, format!("no fixture \"{id}\" is saved in this project - {known}"))
            }
        };
    }
    if q(target, "full").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
        let fixtures: Vec<serde_json::Value> = saved.iter().map(in_full).collect();
        let answer = serde_json::json!({ "fixtures": fixtures }).to_string();
        if answer.len() <= TEMPLATE_ANSWER_CAP {
            return (200, answer);
        }
        let n = answer.len();
        return (200, fixture_index(&saved, Some(format!("The fixtures in full would be {n} characters: pass id for one in full."))));
    }
    (200, fixture_index(&saved, None))
}

pub(crate) fn fixture_index(saved: &[crate::api_templates::fixture_store::SavedFixture], over: Option<String>) -> String {
    let fixtures: Vec<serde_json::Value> = saved
        .iter()
        .map(|s| {
            let f = &s.fixture;
            let last = s.runs.first().map(|r| {
                let mut v = serde_json::json!({ "at": r.at, "ok": r.ok });
                if let Some(step) = r.failed_step {
                    v["failed_step"] = serde_json::json!(step);
                }
                v
            });
            let mut row = serde_json::json!({
                "id": f.id,
                "name": f.name,
                "outputs": f.outputs.keys().collect::<Vec<_>>(),
                "creates": f.creates.iter().map(|c| serde_json::json!({ "kind": c.kind, "name": c.name })).collect::<Vec<_>>(),
                "last_run": last,
            });
            put_here(&mut row, &s.here);
            row
        })
        .collect();
    let note = over.unwrap_or_else(|| FIXTURES_IN_SHORT.to_string());
    serde_json::json!({ "fixtures": fixtures, "note": note }).to_string()
}

/// `POST /api-template-fixture-save`: checks the fixture against the saved
/// templates (`fixture::validate`) and saves it, or answers 400 with every
/// refusal sentence as it is, one per line. Needs the API templates switch.
pub fn api_fixture_save(ctx: &BridgeContext, body: &str) -> (u16, String) {
    use crate::api_templates::fixture::Fixture;
    use crate::api_templates::fixture_store;
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FIXTURE_SAVE_SHAPE}.")),
    };
    let Some(raw) = json_arg(v.get("fixture")) else {
        return (400, format!("this call needs a \"fixture\". Expected {FIXTURE_SAVE_SHAPE}."));
    };
    let fixture: Fixture = match serde_json::from_value(raw.clone()) {
        Ok(f) => f,
        Err(e) => {
            let said = crate::api_templates::fixture::shape_problem(&raw, &e);
            return (400, format!("that is not a fixture: {said} - call get_guide (topic api_templates) for the format"));
        }
    };
    let existed = matches!(fixture_store::load(&root, &ctx.org, &ctx.project, &fixture.id), Ok(Some(_)));
    match fixture_store::save(&root, &ctx.org, &ctx.project, &fixture) {
        Ok(()) => {
            crate::applog::info(format!("api fixture {}: {}", fixture.id, if existed { "replaced" } else { "saved" }));
            templates_changed(&fixture.id);
            let mut answer = serde_json::json!({ "saved": fixture.id, "replaced": existed });
            // The stages it says this record does not include, echoed so a
            // wrong name is seen at once.
            let templates = |id: &str| crate::api_templates::store::load(&root, &ctx.org, &ctx.project, id).ok().flatten();
            let flows = |id: &str| crate::api_templates::flow_store::load(&root, &ctx.org, &ctx.project, id).ok().flatten();
            let skipped = crate::api_templates::fixture::skipped_stages(&fixture, &templates, &flows);
            if !skipped.is_empty() {
                answer["skipped_stages"] = serde_json::json!(skipped);
            }
            (200, answer.to_string())
        }
        Err(problems) => (400, problems.join("\n")),
    }
}

/// `POST /api-template-fixture-run`: runs a saved fixture - a first build
/// and a Rebuild alike - and answers with its sentence, outputs, what it
/// made and its warnings: 200 when every step passed, 502 when one did not.
/// Needs the API templates switch. The browser factory is handed in, as
/// `api_template_run`'s is, so a test reaches all of it but a real browser.
pub async fn api_fixture_run<B: crate::autorun::replay::Browsers>(
    ctx: &BridgeContext,
    body: &str,
    open: impl FnOnce(crate::browser::launch::Browser) -> B,
    timing: &crate::browser::timing::Timing,
) -> (u16, String) {
    use crate::api_templates::fixture_run::{run_saved, NotRun};
    if !ctx.api_writes {
        return (400, API_WRITES_OFF.to_string());
    }
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FIXTURE_RUN_SHAPE}.")),
    };
    let id = match v.get("id") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return (400, format!("this call needs an \"id\". Expected {FIXTURE_RUN_SHAPE}.")),
    };
    let which = match browser_arg(&v) {
        Ok(b) => b,
        Err(refused) => return refused,
    };
    let mut browsers = open(which);
    let may_run = || assistant_may_still_write(ctx, &root);
    let source = crate::api_templates::here::TrySource::Assistant;
    let report = match run_saved(&mut browsers, &root, &ctx.org, &ctx.project, &id, timing, may_run, source).await {
        Ok(r) => r,
        Err(NotRun::Busy) => return (409, API_TEMPLATE_BUSY.to_string()),
        Err(NotRun::Refused(why)) => return (400, why),
    };
    drop(browsers);
    templates_changed(&id);
    let made: Vec<serde_json::Value> =
        report.made.iter().map(|m| serde_json::json!({ "kind": m.kind, "id": m.id, "name": m.name })).collect();
    let out = serde_json::json!({
        "ok": report.ok,
        "sentence": report.message(),
        "outputs": report.outputs,
        "made": made,
        "warnings": report.warnings,
    });
    (if report.ok { 200 } else { 502 }, out.to_string())
}

pub(crate) const FLOW_SAVE_SHAPE: &str = "{ \"flow\": <the flow>, \"sample\": <the subject of a real record>, \"replace\"?: true, \"why\"?: \"<reason>\" }";
pub(crate) const FLOW_PROGRESS_SHAPE: &str = "{ \"flow\": \"<flow id>\", \"subject\": <the record's id> }";

/// What a flow save is told when it would replace a flow without `replace: true` and a why,
/// whether that is found before the database checks or at the moment of writing.
pub(crate) fn flow_already_exists(id: &str) -> String {
    format!("a flow called \"{id}\" already exists - send replace: true and a why to change it")
}

/// `POST /api-template-flow-save` (design doc "API template flows" §6): a
/// flow is saved only once every stage's check has run on `sample`, a real
/// record - done or not done both fine, a check that could not run is not.
/// Every problem with the call comes back together, before the database is
/// asked for. Replacing a saved flow needs `replace: true` and a `why`, and
/// the answer names the saved templates whose stage it no longer has: they
/// are refused until a replacement is proven. The tab is told through the
/// same sink a prove or a run uses. Writes nothing to the application, so
/// the API templates switch is not needed.
pub async fn api_template_flow_save<D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
) -> (u16, String) {
    use crate::api_templates::flow::{parse_flow, FlowSaved};
    use crate::api_templates::flow_store;
    use crate::api_templates::gate::{self, CheckFor, StageState};
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FLOW_SAVE_SHAPE}.")),
    };
    let Some(draft) = json_arg(v.get("flow")) else {
        return (400, format!("this call needs a \"flow\". Expected {FLOW_SAVE_SHAPE}."));
    };
    let sample = match v.get("sample") {
        None | Some(serde_json::Value::Null) => None,
        Some(s) => Some(s.clone()),
    };
    let replace = v["replace"].as_bool().unwrap_or(false);
    let why = v["why"].as_str().map(str::to_string);
    let (org, project) = (ctx.org.as_str(), ctx.project.as_str());

    let mut problems = Vec::new();
    let parsed = match parse_flow(&draft) {
        Ok(f) => Some(f),
        Err(p) => {
            problems.extend(p);
            None
        }
    };
    match (&parsed, &sample) {
        (_, None) => problems.push(format!(
            "this call needs a \"sample\": the subject of a real record, found with db_query. Expected {FLOW_SAVE_SHAPE}."
        )),
        (Some(f), Some(s)) => {
            if let Err(e) = gate::validate_value(f, s) {
                problems.push(e);
            }
        }
        (None, Some(_)) => {}
    }
    let mut replacing = false;
    let may_replace = replace && why.as_deref().is_some_and(|w| !w.trim().is_empty());
    if let Some(f) = &parsed {
        replacing = match flow_store::load(&root, org, project, &f.id) {
            Ok(found) => found.is_some(),
            // A saved file that no longer reads still stands for a flow of
            // that id, so replacing it needs the same reason.
            Err(e) => {
                crate::applog::warn(format!("api template flow {}: the saved copy could not be read: {e}", f.id));
                true
            }
        };
        if replacing && !may_replace {
            problems.push(flow_already_exists(&f.id));
        }
    }
    let (Some(mut f), Some(sample), true) = (parsed, sample, problems.is_empty()) else {
        return (400, problems.join("\n"));
    };

    let db = match open_db(ctx) {
        Ok(d) => d,
        Err(refused) => return refused,
    };
    let checked = CheckFor { purpose: "save", template: None };
    let mut results: Vec<(String, String, StageState)> = Vec::with_capacity(f.stages.len());
    for s in &f.stages {
        let state = gate::stage_state(&db, &f, s, &sample, &checked).await;
        results.push((s.id.clone(), s.title.clone(), state));
    }
    let failed: Vec<String> = results
        .iter()
        .filter(|(_, _, st)| *st == StageState::CouldNotRun)
        .map(|(_, title, _)| {
            format!(
                "the check for {title} could not be run on {} {} - see the activity folder in Settings, Logs",
                f.subject.name,
                gate::shown(&sample)
            )
        })
        .collect();
    if !failed.is_empty() {
        return (400, failed.join("\n"));
    }

    f.saved = Some(FlowSaved { at: crate::applog::stamp(), sample: sample.clone() });
    // The early check ran before the database awaits above; a second call may have saved this
    // id since. Looked at again at the moment of writing, under the store's lock, so a call that
    // did not say replace with a why never replaces a flow it did not see.
    match flow_store::save_if(&root, org, project, &f, !may_replace) {
        Ok(()) => {}
        Err(flow_store::SaveIfError::Appeared) => {
            crate::applog::warn(format!("api template flow {}: another call saved it while this one was checked", f.id));
            return (409, flow_already_exists(&f.id));
        }
        Err(flow_store::SaveIfError::Failed(e)) => {
            crate::applog::warn(format!("api template flow {}: it could not be saved: {e}", f.id));
            return (500, "the flow could not be saved - see Settings, Logs".to_string());
        }
    }
    if replacing {
        crate::applog::info(format!(
            "api template flow {} replaced: {}",
            f.id,
            one_short_line(why.as_deref().unwrap_or(""))
        ));
    }

    let saved = saved_templates(&root, org, project);
    // The rest of the map: saved templates no flow places yet. A flow is
    // meant to take in every template found for its record, so the answer
    // hands the assistant the ones still outside one.
    let loose: Vec<(String, String, String)> = saved
        .iter()
        .filter(|s| s.template.stage.is_none())
        .map(|s| (s.template.id.clone(), s.template.title.clone(), s.template.module.clone()))
        .collect();
    let orphaned: Vec<(String, String, String)> = saved
        .into_iter()
        .filter_map(|s| {
            let r = s.template.stage?;
            (r.flow == f.id && !f.stages.iter().any(|st| st.id == r.id))
                .then_some((s.template.id, s.template.title, r.id))
        })
        .collect();
    let mut message = if orphaned.is_empty() {
        format!("flow {} saved; every check ran on {} {}", f.id, f.subject.name, gate::shown(&sample))
    } else {
        let ids: Vec<&str> = orphaned.iter().map(|(id, _, _)| id.as_str()).collect();
        format!(
            "flow {} saved. These templates perform a stage it no longer has, so they are refused until a replacement is proven: {}",
            f.id,
            ids.join(", ")
        )
    };
    if !loose.is_empty() {
        let ids: Vec<&str> = loose.iter().map(|(id, _, _)| id.as_str()).collect();
        message.push_str(&format!(
            ". {} saved template{} on no flow yet: {}. Place each one acting on this record on a stage of this flow (add the stage if it is missing), save the flow again, then re-prove the template with its stage set; one acting on another record belongs in that record's own flow.",
            loose.len(),
            if loose.len() == 1 { " is" } else { "s are" },
            ids.join(", ")
        ));
    }
    templates_changed(&f.id);
    let stages: Vec<serde_json::Value> = results
        .iter()
        .map(|(id, title, st)| serde_json::json!({ "id": id, "title": title, "done": *st == StageState::Done }))
        .collect();
    let orphaned: Vec<serde_json::Value> = orphaned
        .into_iter()
        .map(|(id, title, stage)| serde_json::json!({ "id": id, "title": title, "stage": stage }))
        .collect();
    let loose: Vec<serde_json::Value> = loose
        .into_iter()
        .map(|(id, title, module)| serde_json::json!({ "id": id, "title": title, "module": module }))
        .collect();
    (
        200,
        serde_json::json!({
            "saved": f.id,
            "sample": sample,
            "stages": stages,
            "orphaned": orphaned,
            "not_on_a_flow": loose,
            "message": message,
        })
        .to_string(),
    )
}

/// `POST /api-template-flow-progress`: every stage of a saved flow for one
/// record, each checked once - `done`, `next`, `blocked`, `skippable` or
/// `could_not_check` - with the templates on each (design doc §7). Only
/// reads, so the API templates switch is not needed.
pub async fn api_template_flow_progress<D: crate::api_templates::gate::StageDb>(
    ctx: &BridgeContext,
    body: &str,
    open_db: impl FnOnce(&BridgeContext) -> Result<D, (u16, String)>,
) -> (u16, String) {
    use crate::api_templates::{flow_store, gate, valid_id};
    let root = match autorun_root() {
        Ok(r) => r,
        Err(refused) => return refused,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return (400, format!("that is not readable JSON: {e}. Expected {FLOW_PROGRESS_SHAPE}.")),
    };
    let id = match v.get("flow") {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return (400, format!("this call needs a \"flow\" id. Expected {FLOW_PROGRESS_SHAPE}.")),
    };
    let subject = match v.get("subject") {
        None | Some(serde_json::Value::Null) => {
            return (400, format!("this call needs a \"subject\": the record's id. Expected {FLOW_PROGRESS_SHAPE}."))
        }
        Some(s) => s.clone(),
    };
    let not_saved =
        || format!("no flow called \"{id}\" is saved for this project - list_api_templates shows the ones that are");
    if !valid_id(&id) {
        return (400, not_saved());
    }
    let f = match flow_store::load(&root, &ctx.org, &ctx.project, &id) {
        Ok(Some(f)) => f,
        Ok(None) => return (400, not_saved()),
        Err(e) => {
            crate::applog::warn(format!("api template flow {id}: it could not be read: {e}"));
            return (400, format!("flow {id} could not be read - save it again with save_api_flow"));
        }
    };
    // The wrong type is said before any database is asked for.
    if let Err(e) = gate::validate_value(&f, &subject) {
        return (400, e);
    }
    let db = match open_db(ctx) {
        Ok(d) => d,
        Err(refused) => return refused,
    };
    let templates = saved_templates(&root, &ctx.org, &ctx.project);
    match gate::progress(&db, &f, &subject, &templates).await {
        Ok(stages) => (200, serde_json::json!({ "flow": f.id, "subject": subject, "stages": stages }).to_string()),
        Err(e) => (400, e),
    }
}

