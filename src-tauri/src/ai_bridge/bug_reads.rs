//! Reading bugs from Azure DevOps for the assistant: `GET /bugs`.

use super::*;
use crate::ado::bugs::BugSummary;
use crate::ado::AdoError;

fn bug_json(b: &BugSummary, titles_only: bool) -> serde_json::Value {
    if titles_only {
        serde_json::json!({ "id": b.id, "title": b.title, "state": b.state })
    } else {
        serde_json::to_value(b).unwrap_or_default()
    }
}

fn parse_id_arg(target: &str, key: &str) -> Result<Option<i32>, (u16, String)> {
    match q(target, key) {
        None => Ok(None),
        Some(v) => v
            .trim()
            .trim_start_matches('#')
            .parse::<i32>()
            .map(Some)
            .map_err(|_| (400, format!("{key} must be a work item id, got {v:?}"))),
    }
}

fn parse_bug_ids(raw: &str) -> Result<Vec<i32>, String> {
    let mut ids: Vec<i32> = vec![];
    for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let id = token
            .trim_start_matches('#')
            .parse::<i32>()
            .map_err(|_| format!("ids must be bug work item ids, got {token:?}"))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("ids is empty - pass one or more bug ids, comma-separated".into());
    }
    if ids.len() > MAX_CASE_IDS {
        return Err(format!("at most {MAX_CASE_IDS} ids per call"));
    }
    Ok(ids)
}

fn ado_failure(e: &AdoError) -> (u16, String) {
    (502, format!("Azure DevOps error: {}", e.user_text()))
}

/// The work items linked to `id`, plus (for a PBI) the items linked to the
/// test cases it is tested by. The batch read decides which are bugs.
async fn candidate_ids(
    client: &crate::ado::AdoClient,
    org: &str,
    id: i32,
    through_cases: bool,
) -> Result<Vec<i32>, AdoError> {
    let links = client.linked_ids(org, id).await?;
    let mut out: Vec<i32> = links.iter().map(|(_, i)| *i).collect();
    if through_cases {
        let cases: Vec<i32> = links
            .iter()
            .filter(|(rel, _)| crate::ado::AdoClient::is_tested_by_link(rel))
            .map(|(_, i)| *i)
            .collect();
        for chunk in cases.chunks(crate::ado::READ_CONCURRENCY) {
            let reads = futures::future::join_all(chunk.iter().map(|c| client.linked_ids(org, *c))).await;
            for r in reads {
                out.extend(r?.into_iter().map(|(_, i)| i));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|i| *i != id && seen.insert(*i));
    Ok(out)
}

/// Bugs for `?pbi=`, `?case=` or `?ids=` (alone, or narrowing the others).
pub(crate) async fn bugs(
    ctx: &BridgeContext,
    client: &crate::ado::AdoClient,
    target: &str,
) -> (u16, String) {
    let pbi = match parse_id_arg(target, "pbi") {
        Ok(v) => v,
        Err(r) => return r,
    };
    let case = match parse_id_arg(target, "case") {
        Ok(v) => v,
        Err(r) => return r,
    };
    let ids = match q(target, "ids") {
        None => None,
        Some(v) => match parse_bug_ids(&v) {
            Ok(ids) => Some(ids),
            Err(msg) => return (400, msg),
        },
    };
    if pbi.is_none() && case.is_none() && ids.is_none() {
        return (
            400,
            "pass ?pbi=<PBI work item id> for the bugs on a PBI, ?case=<test case id> for the bugs on a case, \
             or ?ids=<bug ids, comma-separated> to read bugs by their own ids"
                .into(),
        );
    }
    let (limit, offset, titles_only) = paging(target);
    let page = if titles_only { 200 } else { limit };
    let org = ctx.org.as_str();
    let bug_type = match client.detect_bug_type(org, &ctx.project).await {
        Ok(t) => t.wi_type,
        Err(e) => return ado_failure(&e),
    };

    // The bug set the PBI / case defines, if either was named.
    let mut scope: Option<Vec<i32>> = None;
    for (id, through_cases) in [(pbi, true), (case, false)] {
        let Some(id) = id else { continue };
        let cands = match candidate_ids(client, org, id, through_cases).await {
            Ok(c) => c,
            Err(AdoError::NotFound) => return (404, format!("Azure DevOps has no work item #{id}")),
            Err(e) => return ado_failure(&e),
        };
        let found = match client.get_bugs_by_ids(org, &bug_type, &cands).await {
            Ok((b, _)) => b.into_iter().map(|b| b.id).collect::<Vec<_>>(),
            Err(e) => return ado_failure(&e),
        };
        // A PBI and a case together: the bugs on both.
        scope = Some(match scope {
            None => found,
            Some(prev) => prev.into_iter().filter(|i| found.contains(i)).collect(),
        });
    }

    let mut not_on_pbi: Vec<i32> = vec![];
    let wanted: Vec<i32> = match (&scope, &ids) {
        (Some(s), Some(ids)) => {
            not_on_pbi = ids.iter().copied().filter(|i| !s.contains(i)).collect();
            ids.iter().copied().filter(|i| s.contains(i)).collect()
        }
        (Some(s), None) => s.clone(),
        (None, Some(ids)) => ids.clone(),
        (None, None) => vec![],
    };
    let (mut found, not_a_bug) = match client.get_bugs_by_ids(org, &bug_type, &wanted).await {
        Ok(r) => r,
        Err(AdoError::NotFound) => {
            return (
                404,
                format!(
                    "Azure DevOps has no work item for at least one of {} - check the ids",
                    wanted.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
                ),
            )
        }
        Err(e) => return ado_failure(&e),
    };
    if ids.is_some() {
        found.sort_by_key(|b| wanted.iter().position(|&i| i == b.id).unwrap_or(usize::MAX));
    } else {
        found.sort_by(|a, b| b.id.cmp(&a.id));
    }
    let mut out = serde_json::json!({
        "bugs": found.iter().skip(offset).take(page).map(|b| bug_json(b, titles_only)).collect::<Vec<_>>(),
        "total": found.len(),
    });
    if !not_a_bug.is_empty() {
        out["not_a_bug"] = serde_json::json!(not_a_bug);
    }
    if !not_on_pbi.is_empty() {
        out["not_on_pbi"] = serde_json::json!(not_on_pbi);
    }
    (200, out.to_string())
}
