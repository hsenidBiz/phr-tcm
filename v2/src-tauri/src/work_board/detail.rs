//! Work-item detail loading, comments and avatars.

use super::{InlineImage, WorkComment, WorkItemDetail};
use crate::ado::{AdoClient, AdoError};

impl AdoClient {
    /// One work item, fully loaded for the detail drawer. Read only.
    pub async fn get_work_item_detail(
        &self,
        org: &str,
        project: &str,
        id: i32,
    ) -> Result<WorkItemDetail, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url, org, project, id
        );
        let data = self.get_json(url).await?;
        let f = &data["fields"];
        let s = |key: &str| f[key].as_str().unwrap_or_default().to_string();
        let wi_type = s("System.WorkItemType");
        let description_field = if wi_type == "Bug" && f["Microsoft.VSTS.TCM.ReproSteps"].is_string()
        {
            "Microsoft.VSTS.TCM.ReproSteps"
        } else {
            "System.Description"
        };
        // The process can put extra form pages on a type (Bug: RCA,
        // Preventive Measures). Discover them from the type's layout so
        // every field on those tabs is shown, whatever the org calls them.
        // A failed lookup means no extra tabs, with the reason surfaced.
        let (extra_pages, extra_pages_error) =
            match self.extra_pages_for(org, project, &wi_type, f).await {
                Ok(pages) => (pages, None),
                Err(e) => (Vec::new(), Some(e)),
            };
        // Attachment images need auth the WebView can't send. Field values
        // stay byte-faithful (edits must round-trip the URLs, not megabytes
        // of base64) - the preview applies this url -> data-uri map instead.
        let mut rich_htmls: Vec<&str> = vec![];
        let desc_html = s(description_field);
        rich_htmls.push(&desc_html);
        for page in &extra_pages {
            for field in &page.fields {
                if field.kind == "html" {
                    rich_htmls.push(&field.value);
                }
            }
        }
        let inline_images = self.collect_attachment_images(&rich_htmls).await;
        Ok(WorkItemDetail {
            id,
            title: s("System.Title"),
            state: s("System.State"),
            assigned_to: f["System.AssignedTo"]["displayName"].as_str().unwrap_or_default().to_string(),
            assigned_to_unique: f["System.AssignedTo"]["uniqueName"].as_str().unwrap_or_default().to_string(),
            activity: s("Microsoft.VSTS.Common.Activity"),
            tags: s("System.Tags"),
            area_path: s("System.AreaPath"),
            iteration_path: s("System.IterationPath"),
            remaining_work: f["Microsoft.VSTS.Scheduling.RemainingWork"].as_f64(),
            completed_work: f["Microsoft.VSTS.Scheduling.CompletedWork"].as_f64(),
            original_estimate: f["Microsoft.VSTS.Scheduling.OriginalEstimate"].as_f64(),
            start_date: s("Microsoft.VSTS.Scheduling.StartDate"),
            finish_date: s("Microsoft.VSTS.Scheduling.FinishDate"),
            description_text: crate::steps_xml::html_to_text(&desc_html),
            description_html: desc_html,
            description_field: description_field.to_string(),
            inline_images,
            work_item_type: wi_type,
            extra_pages,
            extra_pages_error,
        })
    }

    /// A work item's comments, newest first, ported from v1
    /// get_work_item_comments incl. the avatar fallback chain
    /// (_links.avatar.href -> imageUrl -> empty = initials disc). Read only.
    pub async fn get_work_item_comments(
        &self,
        org: &str,
        project: &str,
        wi_id: i32,
    ) -> Result<Vec<WorkComment>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workItems/{}/comments?order=desc&api-version=7.1-preview.4",
            self.base_url, org, project, wi_id
        );
        let data = self.get_json(url).await?;
        Ok(data["comments"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|c| {
                let cb = &c["createdBy"];
                let avatar = cb["_links"]["avatar"]["href"]
                    .as_str()
                    .or_else(|| cb["imageUrl"].as_str())
                    .unwrap_or_default()
                    .to_string();
                WorkComment {
                    id: c["id"].as_i64().unwrap_or_default() as i32,
                    text: crate::steps_xml::html_to_text(c["text"].as_str().unwrap_or_default()),
                    created_by: cb["displayName"].as_str().unwrap_or_default().to_string(),
                    created_date: c["createdDate"].as_str().unwrap_or_default().to_string(),
                    avatar_url: avatar,
                }
            })
            .collect())
    }

    /// POST a comment; no DELETE.
    pub async fn add_work_item_comment(
        &self,
        org: &str,
        project: &str,
        wi_id: i32,
        text: &str,
    ) -> Result<(), AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workItems/{}/comments?api-version=7.1-preview.4",
            self.base_url, org, project, wi_id
        );
        self.post_json(url, &serde_json::json!({"text": text})).await?;
        Ok(())
    }

    /// Download the attachment images referenced by rich-text HTML (a plain
    /// <img> gets 401 - the WebView sends no bearer header) and return
    /// url -> data-uri pairs for the preview to apply. Best-effort per
    /// image; caps guard pathological fields.
    pub async fn collect_attachment_images(&self, htmls: &[&str]) -> Vec<InlineImage> {
        use base64::Engine;
        let re = regex::Regex::new(r#"src=["']([^"']+)["']"#).unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for html in htmls {
            if !html.contains("<img") {
                continue;
            }
            for cap in re.captures_iter(html) {
                if out.len() >= 12 {
                    return out;
                }
                // The src sits in an HTML attribute, so & is entity-encoded.
                let url = cap[1].replace("&amp;", "&");
                let Some(download) = attachment_download_url(&url, &self.base_url) else {
                    continue;
                };
                if !seen.insert(url.clone()) {
                    continue;
                }
                // Through the transport, like every other request: paced,
                // and with a log line, which is what "log every request"
                // has to mean for the one the user is looking for. Every
                // failure here used to be a bare `continue`, so a picture
                // that would not load left no trace anywhere - the field
                // just showed a broken image and the log had nothing to
                // say about it.
                let bytes = match self.get_bytes(download).await {
                    Ok(b) => b,
                    Err(e) => {
                        crate::applog::warn(format!("inline image {url} could not be fetched: {e}"));
                        continue;
                    }
                };
                // The bytes carry their own type; ADO serves these as
                // octet-stream, so sniff rather than trust a header we no
                // longer see. PNG and GIF are unambiguous, JPEG starts FFD8.
                let mime = match bytes.as_slice() {
                    [0x89, b'P', b'N', b'G', ..] => "image/png",
                    [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
                    [b'G', b'I', b'F', ..] => "image/gif",
                    _ => "image/png",
                }
                .to_string();
                if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
                    crate::applog::warn(format!(
                        "inline image {url} skipped at {} bytes",
                        bytes.len()
                    ));
                    continue; // keep the broken link rather than a 10MB blob
                }
                out.push(InlineImage {
                    url,
                    data: format!(
                        "data:{};base64,{}",
                        mime,
                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                    ),
                });
            }
        }
        out
    }

    /// Fetch an avatar as base64 PNG-ish bytes. Best-effort like v1
    /// get_avatar_image: any problem -> None so the UI falls back to
    /// initials. Handles the Graph endpoint's base64-JSON body variant.
    pub async fn get_avatar_b64(&self, url: &str) -> Option<String> {
        use base64::Engine;
        // The attachment path has checked this since the leak found while
        // widening its filter. Avatars were fetched exactly the same way -
        // bearer token attached to whatever URL arrived over IPC - and
        // never did, so anything that could reach the command could name
        // the host the token went to.
        if !token_may_be_sent_to(url, &self.base_url) {
            crate::applog::warn(format!("refused to send the token to {url} for an avatar"));
            return None;
        }
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .header("Accept", "image/png,image/*;q=0.8")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let is_json = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .map(|c| c.contains("application/json"))
            .unwrap_or(false);
        let bytes = resp.bytes().await.ok()?;
        if bytes.is_empty() {
            return None;
        }
        if is_json {
            let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            return v["value"].as_str().map(String::from);
        }
        Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
    }
}

/// The scheme+host of a URL, lower-cased, or None if it has neither.
fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    // Userinfo makes a URL read as one host and connect to another
    // ("https://dev.azure.com@evil.example/"). Nothing legitimate here has
    // it, so refuse outright rather than rely on which half we happen to
    // keep.
    if host.is_empty() || host.contains('@') {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Whether this app may attach the user's Azure DevOps bearer token to a
/// request for `url`.
///
/// The token is the whole of the user's access, and the rule is that it
/// never leaves the app. URLs here arrive from OUTSIDE - out of a work
/// item's HTML, out of an identity record, out of whatever the frontend
/// passes over IPC - and none of that is a reason to send it somewhere
/// new. So: the host this client is already talking to (which covers an
/// on-premises server), or Microsoft's own Azure DevOps domains.
pub fn token_may_be_sent_to(url: &str, base_url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    host_of(base_url).is_some_and(|b| b == host)
        || host == "dev.azure.com"
        // vssps./vsrm./vstmr. - avatars and test results live on these.
        || host.ends_with(".dev.azure.com")
        || host.ends_with(".visualstudio.com")
}

/// The URL to download an `<img src>` from with the user's token, or None
/// if this app has no business fetching it.
///
/// TWO checks, and the host one is the important half. A work item's HTML
/// is attacker-controlled - anyone who can edit the item chooses what the
/// src says - so a rule that looked only at the PATH would happily send an
/// Azure DevOps bearer token to `https://evil.example/_apis/wit/attachments/x`.
/// The token only ever goes to the host this client is already talking to,
/// or to Microsoft's own Azure DevOps domains.
///
/// The path check is deliberately broader than "work item attachment":
/// a screenshot pasted into a bug from a failed test run is a TEST RESULT
/// attachment (`/_apis/test/Runs/.../attachments/...`), which is exactly
/// the case that showed up as a permanently broken image - it was never
/// recognised as an attachment at all, so nothing was ever fetched.
///
/// Azure DevOps also embeds these without an api-version - the browser
/// gets one from its session, a bare request does not, and the service can
/// answer 400 rather than the bytes.
pub fn attachment_download_url(src: &str, base_url: &str) -> Option<String> {
    if !token_may_be_sent_to(src, base_url) {
        return None;
    }
    let path = src.to_ascii_lowercase();
    if !path.contains("/_apis/") || !path.contains("attachment") {
        return None;
    }
    if path.contains("api-version=") {
        return Some(src.to_string());
    }
    let sep = if src.contains('?') { '&' } else { '?' };
    Some(format!("{src}{sep}api-version=7.1"))
}
