//! Sending a reviewed run to Azure DevOps as one test run - the ONLY door
//! out of Auto Run's own data. Reached only from `commands::autorun_publish`,
//! reached only when a person presses Send after reviewing every verdict.
//! GET, POST and PATCH only - no DELETE, and no plan or suite is ever
//! created here (that already happened, in Run Tests, before this runs).

use super::{store, CaseRecord, PublishedRun};
use crate::ado::{AdoClient, AdoError};
use crate::ado_testplan::EnsuredSuite;
use crate::commands::runs::{record_point_outcome, PointOutcome, RunAttachment};
use std::path::Path;

/// What the screen knows about a case that the run file does not: the
/// case's real Azure DevOps step ids, in document order.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct PublishCase {
    pub case_id: i32,
    pub step_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct SkippedCase {
    pub case_id: i32,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct PublishReport {
    pub run_id: i32,
    pub web_url: String,
    /// Case ids whose outcome was recorded.
    pub sent: Vec<i32>,
    /// Never attempted, and why.
    pub skipped: Vec<SkippedCase>,
    /// Attempted and not (fully) done, in words.
    pub problems: Vec<String>,
}

/// A refusal is an answer, not an error: nothing was sent and this is why.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PublishResult {
    Sent(PublishReport),
    Refused { why: String },
}

/// Pictures are evidence for a failure, not an archive - a case with more
/// than this many keeps only the first, in the order `pictures_for` builds
/// them (failures first, then the last step).
pub const MAX_PICTURES_PER_CASE: usize = 5;

/// The comment a result gets: the person's own note first (if any), then
/// what the machine proposed and why. The note is never the part that gets
/// cut when the whole is too long - `update_run_results` caps a comment at
/// 1000 characters, but relying on that would let it truncate mid-note.
pub fn comment_for(case: &CaseRecord) -> String {
    let account_part = match &case.account {
        Some(a) if !a.is_empty() => format!(" as {a}"),
        _ => String::new(),
    };
    let lead = format!("Auto Run{account_part}: ");
    let reason = if case.reason.is_empty() {
        "no reason recorded"
    } else {
        case.reason.as_str()
    };

    let note = case.note.trim();
    let prefix = if note.is_empty() {
        String::new()
    } else {
        format!("{note}\n\n")
    };

    let full = format!("{prefix}{lead}{reason}");
    if full.chars().count() <= 1000 {
        return full;
    }

    // Too long: the reason is what gets cut, never the person's note.
    let head_len = (prefix.chars().count() + lead.chars().count()).min(1000);
    let reason_budget = 1000 - head_len;
    let cut_reason: String = reason.chars().take(reason_budget).collect();
    format!("{prefix}{lead}{cut_reason}").chars().take(1000).collect()
}

/// One mark per Azure DevOps step id, in order. Entry `i` looks at the
/// case's own step `i + 1` (the sign-in step, 0, is never marked): `None`
/// when there is no such step or every one of its outcomes was skipped
/// ("not run:" - an earlier step of the case already failed), `Some`
/// ("Failed") when any outcome that DID run was not ok, otherwise
/// `Some("Passed")`. A script with more steps than the case has ids: the
/// extra steps are ignored, because there is no id to mark them against.
pub fn step_marks(case: &CaseRecord, step_ids: &[String]) -> Vec<Option<String>> {
    step_ids
        .iter()
        .enumerate()
        .map(|(i, _)| mark_for_step(case, i as i32 + 1))
        .collect()
}

fn mark_for_step(case: &CaseRecord, step_number: i32) -> Option<String> {
    let step = case.steps.iter().find(|s| s.step_number == step_number)?;
    let ran: Vec<bool> = step
        .outcomes
        .iter()
        .filter(|o| !o.detail.starts_with("not run:"))
        .map(|o| o.ok)
        .collect();
    if ran.is_empty() {
        return None;
    }
    if ran.iter().any(|ok| !ok) {
        Some("Failed".to_string())
    } else {
        Some("Passed".to_string())
    }
}

/// Every failed action's screenshot, then the step-end picture of the last
/// executed step; duplicates by name removed (the same picture can be both
/// a failure shot and a step-end shot), first [`MAX_PICTURES_PER_CASE`]
/// kept. Returns `(step_number, shot name)` pairs.
pub fn pictures_for(case: &CaseRecord) -> Vec<(i32, String)> {
    let mut out: Vec<(i32, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for step in &case.steps {
        for oc in &step.outcomes {
            if !oc.ok {
                if let Some(name) = &oc.screenshot {
                    if seen.insert(name.clone()) {
                        out.push((step.step_number, name.clone()));
                    }
                }
            }
        }
    }

    if let Some(last) = case
        .steps
        .iter()
        .filter(|s| s.outcomes.iter().any(|o| !o.detail.starts_with("not run:")))
        .max_by_key(|s| s.step_number)
    {
        if let Some(name) = &last.screenshot {
            if seen.insert(name.clone()) {
                out.push((last.step_number, name.clone()));
            }
        }
    }

    out.truncate(MAX_PICTURES_PER_CASE);
    out
}

/// Send a reviewed run to Azure DevOps as one test run. Only cases with a
/// confirmed `verdict` (`Passed`, `Failed` or `Blocked`) are sent - a
/// proposal alone is never enough. See the rules in the task brief for the
/// full contract; the short version: everything before `create_test_run`
/// can refuse or fail outright, and nothing after it can, because a run
/// that exists in Azure DevOps and is not remembered here would be sent
/// twice.
pub async fn publish_run(
    client: &AdoClient,
    root: &Path,
    organization: &str,
    project: &str,
    suite: &EnsuredSuite,
    run_name: &str,
    run_id: &str,
    cases: &[PublishCase],
) -> Result<PublishResult, AdoError> {
    let refused = |why: String| Ok(PublishResult::Refused { why });
    let Ok(Some(mut run)) = store::load_run(root, run_id) else {
        return refused("this run is no longer on this machine".into());
    };
    if let Some(p) = &run.published {
        return refused(format!("this run was already sent to Azure DevOps: {}", p.web_url));
    }
    let confirmed: Vec<&CaseRecord> = run
        .cases
        .iter()
        .filter(|c| matches!(c.verdict.as_str(), "Passed" | "Failed" | "Blocked"))
        .collect();
    let mut skipped: Vec<SkippedCase> = run
        .cases
        .iter()
        .filter(|c| !confirmed.iter().any(|k| k.case_id == c.case_id))
        .map(|c| SkippedCase { case_id: c.case_id, why: "no verdict was confirmed".into() })
        .collect();
    if confirmed.is_empty() {
        return refused("confirm at least one verdict before sending".into());
    }

    let ids: Vec<i32> = confirmed.iter().map(|c| c.case_id).collect();
    let points = client.get_test_points(organization, project, suite.plan_id, suite.suite_id, &ids).await?;
    let points_of = |case_id: i32| -> Vec<i32> {
        points.iter().filter(|p| p.test_case_id == Some(case_id)).map(|p| p.point_id).collect()
    };
    for c in &confirmed {
        if points_of(c.case_id).is_empty() {
            skipped.push(SkippedCase { case_id: c.case_id, why: "it is not in this PBI's test suite".into() });
        }
    }
    let sendable: Vec<&CaseRecord> = confirmed.iter().copied().filter(|c| !points_of(c.case_id).is_empty()).collect();
    if sendable.is_empty() {
        return refused("none of these cases is in the PBI's test suite in Azure DevOps".into());
    }
    let point_ids: Vec<i32> = sendable.iter().flat_map(|c| points_of(c.case_id)).collect();

    // From here on a run exists in Azure DevOps. Nothing below may return Err.
    let created = client.create_test_run(organization, project, suite.plan_id, run_name, &point_ids).await?;
    let mut problems: Vec<String> = Vec::new();
    let mut sent: Vec<i32> = Vec::new();
    let results = match client.get_run_results(organization, project, created.run_id).await {
        Ok(r) => r,
        Err(e) => {
            crate::applog::warn(format!("auto-run publish: run {} results could not be read: {e}", created.run_id));
            problems.push("the new run's result rows could not be read, so no outcome was recorded in it".into());
            vec![]
        }
    };
    use base64::Engine;
    for case in &sendable {
        let id = case.case_id;
        let step_ids = cases.iter().find(|c| c.case_id == id).map(|c| c.step_ids.clone()).unwrap_or_default();
        let marks = step_marks(case, &step_ids);

        let mut attachments = Vec::new();
        let mut per_step: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
        for (step, name) in pictures_for(case) {
            match store::load_shot(root, &name) {
                Ok(bytes) => {
                    let n = per_step.entry(step).or_insert(0);
                    *n += 1;
                    let file_name = if *n == 1 {
                        format!("case-{id}-step-{step}.jpg")
                    } else {
                        format!("case-{id}-step-{step}-{n}.jpg")
                    };
                    attachments.push(RunAttachment { file_name, b64: base64::engine::general_purpose::STANDARD.encode(bytes) });
                }
                Err(_) => problems.push(format!("case {id}: the picture of step {step} is no longer on this machine")),
            }
        }

        let mut every_point_recorded = true;
        for point_id in points_of(id) {
            let Some(result_id) = results.iter().find(|r| r.point_id == Some(point_id)).map(|r| r.result_id) else {
                every_point_recorded = false;
                problems.push(format!("case {id}: Azure DevOps made no result row for test point {point_id}"));
                continue;
            };
            let outcome = PointOutcome {
                point_id,
                outcome: case.verdict.clone(),
                comment: Some(comment_for(case)),
                duration_ms: case.duration_ms,
                step_ids: Some(step_ids.clone()),
                step_outcomes: Some(marks.clone()),
                attachments: Some(attachments.clone()),
                bug_ids: None,
            };
            match record_point_outcome(client, organization, project, created.run_id, result_id, &outcome).await {
                Ok(extras) => problems.extend(extras.into_iter().map(|x| format!("case {id}: {x} was not saved"))),
                Err(e) => {
                    crate::applog::warn(format!("auto-run publish: run {} case {id} was not recorded: {e}", created.run_id));
                    every_point_recorded = false;
                    problems.push(format!("case {id}: its outcome was not recorded"));
                }
            }
        }
        if every_point_recorded {
            sent.push(id);
        }
    }

    if let Err(e) = client.complete_test_run(organization, project, created.run_id).await {
        crate::applog::warn(format!("auto-run publish: run {} could not be completed: {e}", created.run_id));
        problems.push("the run was left In Progress in Azure DevOps - complete it there".into());
    }

    run.published = Some(PublishedRun {
        run_id: created.run_id,
        web_url: created.web_url.clone(),
        at: super::sessions::now_ms().to_string(),
    });
    if let Err(e) = store::save_run(root, &run) {
        crate::applog::warn(format!("auto-run publish: run {} could not be marked as sent: {e}", created.run_id));
        problems.push(format!(
            "this machine could not record that the run was sent - do not send it again: {}",
            created.web_url
        ));
    }

    skipped.sort_by_key(|s| s.case_id);
    Ok(PublishResult::Sent(PublishReport { run_id: created.run_id, web_url: created.web_url, sent, skipped, problems }))
}
