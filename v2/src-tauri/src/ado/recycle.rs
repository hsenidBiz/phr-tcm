//! The ONLY file in this client permitted to issue a DELETE.
//!
//! Everywhere else the rule still holds absolutely - `tests/ado.rs` scans
//! every other source file and fails the build if `.delete(` or
//! `Method::DELETE` appears in one. This file is the single, deliberate
//! exception, carved out so the thing a reviewer has to audit is one small
//! module rather than a relaxed rule spread across a dozen.
//!
//! TWO HARD LIMITS, both enforced by test rather than by discipline:
//!
//! 1. **Recycle bin only.** `DELETE _apis/wit/workitems/{id}` moves a work
//!    item to the project's recycle bin, where it can be restored from the
//!    Azure DevOps UI. Azure DevOps also accepts a query parameter on that
//!    same endpoint which erases the item permanently and irrecoverably
//!    instead. This app has no use for it, and `tests/ado.rs` asserts that
//!    parameter's name appears NOWHERE in this file - including in
//!    comments, so the assertion can never be softened by prose explaining
//!    an exception. That is why it is described here and not named.
//! 2. **Permission first, and fail closed.** The app asks Azure DevOps
//!    whether this user may delete before it offers to. Anything other than
//!    an explicit yes - a failed request, a missing evaluation, an
//!    unexpected shape - is treated as no.
//!
//! Both limits exist because a delete is the one thing this tool does that
//! the user cannot undo from inside it.

use super::{AdoClient, AdoError};

/// Azure DevOps' PROJECT security namespace, which is where work-item
/// delete lives.
///
/// The first version of this used the Classification-node (area path)
/// namespace and its bit 8. Those are real and internally consistent, so
/// Azure DevOps resolved them and answered confidently - about a different
/// question: "may this user delete this AREA PATH NODE". A default
/// Contributor holds project-level "Delete and restore work items" but not
/// "Delete this node", so they got a clean `false` and never saw the
/// button.
///
/// NOT VERIFIED against a live organization - this machine cannot reach
/// one. And note what that first version got wrong in its REASONING, not
/// just its constants: it claimed a wrong constant could only ever cost a
/// missing button. That holds for a constant Azure DevOps cannot resolve.
/// A wrong-but-valid one gets a confident yes or no about the wrong thing,
/// and `evaluate_delete_permission` cannot tell the difference. So the
/// permission check is a courtesy that hides a button nobody could use -
/// the real backstop is the 403 handling on the delete itself, which needs
/// no constant to be right.
const PROJECT_NAMESPACE_ID: &str = "52d39943-cb85-4d7f-8fa8-c6baac873819";

/// WORK_ITEM_DELETE in the PROJECT namespace. Pinned by a test that reads
/// the request body, so changing it is a deliberate act rather than a typo.
const WORK_ITEM_DELETE: u32 = 8192;

/// One work item's fate after a delete attempt.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DeleteOutcome {
    pub id: i32,
    pub deleted: bool,
    /// Why not, when it was not. `None` on success.
    ///
    /// The error travels STRUCTURED rather than as a string. It used to be
    /// `e.to_string()`, which for an unmapped status renders as the four
    /// characters "http" plus a number - so a user reporting a failure
    /// could only say "it gives http 400", and the sentence Azure DevOps
    /// sent explaining which rule or constraint refused was read off the
    /// wire and dropped one line later. The frontend's `describeAdoError`
    /// already knows how to lift `message` out of that body; handing it
    /// the real error is what lets it.
    pub error: Option<AdoError>,
}

impl AdoClient {
    /// Whether this user may delete work items in this project.
    ///
    /// Fails closed on EVERY uncertain path. The caller uses this to decide
    /// whether to show the affordance at all, so a false negative is a
    /// missing button and a false positive would be a promise the app
    /// cannot keep - those are not symmetrical, and this leans hard to the
    /// safe side.
    pub async fn can_delete_work_items(&self, org: &str, project: &str) -> bool {
        match self.evaluate_delete_permission(org, project).await {
            Ok(allowed) => allowed,
            Err(e) => {
                crate::applog::warn(format!(
                    "could not establish delete permission for {project}, so it stays disabled: {e}"
                ));
                false
            }
        }
    }

    async fn evaluate_delete_permission(
        &self,
        org: &str,
        project: &str,
    ) -> Result<bool, AdoError> {
        // The permission is held against the PROJECT, so the security token
        // is built from the project's id - which is a GUID, not the name in
        // the URL.
        let meta = self
            .get_json(format!(
                "{}/{}/_apis/projects/{}?api-version=7.1",
                self.base_url, org, project
            ))
            .await?;
        let Some(project_id) = meta["id"].as_str() else {
            return Err(AdoError::Http {
                status: 0,
                body: "the project carried no id to build a security token from".into(),
            });
        };

        let body = serde_json::json!({
            "evaluations": [{
                "securityNamespaceId": PROJECT_NAMESPACE_ID,
                "token": format!("$PROJECT:vstfs:///Classification/TeamProject/{project_id}"),
                "permissions": WORK_ITEM_DELETE,
            }],
            // FALSE on purpose: this asks Azure DevOps for the literal ACL
            // answer. `true` tells it to pass anyone in an Administrators
            // group whatever their ACL says - which is the one input in
            // this request that can bias it toward yes, in a check whose
            // whole stated posture is to fail closed. Getting this wrong in
            // the `false` direction costs a missing button; getting it
            // wrong in the `true` direction offers a delete that cannot
            // work. Those are not symmetrical.
            "alwaysAllowAdministrators": false,
        });
        let answer = self
            .post_json(
                format!(
                    "{}/{}/_apis/security/permissionevaluationbatch?api-version=7.1",
                    self.base_url, org
                ),
                &body,
            )
            .await?;

        // Explicitly true, or nothing. A missing or oddly-shaped evaluation
        // is not a yes.
        Ok(answer["evaluations"][0]["value"].as_bool() == Some(true))
    }

    /// Move work items to the project's recycle bin, one at a time.
    ///
    /// One failure never stops the rest - a partly-completed delete is
    /// reported item by item so the caller can say exactly which survived,
    /// rather than leaving the user to guess from a count.
    ///
    /// The permission is re-checked here, not just in the UI: the button
    /// being visible is not authorisation, and the check that hid it may
    /// have been made minutes ago.
    pub async fn delete_test_cases_to_recycle_bin(
        &self,
        org: &str,
        project: &str,
        ids: &[i32],
    ) -> Result<Vec<DeleteOutcome>, AdoError> {
        if !self.can_delete_work_items(org, project).await {
            return Err(AdoError::Forbidden);
        }

        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            // The id is an i32, so the one segment that names WHAT gets
            // deleted cannot carry arbitrary text. `org` and `project` are
            // caller-supplied strings - a previous version of this comment
            // claimed otherwise, which is worth correcting in the one file
            // whose safety argument is that it is short enough to audit by
            // reading. They are the same two strings every other endpoint
            // interpolates, and the query carries only api-version - the
            // recoverable form. See this file's header for the parameter
            // that is deliberately absent, and the test that keeps it so.
            let url = format!(
                "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
                self.base_url, org, project, id
            );
            match self.send_recycle_delete(&url, id).await {
                Ok(()) => out.push(DeleteOutcome { id, deleted: true, error: None }),
                Err(e) => out.push(DeleteOutcome { id, deleted: false, error: Some(e) }),
            }
        }
        Ok(out)
    }

    /// The request itself. Paced and logged like every other call - a delete
    /// is the LAST thing that should be missing from the log the user pastes
    /// into a bug report.
    ///
    /// The body is read BEFORE the status is matched, and logged with it.
    /// Previously the log line carried the status alone and the body was
    /// only read inside the catch-all arm, so a refusal Azure DevOps had
    /// explained in full arrived as a bare number - in the log and in the
    /// UI both. For the one irreversible thing this app does, "400" with
    /// no sentence is not a diagnosis.
    async fn send_recycle_delete(&self, url: &str, id: i32) -> Result<(), AdoError> {
        super::throttle::pace().await;
        let started = std::time::Instant::now();
        let resp = self
            .http
            .delete(url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                crate::applog::error(format!("delete of #{id} failed to send: {e}"));
                AdoError::Network(e.to_string())
            })?;
        let status = resp.status().as_u16();
        let retry_after = super::transport::retry_after(&resp);
        // `text()` consumes the response, so it has to happen once, here -
        // not inside one arm of the match below.
        let body = resp.text().await.unwrap_or_default();
        let ms = started.elapsed().as_millis();
        let line = format!("DELETE work item #{id} -> {status} in {ms} ms (recycle bin)");
        if (200..=299).contains(&status) {
            crate::applog::info(line);
        } else {
            // Same cap the frontend applies before putting a body in front
            // of a user - enough for Azure DevOps' sentence, not enough for
            // an error page.
            let mut why = body.trim().chars().take(400).collect::<String>();
            if why.is_empty() {
                why = "(no response body)".into();
            }
            crate::applog::warn(format!("{line}: {why}"));
        }
        match status {
            200..=299 => Ok(()),
            401 => Err(AdoError::Unauthorized),
            403 => Err(AdoError::Forbidden),
            404 => Err(AdoError::NotFound),
            429 => Err(AdoError::RateLimited { retry_after_secs: retry_after }),
            s => Err(AdoError::Http { status: s, body }),
        }
    }
}
