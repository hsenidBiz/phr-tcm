//! Reading test cases, test suites and a suite's cases from Azure DevOps.

use super::*;

/// How many case ids one call may name. The batch read chunks at 200, but a
/// list longer than this is a whole suite - get_suite_test_cases reads that.
pub(crate) const MAX_CASE_IDS: usize = 200;

/// Real cases in the import JSON record shape, so they double as format
/// demonstrations. Three ways in:
/// - `pbi` alone: every case the PBI is tested by;
/// - `ids` alone: those cases, in the order asked - no PBI needed;
/// - both: the PBI's cases narrowed to those ids, with `not_on_pbi` naming
///   any id the PBI is not tested by.
pub(crate) async fn test_cases(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let pbi = match q(target, "pbi") {
        None => None,
        Some(v) => match v.parse::<i32>() {
            Ok(id) => Some(id),
            Err(_) => return (400, format!("pbi must be a work item id, got {v:?}")),
        },
    };
    let ids = match q(target, "ids") {
        None => None,
        Some(v) => match parse_case_ids(&v) {
            Ok(ids) => Some(ids),
            Err(msg) => return (400, msg),
        },
    };
    let (limit, offset, titles_only) = paging(target);
    let (module_ref, preconditions_ref) = (ctx.module_ref.as_deref(), ctx.preconditions_ref.as_deref());

    match (pbi, ids) {
        (None, None) => (
            400,
            "pass ?pbi=<PBI work item id> for the cases a PBI is tested by (find one with search_pbis), \
             or ?ids=<test case ids, comma-separated> to read cases by their own ids"
                .into(),
        ),
        (Some(pbi), ids) => {
            let mut cases = match client.get_pbi_test_cases_full(&ctx.org, pbi, module_ref, preconditions_ref).await {
                Ok(c) => c,
                Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
            };
            // Existing step attachments, by name; a failed read only logs.
            if !titles_only {
                client.load_step_attachments(&ctx.org, &mut cases).await;
            }
            let Some(ids) = ids else {
                return (200, case_page(&cases, limit, offset, titles_only).to_string());
            };
            let mut picked: Vec<_> = cases.into_iter().filter(|c| ids.contains(&c.id)).collect();
            picked.sort_by_key(|c| ids.iter().position(|&i| i == c.id).unwrap_or(usize::MAX));
            let not_on_pbi: Vec<i32> =
                ids.iter().copied().filter(|i| !picked.iter().any(|c| c.id == *i)).collect();
            let mut out = case_page(&picked, limit, offset, titles_only);
            if !not_on_pbi.is_empty() {
                out["not_on_pbi"] = serde_json::json!(not_on_pbi);
                out["note_not_on_pbi"] = serde_json::json!(format!(
                    "PBI #{pbi} is not tested by these ids; call get_test_cases with case_ids alone (no pbi_id) to read them wherever they are."
                ));
            }
            (200, out.to_string())
        }
        (None, Some(ids)) => match client.get_test_cases_by_ids(&ctx.org, &ids, module_ref, preconditions_ref).await {
            Ok(mut cases) => {
                if !titles_only {
                    client.load_step_attachments(&ctx.org, &mut cases).await;
                }
                // The batch read answers in id order; give back the order asked.
                cases.sort_by_key(|c| ids.iter().position(|&i| i == c.id).unwrap_or(usize::MAX));
                (200, case_page(&cases, limit, offset, titles_only).to_string())
            }
            // ADO refuses the whole batch when any id is not a work item.
            Err(crate::ado::AdoError::NotFound) => (
                404,
                format!(
                    "Azure DevOps has no work item for at least one of {} - check the ids, or find the case through its PBI or suite",
                    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
                ),
            ),
            Err(e) => (502, format!("Azure DevOps error: {e:?}")),
        },
    }
}

/// `ids=12,34` as a de-duplicated list in the order given. A token that is
/// not a whole number is refused rather than dropped, so a typo cannot
/// quietly return fewer cases than were asked for.
pub(crate) fn parse_case_ids(raw: &str) -> Result<Vec<i32>, String> {
    let mut ids: Vec<i32> = vec![];
    for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let id = token
            .trim_start_matches('#')
            .parse::<i32>()
            .map_err(|_| format!("ids must be test case work item ids, got {token:?}"))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("ids is empty - pass one or more test case ids, comma-separated".into());
    }
    if ids.len() > MAX_CASE_IDS {
        return Err(format!(
            "at most {MAX_CASE_IDS} ids per call - for a whole suite use get_suite_test_cases"
        ));
    }
    Ok(ids)
}

/// The paging arguments every case-listing route reads the same way:
/// `limit` (default 5, cap 20), `offset`, and `titles_only`.
pub(crate) fn paging(target: &str) -> (usize, usize, bool) {
    let limit = q(target, "limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5)
        .min(20);
    let offset = q(target, "offset").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    // Titles-only mode exists for duplicate checking: comparing titles
    // against a set of 60 cases must not cost 60 cases of step text.
    let titles_only = matches!(q(target, "titles_only").as_deref(), Some("true") | Some("1"));
    (limit, offset, titles_only)
}

/// One page of cases in the import JSON record shape, with the total and
/// a note when the page did not reach the end. Shared by the PBI and the
/// suite listings so the two never drift apart in shape.
pub(crate) fn case_page(
    cases: &[crate::ado::TestCaseFull],
    limit: usize,
    offset: usize,
    titles_only: bool,
) -> serde_json::Value {
    let total = cases.len();
    // Titles are cheap: a fixed 200, not the caller's limit, so one call
    // can cover a whole set when all it needs is duplicate checking.
    let page = if titles_only { 200 } else { limit };
    let records: Vec<serde_json::Value> = cases
        .iter()
        .skip(offset)
        .take(page)
        .map(|c| {
            if titles_only {
                serde_json::json!({ "id": c.id, "title": c.title })
            } else {
                let mut rec = serde_json::json!({
                    "id": c.id,
                    "title": c.title,
                    "tags": c.tags,
                    "automation_status": c.automation_status,
                    "module": c.module_value,
                    "preconditions": c.preconditions,
                    "steps": c.steps.iter().map(crate::import_parser::step_json).collect::<Vec<_>>(),
                });
                // Present only when the work item has one, the importer's
                // shape rule for optional fields.
                if let Some(p) = c.priority {
                    rec["priority"] = serde_json::json!(p);
                }
                rec
            }
        })
        .collect();
    let returned = records.len();
    let mut out = serde_json::json!({
        "test_cases": records,
        "total": total,
        "offset": offset,
    });
    if !titles_only && cases.iter().skip(offset).take(page).any(|c| !c.attachments_read) {
        out["attachments_note"] = serde_json::json!(
            "Step attachments could not be read from Azure DevOps, so none are shown: a step may have files. \
             Do not attach a file again on this evidence."
        );
    }
    if offset + returned < total {
        // Say so explicitly: a silently incomplete page is how a
        // duplicate check quietly misses cases.
        out["note"] = serde_json::json!(format!(
            "{} of {} cases returned - pass offset={} for the next page.",
            returned,
            total,
            offset + returned
        ));
    }
    out
}

pub(crate) type SuiteTree = Vec<crate::ado_testplan::PlanWithSuites>;

/// The plans and suites the Test Suites tab shows for `project`, cached in
/// memory per org and project for `cache::keys::SUITE_TREE_TTL`;
/// `refresh=true` reads them again. Session tier: the tree is large, and
/// TestPlan's `#[serde(skip)]` fields would not survive a trip through the
/// disk.
pub(crate) async fn suite_tree(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    project: &str,
    refresh: bool,
) -> Result<SuiteTree, crate::ado::AdoError> {
    let key = crate::cache::keys::suite_tree(&client.base_url, &ctx.org, project);
    if !refresh {
        if let Some(tree) = crate::cache::session_fresh::<SuiteTree>(&key, crate::cache::keys::SUITE_TREE_TTL) {
            return Ok(tree);
        }
    }
    let tree = client.list_plans_with_suites(&ctx.org, project).await?;
    crate::cache::session_put(&key, tree.clone());
    Ok(tree)
}

/// The project that holds test plan `plan`, for a combined `suite-cases`
/// call that was not told: the project whose cached suite tree lists the
/// plan (the suite search keeps them), else one organization-level work item
/// read, because a test plan is a work item and a work item never changes
/// project. A project list that cannot be read only costs the shortcut.
pub(crate) async fn plan_project(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    plan: i32,
) -> Result<String, crate::ado::AdoError> {
    if let Ok(names) = client.org_project_names(&ctx.org).await {
        for name in names {
            let key = crate::cache::keys::suite_tree(&client.base_url, &ctx.org, &name);
            if let Some(tree) = crate::cache::session_fresh::<SuiteTree>(&key, crate::cache::keys::SUITE_TREE_TTL) {
                if tree.iter().any(|p| p.plan.id == plan) {
                    return Ok(name);
                }
            }
        }
    }
    work_item_project_cached(ctx, client, plan).await
}

/// Every suite in the project, one row each with its plan, filtered by
/// `q` against the plan name, the suite name, or a requirement id. The
/// plan id and suite id in a row are what `get_suite_test_cases` takes.
/// Combined, the rows are every project's (capped, one project at a time),
/// each naming its project, and the answer says how many projects it read.
pub(crate) async fn suites(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    suites_within(ctx, client, target, SUITES_WALK_BUDGET).await
}

/// How long the combined suites walk goes on starting projects. A cold walk
/// of a big organization can outlast the MCP proxy's own limit on a call
/// (mcp.rs, 300 s), which would report the app unreachable; past this the
/// walk answers with what it has read, and the trees it did read stay cached
/// so the next call carries on from there.
pub(crate) const SUITES_WALK_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

/// `suites` with the time the combined walk may spend starting projects. A
/// project already started is read to the end, so a call can run up to one
/// project's read past the budget.
pub async fn suites_within(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
    budget: std::time::Duration,
) -> (u16, String) {
    let started = std::time::Instant::now();
    const CAP: usize = 200;
    let query = q(target, "q").map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let refresh = matches!(q(target, "refresh").as_deref(), Some("true") | Some("1"));
    // (the project a tree belongs to, named in a row only while combined; the tree)
    let mut trees: Vec<(Option<String>, SuiteTree)> = vec![];
    let mut coverage: Option<crate::ado::fanout::ProjectCoverage> = None;
    if ctx.all_projects {
        let names = match client.org_project_names(&ctx.org).await {
            Ok(n) => n,
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        };
        let (names, total_projects) = crate::ado::fanout::capped(names);
        // One project at a time: each tree is itself a scan of its plans,
        // several requests at once.
        let mut results = Vec::with_capacity(names.len());
        for name in &names {
            // Out of time: answer with what is read (the coverage note below
            // says how much that is). Always at least one project is read.
            if !results.is_empty() && started.elapsed() >= budget {
                break;
            }
            let result = suite_tree(ctx, client, name, refresh).await;
            let stop = matches!(&result, Err(e) if !matches!(e, crate::ado::AdoError::Forbidden | crate::ado::AdoError::NotFound));
            results.push((name.clone(), result));
            if stop {
                break;
            }
        }
        match crate::ado::fanout::gather(results, total_projects) {
            Ok((read, cov)) => {
                trees.extend(read.into_iter().map(|(name, tree)| (Some(name), tree)));
                coverage = Some(cov);
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    } else {
        match suite_tree(ctx, client, &ctx.project, refresh).await {
            Ok(t) => trees.push((None, t)),
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    }
    let mut rows: Vec<serde_json::Value> = vec![];
    for (project, tree) in &trees {
        for p in tree {
            let plan_hit = query.is_empty() || p.plan.name.to_lowercase().contains(&query);
            for s in &p.suites {
                let hit = plan_hit
                    || s.name.to_lowercase().contains(&query)
                    || s.requirement_id.map(|r| r.to_string() == query).unwrap_or(false);
                if !hit {
                    continue;
                }
                let mut row = serde_json::json!({
                    "plan_id": p.plan.id,
                    "plan_name": p.plan.name,
                    "suite_id": s.id,
                    "suite_name": s.name,
                    "suite_type": s.suite_type,
                    "requirement_id": s.requirement_id,
                    "parent_suite_id": s.parent_id,
                });
                if let Some(project) = project {
                    row["project"] = serde_json::json!(project);
                }
                rows.push(row);
            }
        }
    }
    let total = rows.len();
    rows.truncate(CAP);
    let mut out = serde_json::json!({ "suites": rows, "total": total });
    let mut notes: Vec<String> = vec![];
    if total > CAP {
        notes.push(format!(
            "{CAP} of {total} suites returned - pass q=<plan or suite name> to narrow the list."
        ));
    }
    if let Some(cov) = coverage {
        // Said whenever a project is missing, whether the cap cut it or the
        // sign-in cannot read it: a short list must never look complete.
        if cov.shown < cov.total {
            out["projects_shown"] = serde_json::json!(cov.shown);
            out["projects_total"] = serde_json::json!(cov.total);
            notes.push(format!(
                "Suites from {} of {} projects are listed; the rest are past the {} project limit, not readable with this sign-in, or not read in time (call again to read more of them).",
                cov.shown,
                cov.total,
                crate::ado::fanout::MAX_FANOUT_PROJECTS
            ));
        }
    }
    if !notes.is_empty() {
        out["note"] = serde_json::json!(notes.join(" "));
    }
    (200, out.to_string())
}

/// The cases in one suite, in the import JSON record shape, by way of the
/// suite's test points - the same read the Run Tests tab makes. With
/// `children=true` the endpoint's own `isRecursive` flag brings in every
/// child suite's cases, so a folder reads in one call. Combined, the points
/// are read in the plan's own project: the `project` the caller names, else
/// the one found by `plan_project`.
pub(crate) async fn suite_cases(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let (Some(plan), Some(suite)) = (
        q(target, "plan").and_then(|v| v.parse::<i32>().ok()),
        q(target, "suite").and_then(|v| v.parse::<i32>().ok()),
    ) else {
        return (400, "pass ?plan=<plan id>&suite=<suite id> (find them with search_test_suites)".into());
    };
    let (limit, offset, titles_only) = paging(target);
    let children = matches!(q(target, "children").as_deref(), Some("true") | Some("1"));
    // Switched off, the project the bridge was given, with no lookup (and a
    // `project` in the request is ignored).
    let project = if ctx.all_projects {
        match q(target, "project").map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
            Some(p) => p,
            None => match plan_project(ctx, client, plan).await {
                Ok(p) => p,
                Err(crate::ado::AdoError::NotFound) => {
                    return (
                        404,
                        format!("Plan {plan} was not found in this organization (find one with search_test_suites)."),
                    )
                }
                Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
            },
        }
    } else {
        ctx.project.clone()
    };
    let points = match client
        .get_test_points_in(&ctx.org, &project, plan, suite, &[], children)
        .await
    {
        Ok(p) => p,
        Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
    };
    // One point per case per configuration: a suite run on two browsers
    // lists every case twice. Keep the suite's order, each case once.
    let mut ids: Vec<i32> = vec![];
    for p in &points {
        if let Some(id) = p.test_case_id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    match client
        .get_test_cases_by_ids(&ctx.org, &ids, ctx.module_ref.as_deref(), ctx.preconditions_ref.as_deref())
        .await
    {
        Ok(mut cases) => {
            if !titles_only {
                client.load_step_attachments(&ctx.org, &mut cases).await;
            }
            // The work item read answers in id order; put the suite's own
            // order back so the page reads like the suite does.
            cases.sort_by_key(|c| ids.iter().position(|&i| i == c.id).unwrap_or(usize::MAX));
            let mut out = case_page(&cases, limit, offset, titles_only);
            out["plan_id"] = serde_json::json!(plan);
            out["suite_id"] = serde_json::json!(suite);
            if ctx.all_projects {
                out["project"] = serde_json::json!(project);
            }
            (200, out.to_string())
        }
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

