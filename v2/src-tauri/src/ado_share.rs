//! Share-a-draft-for-review: the queue JSON travels through Azure DevOps
//! itself, so there is no server, no public exposure, and access control
//! is the PBI's own permissions.
//!
//! Share = upload the queue JSON as an attachment blob, then add an
//! `AttachedFile` relation on the PBI so the blob is durable (unreferenced
//! blobs can be garbage-collected) and visible on the PBI's Attachments
//! tab in ADO. Open = fetch the blob with the RECIPIENT's own sign-in.
//!
//! Share links are ONE-TIME USE (owner directive): the fetch first checks
//! the AttachedFile relation still exists (gone = "already used"), and
//! after a successful import it removes that relation - which is also how
//! the blob dies, since ADO has no attachment-delete API: an unreferenced
//! blob disappears from the PBI immediately and is garbage-collected
//! server-side. During that GC window the blob technically still answers
//! raw API calls, but both ends of this feature are this app, and the app
//! refuses links whose relation is gone.
//!
//! ── INVARIANT CHANGE, owner-approved 2026-07-26 ────────────────────────
//! "The PBI is never modified" held from v1 until here. The owner
//! explicitly approved EXACTLY TWO writes, both scoped to the share file:
//! adding the AttachedFile relation, and removing THAT SAME relation on
//! first import (guarded by a json-patch `test` on /rev so a concurrent
//! change can never make the remove hit the wrong relation). Nothing else
//! about the PBI may change - no fields, no other relation types, and
//! still no HTTP DELETE anywhere (scanned by tests/ado.rs).
//! ────────────────────────────────────────────────────────────────────────

use crate::ado::{AdoClient, AdoError};

/// One pasteable string. Carries only ids - useless without org access.
/// `tcm-share:{org}/{project}/{pbi_id}/{attachment_guid}`
const LINK_PREFIX: &str = "tcm-share:";

#[derive(Debug, Clone, PartialEq)]
pub struct ShareRef {
    pub org: String,
    pub project: String,
    pub pbi_id: i32,
    pub attachment_id: String,
}

/// Parses a share link, tolerating surrounding whitespace (links arrive
/// via chat apps, which love to add it).
pub fn parse_share_link(link: &str) -> Result<ShareRef, String> {
    let rest = link
        .trim()
        .strip_prefix(LINK_PREFIX)
        .ok_or_else(|| format!("not a share link - it should start with {LINK_PREFIX}"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() != 4 {
        return Err("malformed share link".into());
    }
    let pbi_id: i32 = parts[2].parse().map_err(|_| "malformed share link".to_string())?;
    // GUID-shaped only: the id goes into a URL path.
    let id = parts[3];
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("malformed share link".into());
    }
    Ok(ShareRef {
        org: parts[0].to_string(),
        project: parts[1].to_string(),
        pbi_id,
        attachment_id: id.to_string(),
    })
}

pub fn build_share_link(r: &ShareRef) -> String {
    format!(
        "{LINK_PREFIX}{}/{}/{}/{}",
        r.org, r.project, r.pbi_id, r.attachment_id
    )
}

impl AdoClient {
    /// Uploads the draft JSON and attaches it to the PBI. Returns the
    /// pasteable share link. The attachment is named so a human browsing
    /// the PBI in ADO understands what it is.
    pub async fn share_draft(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        json: &str,
    ) -> Result<String, AdoError> {
        let file_name = format!("tcm-draft-review-{pbi_id}.json");
        let upload_url = format!(
            "{}/{}/{}/_apis/wit/attachments?fileName={}&api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(&file_name)
        );
        let created = self.post_octet(upload_url, json.to_string()).await?;
        let id = created["id"].as_str().unwrap_or_default().to_string();
        let url = created["url"].as_str().unwrap_or_default().to_string();
        if id.is_empty() || url.is_empty() {
            return Err(AdoError::Network("attachment upload returned no id/url".into()));
        }

        // The one approved PBI write: add the AttachedFile relation so the
        // blob is durable and visible. Additive only.
        let patch_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, org, project, pbi_id
        );
        let patch = serde_json::json!([{
            "op": "add",
            "path": "/relations/-",
            "value": {
                "rel": "AttachedFile",
                "url": url,
                "attributes": { "comment": "Test Case Manager draft shared for review" }
            }
        }]);
        self.send_json_patch(reqwest::Method::PATCH, patch_url, &patch)
            .await?;

        Ok(build_share_link(&ShareRef {
            org: org.to_string(),
            project: project.to_string(),
            pbi_id,
            attachment_id: id,
        }))
    }

    /// Downloads a shared draft's JSON with the CALLER's own sign-in - ADO
    /// enforces their access to the project. Read only.
    pub async fn fetch_shared_draft(&self, share: &ShareRef) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/attachments/{}?api-version=7.1",
            self.base_url, share.org, share.project, share.attachment_id
        );
        self.get_text(url).await
    }

    /// One-time-use consumption: check the share relation is still live,
    /// download the draft, then revoke the relation. Returns the JSON plus
    /// an optional warning when the revoke could not be performed (e.g.
    /// the recipient lacks edit permission on the PBI) - the import still
    /// succeeded, the link just stays live.
    pub async fn take_shared_draft(
        &self,
        share: &ShareRef,
    ) -> Result<(String, Option<String>), String> {
        // 1) Liveness: the relation IS the link's validity.
        let wi_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, share.org, share.project, share.pbi_id
        );
        let wi = self.get_json(wi_url).await.map_err(|e| e.to_string())?;
        let rev = wi["rev"].as_i64().unwrap_or(0);
        let relations = wi["relations"].as_array().cloned().unwrap_or_default();
        let idx = relations.iter().position(|r| {
            r["rel"].as_str() == Some("AttachedFile")
                && r["url"]
                    .as_str()
                    .is_some_and(|u| u.contains(&share.attachment_id))
        });
        let Some(idx) = idx else {
            return Err(
                "This share link has already been used, or was revoked by the sender.".into(),
            );
        };

        // 2) Download before revoking - a failed download must not burn
        //    the link.
        let json = self.fetch_shared_draft(share).await.map_err(|e| e.to_string())?;

        // 3) Revoke: remove EXACTLY the relation found above. The `test`
        //    op on /rev aborts the patch if the work item changed since
        //    the read, so a shifted relations array can never make the
        //    remove hit a different relation.
        let patch_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, share.org, share.project, share.pbi_id
        );
        let patch = serde_json::json!([
            { "op": "test", "path": "/rev", "value": rev },
            { "op": "remove", "path": format!("/relations/{idx}") }
        ]);
        let warning = match self
            .send_json_patch(reqwest::Method::PATCH, patch_url, &patch)
            .await
        {
            Ok(_) => None,
            Err(e) => Some(format!(
                "Imported, but the share link could not be revoked (it stays usable): {e}"
            )),
        };
        Ok((json, warning))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_links_round_trip_and_reject_garbage() {
        let r = ShareRef {
            org: "acme".into(),
            project: "Web".into(),
            pbi_id: 144714,
            attachment_id: "aaaa1111-2222-3333-4444-555566667777".into(),
        };
        let link = build_share_link(&r);
        assert_eq!(parse_share_link(&link).unwrap(), r);
        // Chat apps pad links with whitespace.
        assert_eq!(parse_share_link(&format!("  {link}\n")).unwrap(), r);

        for bad in [
            "https://example.com/x",
            "tcm-share:acme/Web/notanumber/aaaa1111",
            "tcm-share:acme/Web/1",
            "tcm-share:acme/Web/1/../../secrets",
        ] {
            assert!(parse_share_link(bad).is_err(), "{bad} should be rejected");
        }
    }
}
