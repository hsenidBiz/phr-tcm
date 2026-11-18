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
/// A PBI and a case together give the bugs on both.
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
        Some(v) => match parse_case_ids(&v, "bug") {
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
    let info = match client.detect_bug_type(org, &ctx.project).await {
        Ok(t) => t,
        Err(e) => return ado_failure(&e),
    };

    // The bugs the PBI / case defines, if either was named. Links can lead
    // anywhere, so an item that cannot be read is left out, not fatal.
    let mut scope: Option<Vec<BugSummary>> = None;
    for (id, through_cases) in [(pbi, true), (case, false)] {
        let Some(id) = id else { continue };
        let cands = match candidate_ids(client, org, id, through_cases).await {
            Ok(c) => c,
            Err(AdoError::NotFound) => return (404, format!("Azure DevOps has no work item #{id}")),
            Err(e) => return ado_failure(&e),
        };
        let found = match client.get_bugs_by_ids(org, &info, &cands, true).await {
            Ok((b, _)) => b,
            Err(e) => return ado_failure(&e),
        };
        scope = Some(match scope {
            None => found,
            Some(prev) => prev.into_iter().filter(|b| found.iter().any(|f| f.id == b.id)).collect(),
        });
    }

    let not_found = |wanted: &[i32]| {
        (
            404,
            format!(
                "Azure DevOps has no work item for at least one of {} - check the ids",
                wanted.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
            ),
        )
    };
    let mut not_a_bug: Vec<i32> = vec![];
    let mut not_on_pbi: Vec<i32> = vec![];
    let mut found: Vec<BugSummary> = match (scope, &ids) {
        (Some(s), None) => s,
        (None, Some(ids)) => match client.get_bugs_by_ids(org, &info, ids, false).await {
            Ok((mut b, nab)) => {
                not_a_bug = nab;
                b.sort_by_key(|x| ids.iter().position(|&i| i == x.id).unwrap_or(usize::MAX));
                b
            }
            Err(AdoError::NotFound) => return not_found(ids),
            Err(e) => return ado_failure(&e),
        },
        (Some(s), Some(ids)) => {
            let (inside, outside): (Vec<i32>, Vec<i32>) =
                ids.iter().copied().partition(|i| s.iter().any(|b| b.id == *i));
            // An id off the scope is a bug elsewhere (not_on_pbi) or not a bug at all.
            match client.get_bugs_by_ids(org, &info, &outside, false).await {
                Ok((elsewhere, nab)) => {
                    not_a_bug = nab;
                    not_on_pbi = outside.iter().copied().filter(|i| elsewhere.iter().any(|b| b.id == *i)).collect();
                }
                Err(AdoError::NotFound) => return not_found(&outside),
                Err(e) => return ado_failure(&e),
            }
            inside.iter().filter_map(|i| s.iter().find(|b| b.id == *i).cloned()).collect()
        }
        (None, None) => vec![],
    };
    if ids.is_none() {
        found.sort_by(|a, b| b.id.cmp(&a.id));
    }
    let total = found.len();
    let mut shown: Vec<BugSummary> = found.into_iter().skip(offset).take(page).collect();
    if !titles_only {
        if let Err(e) = client.sort_bug_links(org, &info, &mut shown).await {
            return ado_failure(&e);
        }
    }
    let mut out = serde_json::json!({
        "bugs": shown.iter().map(|b| bug_json(b, titles_only)).collect::<Vec<_>>(),
        "total": total,
        "offset": offset,
    });
    if !not_a_bug.is_empty() {
        out["not_a_bug"] = serde_json::json!(not_a_bug);
    }
    if !not_on_pbi.is_empty() {
        out["not_on_pbi"] = serde_json::json!(not_on_pbi);
    }
    (200, out.to_string())
}
