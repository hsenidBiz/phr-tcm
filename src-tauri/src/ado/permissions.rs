//! Azure DevOps permission questions, asked through
//! `POST _apis/security/permissionevaluationbatch`.
//!
//! Two callers with opposite postures, on purpose:
//! - deleting test cases (deletion.rs) hides its button on anything short
//!   of an explicit yes: a delete that cannot work is a promise broken
//!   after the click;
//! - creating a test suite hides its button only on an explicit NO: the
//!   create itself refuses safely with a message, so an unanswerable
//!   question must not take the button away from someone entitled to it.
//!
//! Both ask with `alwaysAllowAdministrators: false` - the literal ACL
//! answer - and both ask about a SPECIFIC area node, because area
//! permissions are per node and inheritance is exactly what an org
//! overrides.

use super::{AdoClient, AdoError};

/// Azure DevOps' PROJECT security namespace, which is where work-item
/// delete lives.
///
/// The first version of the delete gate used the Classification-node (area
/// path) namespace and its bit 8. Those are real and internally
/// consistent, so Azure DevOps resolved them and answered confidently -
/// about a different question: "may this user delete this AREA PATH NODE".
/// A default Contributor holds project-level "Delete and restore work
/// items" but not "Delete this node", so they got a clean `false` and
/// never saw the button.
///
/// NOT VERIFIED against a live organization - this machine cannot reach
/// one. And note what that first version got wrong in its REASONING, not
/// just its constants: it claimed a wrong constant could only ever cost a
/// missing button. That holds for a constant Azure DevOps cannot resolve.
/// A wrong-but-valid one gets a confident yes or no about the wrong thing,
/// and `evaluate` cannot tell the difference. So the permission check is a
/// courtesy that hides a button nobody could use - the real backstop for
/// delete is the 403 handling on the delete itself, which needs no
/// constant to be right.
pub(crate) const PROJECT_NAMESPACE_ID: &str = "52d39943-cb85-4d7f-8fa8-c6baac873819";

/// WORK_ITEM_DELETE in the PROJECT namespace. Pinned by a test that reads
/// the request body, so changing it is a deliberate act rather than a typo.
pub(crate) const WORK_ITEM_DELETE: u32 = 8192;

/// The CSS (area path) namespace, where Azure DevOps keeps the
/// test-artifact permissions.
///
/// Round two of the wrong-question lesson above, caught in the field this
/// time: WORK_ITEM_DELETE alone is the permission for ORDINARY work items,
/// and a default Contributor holds it. Test Cases are test artifacts, and
/// Microsoft's docs gate deleting those on the area-level "Manage test
/// plans" / "Manage test suites" permissions instead - so the old check
/// answered yes for users Azure DevOps would refuse, and they met the
/// refusal only after clicking a button that looked like a promise. The
/// delete gate now requires BOTH: the work-item delete right and a
/// manage-test right on the project's root area.
pub(crate) const CSS_NAMESPACE_ID: &str = "83e28ad4-2d72-4ceb-97b0-c7726d5502c3";

/// MANAGE_TEST_PLANS / MANAGE_TEST_SUITES in the CSS namespace. Either
/// suffices for the delete gate - the docs name them as alternatives.
/// Creating a static suite is gated on MANAGE_TEST_SUITES specifically.
pub(crate) const MANAGE_TEST_PLANS: u32 = 64;
pub(crate) const MANAGE_TEST_SUITES: u32 = 128;

/// Ask Azure DevOps to evaluate a batch of permission questions and read
/// back one answer per question, in order.
///
/// `None` in a slot means that evaluation was missing from the response or
/// was not a bool - NOT a no. Callers decide for themselves what an
/// unreadable answer means for their button (see the module docs: the two
/// current callers disagree on purpose).
pub(crate) async fn evaluate(
    client: &AdoClient,
    org: &str,
    evaluations: Vec<serde_json::Value>,
) -> Result<Vec<Option<bool>>, AdoError> {
    let body = serde_json::json!({
        "evaluations": evaluations,
        // FALSE on purpose: this asks Azure DevOps for the literal ACL
        // answer. `true` tells it to pass anyone in an Administrators
        // group whatever their ACL says - which is the one input in this
        // request that can bias it toward yes. Both callers of this
        // function want the literal answer, whichever way they then lean
        // on an uncertain one.
        "alwaysAllowAdministrators": false,
    });
    let answer = client
        .post_json(
            format!(
                "{}/{}/_apis/security/permissionevaluationbatch?api-version=7.1",
                client.base_url, org
            ),
            &body,
        )
        .await?;
    Ok(match answer["evaluations"].as_array() {
        Some(evals) => evals.iter().map(|e| e["value"].as_bool()).collect(),
        None => Vec::new(),
    })
}

impl AdoClient {
    /// Whether this user may create or change test suites under this area.
    ///
    /// `Some(false)` is a clear no from Azure DevOps; `None` means the
    /// question could not be asked, which is NOT a no - see the module
    /// docs. `area_path` is the plan's own area, since a root-level answer
    /// does not hold per node.
    pub async fn may_manage_test_suites(
        &self,
        org: &str,
        project: &str,
        area_path: Option<&str>,
    ) -> Option<bool> {
        match self.evaluate_manage_test_suites(org, project, area_path).await {
            Ok(values) => values.first().copied().flatten(),
            Err(e) => {
                crate::applog::warn(format!(
                    "could not establish create-suite permission for {project}, leaving the button in place: {e}"
                ));
                None
            }
        }
    }

    async fn evaluate_manage_test_suites(
        &self,
        org: &str,
        project: &str,
        area_path: Option<&str>,
    ) -> Result<Vec<Option<bool>>, AdoError> {
        let area_token = self.area_node_token(org, project, area_path).await?;
        evaluate(
            self,
            org,
            vec![serde_json::json!({
                "securityNamespaceId": CSS_NAMESPACE_ID,
                "token": area_token,
                "permissions": MANAGE_TEST_SUITES,
            })],
        )
        .await
    }

    /// The security token for a given area node: the given area, or the
    /// project's root when none is given. An area path arrives shaped
    /// "Project\Team\Component"; the classificationnodes URL wants the
    /// segments AFTER the project, '/'-joined and encoded one by one.
    pub(super) async fn area_node_token(
        &self,
        org: &str,
        project: &str,
        area_path: Option<&str>,
    ) -> Result<String, AdoError> {
        let node_url = match area_path {
            Some(path) if !path.trim().is_empty() => {
                let tail: Vec<String> = path
                    .split('\\')
                    .skip(1)
                    .map(|seg| urlencoding::encode(seg).into_owned())
                    .collect();
                if tail.is_empty() {
                    format!(
                        "{}/{}/{}/_apis/wit/classificationnodes/areas?api-version=7.1",
                        self.base_url, org, project
                    )
                } else {
                    format!(
                        "{}/{}/{}/_apis/wit/classificationnodes/areas/{}?api-version=7.1",
                        self.base_url,
                        org,
                        project,
                        tail.join("/")
                    )
                }
            }
            _ => format!(
                "{}/{}/{}/_apis/wit/classificationnodes/areas?api-version=7.1",
                self.base_url, org, project
            ),
        };
        let areas = self.get_json(node_url).await?;
        let Some(area_id) = areas["identifier"].as_str() else {
            return Err(AdoError::Http {
                status: 0,
                body: "the area node carried no identifier to build a security token from".into(),
            });
        };
        Ok(format!("vstfs:///Classification/Node/{area_id}"))
    }
}
