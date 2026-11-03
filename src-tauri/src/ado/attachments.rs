//! Reading step attachments off Azure DevOps and uploading step files.
//!
//! GET and POST only. Each `AttachedFile` relation whose comment starts with
//! `[TestStep=<id>]:` belongs to that step; relations without the marker are
//! case-level attachments and are never shown on a step.

use std::collections::HashMap;

use super::{AdoClient, AdoError, TestCaseFull};
use crate::step_attachments::{
    attachment_guid, parse_marker, CaseRelation, CaseRelations, StepAttachment, Uploaded,
    STEP_ATTACHMENT_MAX_BYTES,
};
use crate::steps_xml::Step;

impl AdoClient {
    /// The `AttachedFile` relations (with their position in the full
    /// `relations` array) and revision of each of `ids`. Cases the server
    /// omits are absent from the map.
    pub async fn case_relations(
        &self,
        organization: &str,
        ids: &[i32],
    ) -> Result<HashMap<i32, CaseRelations>, AdoError> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&$expand=relations&errorPolicy=omit&api-version=7.1",
                self.base_url, organization, ids_csv
            );
            let fetched = self.get_json(url).await?;
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                let Some(id) = w["id"].as_i64() else { continue };
                let mut attachments = Vec::new();
                for (index, r) in w["relations"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                    if r["rel"].as_str() != Some("AttachedFile") {
                        continue;
                    }
                    let Some(guid) = attachment_guid(r["url"].as_str().unwrap_or("")) else { continue };
                    let a = &r["attributes"];
                    attachments.push(CaseRelation {
                        index,
                        guid,
                        name: a["name"].as_str().unwrap_or_default().to_string(),
                        size: a["resourceSize"].as_u64().and_then(|n| u32::try_from(n).ok()),
                        comment: a["comment"].as_str().unwrap_or_default().to_string(),
                    });
                }
                out.insert(id as i32, CaseRelations { rev: w["rev"].as_i64().unwrap_or_default(), attachments });
            }
        }
        Ok(out)
    }

    /// Put each case's step attachments on their steps. On a read failure the
    /// error is logged and `attachments_read` stays false, so a later save
    /// never mistakes "not read" for "none".
    pub async fn load_step_attachments(&self, organization: &str, cases: &mut [TestCaseFull]) {
        let ids: Vec<i32> = cases.iter().map(|c| c.id).collect();
        if ids.is_empty() {
            return;
        }
        match self.case_relations(organization, &ids).await {
            Ok(rel) => fill_step_attachments(cases, &rel),
            Err(e) => crate::applog::warn(&format!("could not read step attachments: {e}")),
        }
    }

    /// Upload every distinct local file named by a step attachment's `path`,
    /// once each. The first failure returns its sentence.
    pub async fn upload_step_files(
        &self,
        organization: &str,
        project: &str,
        steps: &[Step],
    ) -> Result<Vec<Uploaded>, String> {
        let mut done: Vec<Uploaded> = Vec::new();
        for att in steps.iter().flat_map(|s| s.attachments.iter()) {
            let Some(path) = att.path.as_deref() else { continue };
            if done.iter().any(|u| u.path == path) {
                continue;
            }
            let (name, bytes) = read_step_file(path)?;
            let url = match self.upload_wi_attachment(organization, project, &name, bytes).await {
                Ok(u) if !u.is_empty() => u,
                Ok(_) => {
                    crate::applog::warn("attachment upload returned no url");
                    return Err(format!("\"{name}\" could not be uploaded to Azure DevOps."));
                }
                Err(e) => {
                    crate::applog::warn(&format!("attachment upload failed for \"{name}\": {e}"));
                    return Err(format!("\"{name}\" could not be uploaded: {}", e.user_text()));
                }
            };
            let Some(guid) = attachment_guid(&url) else {
                return Err(format!("\"{name}\" could not be uploaded to Azure DevOps."));
            };
            done.push(Uploaded { path: path.to_string(), guid, url, name });
        }
        Ok(done)
    }
}

/// Place each relation on the step whose id its marker names. A relation
/// with no marker, or one for a step the case no longer has, is not shown.
pub fn fill_step_attachments(cases: &mut [TestCaseFull], rel: &HashMap<i32, CaseRelations>) {
    for case in cases.iter_mut() {
        let Some(r) = rel.get(&case.id) else { continue };
        for a in &r.attachments {
            let Some((step_id, comment)) = parse_marker(&a.comment) else { continue };
            let Some(j) = case.step_ids.iter().position(|s| *s == step_id) else { continue };
            let Some(step) = case.steps.get_mut(j) else { continue };
            step.attachments.push(StepAttachment {
                id: Some(a.guid.clone()),
                path: None,
                name: a.name.clone(),
                size: a.size,
                comment,
            });
        }
        case.attachments_read = true;
    }
}

/// Read a local file for upload: a plain local path, an existing file, no
/// larger than the cap. Returns the file name and its bytes. The sentences
/// name the file and never a URL.
pub fn read_step_file(path: &str) -> Result<(String, Vec<u8>), String> {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "that file".to_string());
    if !crate::test_files::is_plain_local_path(path) {
        return Err(format!("\"{name}\" is not on a local drive, so it cannot be attached."));
    }
    let meta = std::fs::metadata(path)
        .map_err(|_| format!("\"{name}\" could not be found, so it was not attached."))?;
    if !meta.is_file() {
        return Err(format!("\"{name}\" is not a file, so it cannot be attached."));
    }
    if meta.len() > STEP_ATTACHMENT_MAX_BYTES {
        return Err(format!(
            "\"{name}\" is larger than {} MB, so it cannot be attached.",
            STEP_ATTACHMENT_MAX_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        crate::applog::warn(&format!("could not read step file \"{name}\": {e}"));
        format!("\"{name}\" could not be read, so it was not attached.")
    })?;
    Ok((name, bytes))
}
