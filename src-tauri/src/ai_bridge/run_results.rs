//! Run results, PBI search and wiki reads from Azure DevOps.

use super::*;

/// How many matching results get their comment + linked bugs fetched. Each
/// one is its own request; a suite with 80 failures is a suite with a
/// bigger problem than missing detail text.
pub(crate) const RUN_FAILURE_DETAIL_CAP: usize = 10;
/// How many matching results are listed at all - id, title, configuration
/// and outcome come with the points, so listing costs no extra request.
pub(crate) const RUN_RESULT_LIST_CAP: usize = 200;

/// Every outcome a test point can carry, in the order the summary lists
/// them: key (Azure DevOps' own value, lowercased), the label an assistant
/// and a person read, and other spellings accepted when asking for it.
/// "Never run" is a point with no verdict yet - the Test Plans UI calls it
/// Active. The last seven are the automated-test outcomes; a manual suite
/// rarely has them, but a filter for one should not be refused as a typo.
pub const RUN_OUTCOMES: &[(&str, &str, &[&str])] = &[
    ("failed", "Failed", &["fail"]),
    ("blocked", "Blocked", &[]),
    ("paused", "Paused", &[]),
    ("inprogress", "In progress", &[]),
    ("notapplicable", "Not applicable", &["na"]),
    ("passed", "Passed", &["pass"]),
    ("neverrun", "Never run", &["notrun", "never", "active", "unspecified", "none"]),
    ("error", "Error", &[]),
    ("timeout", "Timeout", &[]),
    ("aborted", "Aborted", &[]),
    ("inconclusive", "Inconclusive", &[]),
    ("warning", "Warning", &[]),
    ("notexecuted", "Not executed", &[]),
    ("notimpacted", "Not impacted", &[]),
];

pub(crate) fn outcome_word(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// A point's outcome as a RUN_OUTCOMES key. `last_outcome` is empty for a
/// point with no verdict; anything this table does not know keeps its own
/// (normalised) spelling, so it is counted, never dropped.
pub fn outcome_key(last_outcome: &str) -> String {
    if last_outcome.trim().is_empty() {
        "neverrun".into()
    } else {
        outcome_word(last_outcome)
    }
}

/// The label for a key: the table's, or the raw key for one it lacks.
pub fn outcome_label(key: &str) -> String {
    RUN_OUTCOMES
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or_else(|| key.to_string(), |(_, label, _)| label.to_string())
}

/// The outcomes asked for - a comma-separated list in any spelling
/// ("Not Applicable", "not_applicable", "notapplicable"), or "all". None or
/// blank is Failed, what this tool returned before it took a filter. An
/// unknown word is refused with the list, not silently matched to nothing:
/// an empty answer to a typo would read as "nothing had that outcome".
pub fn parse_outcomes(raw: Option<&str>) -> Result<Vec<&'static str>, String> {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
        return Ok(vec!["failed"]);
    };
    let mut keys: Vec<&'static str> = Vec::new();
    for word in raw.split(',').map(outcome_word).filter(|w| !w.is_empty()) {
        if word == "all" {
            return Ok(RUN_OUTCOMES.iter().map(|(k, _, _)| *k).collect());
        }
        let Some((key, _, _)) = RUN_OUTCOMES
            .iter()
            .find(|(k, _, aliases)| *k == word || aliases.contains(&word.as_str()))
        else {
            let known: Vec<&str> = RUN_OUTCOMES.iter().map(|(_, l, _)| *l).collect();
            return Err(format!(
                "\"{word}\" is not a test outcome. Use one or more of: {}, or \"all\"",
                known.join(", ")
            ));
        };
        if !keys.contains(key) {
            keys.push(key);
        }
    }
    if keys.is_empty() {
        return Ok(vec!["failed"]);
    }
    Ok(keys)
}

/// The project a work item (a PBI, or a test plan) is in, from the session
/// cache or one organization-level work item read. A work item never changes
/// project, so the answer is kept for `WORK_ITEM_PROJECT_TTL`.
pub(crate) async fn work_item_project_cached(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    pbi: i32,
) -> Result<String, crate::ado::AdoError> {
    let key = crate::cache::keys::work_item_project(&client.base_url, &ctx.org, pbi);
    if let Some(p) = crate::cache::session_fresh::<String>(&key, crate::cache::keys::WORK_ITEM_PROJECT_TTL) {
        return Ok(p);
    }
    let project = client.work_item_project(&ctx.org, pbi).await?;
    crate::cache::session_put(&key, project.clone());
    Ok(project)
}

/// The cases from a PBI's latest runs with the outcomes asked for (Failed
/// unless told otherwise), each with its comment and linked bugs, plus a
/// count of every outcome in the suite - what an assistant needs to draft
/// regression cases, or to see where a PBI's testing stands.
///
/// GET-only end to end: `find_pbi_requirement_suite` is the find-ONLY
/// scan, never the find-or-create one. A PBI with no suite is an answer
/// ("this PBI has never had a run"), not a reason to create anything -
/// this is the bridge, and the bridge does not write to Azure DevOps.
/// The resolved suite comes from the cache shared with the upload and Run
/// Tests (`ado_testplan::cached_suite`): resolving scans every plan and
/// took about a minute on a large org, which is what made the MCP proxy
/// give up at 30s and blame the connection.
pub(crate) async fn run_results(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(pbi) = q(target, "pbi").and_then(|v| v.parse::<i32>().ok()) else {
        return (400, "pass ?pbi=<work item id> (find one with search_pbis)".into());
    };
    if pbi <= 0 {
        return (400, crate::mcp::RUN_RESULTS_NEEDS_PBI.into());
    }
    let wanted = match parse_outcomes(q(target, "outcome").as_deref()) {
        Ok(w) => w,
        Err(why) => return (400, why),
    };
    // Combined, the PBI's own project holds its suite and results; the home
    // project is only where this app keeps its files. Switched off, the
    // project the bridge was given, with no lookup.
    let project = if ctx.all_projects {
        match work_item_project_cached(ctx, client, pbi).await {
            Ok(p) => p,
            Err(crate::ado::AdoError::NotFound) => {
                return (
                    404,
                    format!("Work item {pbi} was not found in this organization (find one with search_pbis)."),
                )
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    } else {
        ctx.project.clone()
    };
    let project = project.as_str();
    let mut retried = false;
    let (suite, points) = loop {
        let cached = crate::ado_testplan::cached_suite(&client.base_url, &ctx.org, project, pbi);
        let (suite, from_cache) = match cached {
            Some(s) => (s, true),
            None => {
                // No area path here: the scan only uses it to order plans,
                // and it checks every plan regardless, so "" costs at most
                // a slower hit.
                match client
                    .find_pbi_requirement_suite(&ctx.org, project, pbi, "")
                    .await
                {
                    Ok(Some(s)) => {
                        crate::ado_testplan::remember_suite(&client.base_url, &ctx.org, project, pbi, &s);
                        (s, false)
                    }
                    Ok(None) => {
                        return (
                            200,
                            serde_json::json!({
                                "pbi": pbi,
                                "summary": [],
                                "results": [],
                                "note": "This PBI has no test suite, so it has never had a test run - there are no results to read.",
                            })
                            .to_string(),
                        )
                    }
                    Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
                }
            }
        };
        match client
            .get_test_points(&ctx.org, project, suite.plan_id, suite.suite_id, &[])
            .await
        {
            Ok(p) => break (suite, p),
            // A cached suite that 404s was deleted in Azure DevOps since
            // it was resolved: forget it and scan once from scratch.
            Err(crate::ado::AdoError::NotFound) if from_cache && !retried => {
                crate::ado_testplan::forget_suite(&client.base_url, &ctx.org, project, pbi);
                retried = true;
            }
            Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
        }
    };

    let total = points.len();
    // Every outcome in the suite, counted - the table's order first, then
    // anything it does not know, so nothing in the suite goes uncounted.
    let mut counts: Vec<(String, usize)> = Vec::new();
    for p in &points {
        let key = outcome_key(&p.last_outcome);
        match counts.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => counts.push((key, 1)),
        }
    }
    let rank = |k: &str| RUN_OUTCOMES.iter().position(|(key, _, _)| *key == k).unwrap_or(usize::MAX);
    counts.sort_by_key(|(k, _)| rank(k));
    let summary: Vec<serde_json::Value> = counts
        .iter()
        .map(|(k, n)| serde_json::json!({ "outcome": outcome_label(k), "count": n }))
        .collect();

    let matching: Vec<_> = points
        .into_iter()
        .filter(|p| wanted.contains(&outcome_key(&p.last_outcome).as_str()))
        .collect();
    let matched = matching.len();

    let mut results: Vec<serde_json::Value> = vec![];
    for (i, p) in matching.iter().take(RUN_RESULT_LIST_CAP).enumerate() {
        // The comment is where the tester wrote what actually happened -
        // fetched per result, best-effort, for the first few only: a result
        // whose detail cannot be read is still a result worth naming. A
        // point that never ran has no result to read.
        let detail = match (p.last_run_id, p.last_result_id) {
            (Some(run), Some(res)) if i < RUN_FAILURE_DETAIL_CAP => Some(
                client
                    .get_result_report_info(&ctx.org, project, run, res)
                    .await
                    .unwrap_or_default(),
            ),
            _ => None,
        };
        let mut row = serde_json::json!({
            "case_id": p.test_case_id,
            "title": p.test_case_name,
            "configuration": p.config_name,
            "outcome": outcome_label(&outcome_key(&p.last_outcome)),
            "run_id": p.last_run_id,
        });
        if let Some((comment, bug_ids)) = detail {
            row["comment"] = serde_json::json!(comment);
            row["bug_ids"] = serde_json::json!(bug_ids);
        }
        results.push(row);
    }

    let mut out = serde_json::json!({
        "pbi": pbi,
        "plan": { "id": suite.plan_id, "name": suite.plan_name },
        "cases_in_suite": total,
        "summary": summary,
        "outcomes": wanted.iter().map(|k| outcome_label(k)).collect::<Vec<_>>(),
        "matched": matched,
        "results": results,
        "note": "`summary` counts every outcome in the PBI's suite; `results` lists the cases with the outcomes asked for. A result's `comment` is what the tester wrote and `bug_ids` are the bugs they linked. To write regression cases from failures, start with begin_test_case_writing as usual - and read the failed case itself via get_test_cases so the regression case extends it instead of restating it.",
    });
    let mut limits: Vec<String> = Vec::new();
    if matched > RUN_RESULT_LIST_CAP {
        limits.push(format!("{matched} cases matched; the first {RUN_RESULT_LIST_CAP} are listed."));
    }
    if matched > RUN_FAILURE_DETAIL_CAP {
        limits.push(format!(
            "Comments and linked bugs were read for the first {RUN_FAILURE_DETAIL_CAP} only - ask for fewer outcomes to see the others' detail."
        ));
    }
    if !limits.is_empty() {
        out["truncated"] = serde_json::json!(limits.join(" "));
    }
    (200, out.to_string())
}

/// PBI search so the AI can anchor examples/output to the right item.
pub(crate) async fn search_pbis(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(query) = q(target, "q").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?q=<search text>".into());
    };
    if ctx.all_projects {
        // Every project's PBIs; each hit names its own project.
        return match client.search_pbis_all(&ctx.org, &query, 20).await {
            Ok(hits) => (
                200,
                serde_json::json!({
                    "pbis": hits.iter().map(|h| serde_json::json!({
                        "id": h.id, "title": h.title, "work_item_type": h.work_item_type,
                        "project": h.project
                    })).collect::<Vec<_>>()
                })
                .to_string(),
            ),
            Err(e) => (502, format!("Azure DevOps error: {e:?}")),
        };
    }
    match client.search_pbis(&ctx.org, &ctx.project, &query, 20).await {
        Ok(hits) => (
            200,
            serde_json::json!({
                "pbis": hits.iter().map(|h| serde_json::json!({
                    "id": h.id, "title": h.title, "work_item_type": h.work_item_type
                })).collect::<Vec<_>>()
            })
            .to_string(),
        ),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

/// Wiki documentation search, so the AI can ground its output in the
/// project's own docs instead of guessing.
pub(crate) async fn search_wiki(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(query) = q(target, "q").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?q=<search text>".into());
    };
    let top = q(target, "top")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(10)
        .min(25);
    if ctx.all_projects {
        // The organization's wikis; each hit names its own project, which
        // `get_wiki_page` takes back as `project`.
        return match client.search_wiki_all(&ctx.org, &query, top).await {
            Ok(hits) => (
                200,
                serde_json::json!({
                    "results": hits.iter().map(|h| serde_json::json!({
                        "file_name": h.file_name,
                        "path": h.path,
                        "wiki_name": h.wiki_name,
                        "wiki_id": h.wiki_id,
                        "project": h.project,
                        "highlights": h.highlights,
                    })).collect::<Vec<_>>()
                })
                .to_string(),
            ),
            Err(e) => (502, format!("Azure DevOps error: {e:?}")),
        };
    }
    match client.search_wiki(&ctx.org, &ctx.project, &query, top).await {
        Ok(hits) => (
            200,
            serde_json::json!({
                "results": hits.iter().map(|h| serde_json::json!({
                    "file_name": h.file_name,
                    "path": h.path,
                    "wiki_name": h.wiki_name,
                    "wiki_id": h.wiki_id,
                    "highlights": h.highlights,
                })).collect::<Vec<_>>()
            })
            .to_string(),
        ),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}

/// Full markdown content of one wiki page. `path` is a page path, a
/// `search_wiki` hit's path, or the URL from the browser - see
/// `AdoClient::get_wiki_page`. `wiki` is required for all but the URL.
pub(crate) async fn wiki_page(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let Some(path) = q(target, "path").filter(|s| !s.trim().is_empty()) else {
        return (400, "pass ?path=<page path, search path, or wiki url>".into());
    };
    // A wiki URL names its own wiki, so asking for `wiki` as well would
    // refuse the one thing a person has to hand. Every other form still
    // needs it.
    let trimmed = path.trim();
    let is_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    let wiki_id = q(target, "wiki").filter(|s| !s.trim().is_empty()).unwrap_or_default();
    if wiki_id.is_empty() && !is_url {
        return (400, "pass ?wiki=<wiki id>&path=<page path>, or ?path=<wiki url>".into());
    }
    // One wiki name or id - never a route. The decoded value used to reach
    // the URL raw, so `..%2F` climbed out to any GET endpoint.
    if wiki_id.contains('/') || wiki_id.contains('\\') || wiki_id.contains("..") {
        return (400, "the wiki parameter is a wiki name or id - it cannot contain '/', '\\' or '..'".into());
    }
    // While combined the page may be in any project: the hit's (`?project=`),
    // else the one a pasted URL names, else home. Off, the chosen project.
    let project = if ctx.all_projects {
        q(target, "project")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .or_else(|| {
                crate::ado::endpoints::wiki_url_org_project(&path)
                    .filter(|(org, _)| org.eq_ignore_ascii_case(&ctx.org))
                    .map(|(_, project)| project)
            })
            .unwrap_or_else(|| ctx.project.clone())
    } else {
        ctx.project.clone()
    };
    match client.get_wiki_page(&ctx.org, &project, &wiki_id, &path).await {
        Ok(page) => (
            200,
            serde_json::json!({ "path": page.path, "content": page.content }).to_string(),
        ),
        Err(e) => (502, format!("Azure DevOps error: {e:?}")),
    }
}



