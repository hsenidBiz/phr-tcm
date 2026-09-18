//! The ONLY file in this client permitted to issue a DELETE.
//!
//! Everywhere else the rule still holds absolutely - `tests/ado.rs` scans
//! every other source file and fails the build if `.delete(` or
//! `Method::DELETE` appears in one. This file is the single, deliberate
//! exception, carved out so the thing a reviewer has to audit is one small
//! module rather than a relaxed rule spread across a dozen.
//!
//! ## What this delete IS: permanent, and said so
//!
//! This file used to target the work-item recycle bin, and its old name
//! (recycle.rs) said so. That endpoint categorically refuses test work
//! items - `InvalidDeleteWorkItemCallException`, "Use Test Management REST
//! API to delete test artifacts" - which the field confirmed on
//! 2026-08-11. Azure DevOps offers NO recoverable API deletion for test
//! cases: the Test Management endpoint used here removes the case and its
//! results IRREVERSIBLY. The user authorized that trade explicitly, and
//! the confirm dialog says "permanent" in so many words - the one thing
//! this file must never do is dress this up as recoverable.
//!
//! ## The limits that remain, enforced by test rather than discipline
//!
//! 1. **This endpoint and nothing else.** `DELETE
//!    _apis/test/testcases/{id}` with api-version alone (PLURAL - the
//!    singular form 404s with "controller not found", which the field
//!    found on 2026-08-11 because a how-to doc page had it wrong; the
//!    REST reference is the authority). The work-item
//!    endpoint's permanent-erase query parameter is still banned from this
//!    file BY NAME - including in comments - so the wit route can never
//!    quietly grow back with the worse semantics.
//! 2. **Permission first, and fail closed.** The app asks Azure DevOps
//!    whether this user may manage test artifacts before it offers to.
//!    Anything other than an explicit yes - a failed request, a missing
//!    evaluation, an unexpected shape - is treated as no.

use super::permissions::{
    evaluate, CSS_NAMESPACE_ID, MANAGE_TEST_PLANS, MANAGE_TEST_SUITES, PROJECT_NAMESPACE_ID,
    WORK_ITEM_DELETE,
};
use super::{AdoClient, AdoError};

// The permission constants and the batch-evaluation call now live in
// `permissions.rs`, shared with the create-suite gate - see that module's
// header for what the two namespaces mean, the field failures that pinned
// PROJECT_NAMESPACE_ID / WORK_ITEM_DELETE / CSS_NAMESPACE_ID /
// MANAGE_TEST_PLANS / MANAGE_TEST_SUITES, and why this gate and that one
// lean opposite ways on an uncertain answer.
//
// The re-check asks about the area the caller names (the PBI's, from the
// command) and falls back to the root only when none is known.

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
    /// `area_path` scopes the manage-test half of the question to a
    /// SPECIFIC area node - the one the cases being offered for deletion
    /// actually live under. The field taught us why root is not enough:
    /// the TCM API refused a delete with "no permissions to delete work
    /// items under the specified area path" for a user whose root-area
    /// evaluation passed - area permissions are per node, and inheritance
    /// is exactly what an org overrides. `None` falls back to the root
    /// node (the sign-in-time check, before any PBI is chosen).
    pub async fn can_delete_work_items(
        &self,
        org: &str,
        project: &str,
        area_path: Option<&str>,
    ) -> bool {
        match self.evaluate_delete_permission(org, project, area_path).await {
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
        area_path: Option<&str>,
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

        // The GUID of the area node the question is about - the given
        // area, or the root when none is. No identifier is an uncertain
        // path, and uncertain reads as no.
        let area_token = self.area_node_token(org, project, area_path).await?;

        // Getting this wrong in the `false` (fail-closed) direction costs a
        // missing button; getting it wrong in the `true` direction offers a
        // delete that cannot work. Those are not symmetrical - see
        // `evaluate`'s own comment on `alwaysAllowAdministrators`.
        let values = evaluate(
            self,
            org,
            vec![
                serde_json::json!({
                    "securityNamespaceId": PROJECT_NAMESPACE_ID,
                    "token": format!("$PROJECT:vstfs:///Classification/TeamProject/{project_id}"),
                    "permissions": WORK_ITEM_DELETE,
                }),
                serde_json::json!({
                    "securityNamespaceId": CSS_NAMESPACE_ID,
                    "token": area_token,
                    "permissions": MANAGE_TEST_PLANS,
                }),
                serde_json::json!({
                    "securityNamespaceId": CSS_NAMESPACE_ID,
                    "token": area_token,
                    "permissions": MANAGE_TEST_SUITES,
                }),
            ],
        )
        .await?;

        // Explicitly true, or nothing. A missing or oddly-shaped evaluation
        // is not a yes. Deleting a Test Case needs the work-item right AND
        // a manage-test right (either of the two) - see CSS_NAMESPACE_ID.
        let ev = |i: usize| values.get(i).copied().flatten() == Some(true);
        Ok(ev(0) && (ev(1) || ev(2)))
    }

    /// PERMANENTLY delete test cases through the Test Management API, one
    /// at a time. There is no undo and no bin behind this - see the module
    /// header for why that is the only deletion Azure DevOps offers here.
    ///
    /// One failure never stops the rest - a partly-completed delete is
    /// reported item by item so the caller can say exactly which survived,
    /// rather than leaving the user to guess from a count.
    ///
    /// The permission is re-checked here, not just in the UI: the button
    /// being visible is not authorisation, and the check that hid it may
    /// have been made minutes ago.
    pub async fn delete_test_cases_permanently(
        &self,
        org: &str,
        project: &str,
        area_path: Option<&str>,
        ids: &[i32],
    ) -> Result<Vec<DeleteOutcome>, AdoError> {
        // The same area the button was decided on - area permissions are
        // per node, and a root re-check refused users the UI had offered it to.
        if !self.can_delete_work_items(org, project, area_path).await {
            return Err(AdoError::Forbidden);
        }

        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            // The id is an i32, so the one segment that names WHAT gets
            // deleted cannot carry arbitrary text. `org` and `project` are
            // the same two caller-supplied strings every other endpoint
            // interpolates, and the query carries api-version and nothing
            // else - see this file's header for the wit parameter that is
            // deliberately absent, and the test that keeps it so.
            let url = format!(
                "{}/{}/{}/_apis/test/testcases/{}?api-version=7.1",
                self.base_url, org, project, id
            );
            match self.send_permanent_delete(&url, id).await {
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
    async fn send_permanent_delete(&self, url: &str, id: i32) -> Result<(), AdoError> {
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
                super::transport::network_error(&e)
            })?;
        let status = resp.status().as_u16();
        // The only DELETE keeps its own sender (the transport's verb
        // allow-list stays GET/POST/PATCH), but it honours the server's
        // "slow down" exactly like `transport::send`: Retry-After or
        // X-RateLimit-Delay, on any status.
        if let Some(secs) = super::transport::server_delay(&resp) {
            super::throttle::note_server_delay(secs);
        }
        let retry_after = super::transport::retry_after(&resp);
        // `text()` consumes the response, so it has to happen once, here -
        // not inside one arm of the match below.
        let body = resp.text().await.unwrap_or_default();
        let ms = started.elapsed().as_millis();
        let line = format!("DELETE test case #{id} -> {status} in {ms} ms (permanent, test management api)");
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
