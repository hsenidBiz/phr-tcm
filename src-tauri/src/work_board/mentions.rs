//! Work-item @mentions of the signed-in user, for the notification bell.
//!
//! Azure DevOps keeps the items that mentioned you in the last 30 days
//! behind the `@RecentMentions` WIQL macro. The newest comments of the 20
//! most recently changed of those are read, and each comment that mentions
//! you - written by someone else - becomes a `Mention`. PR mentions are
//! found in the webview, from the PR threads it already reads.

use serde::Serialize;

use super::ConnectedUser;
use crate::ado::{AdoClient, AdoError};
use crate::cache::{self, keys};

/// Items Azure DevOps lists as mentioning you, in this project, most
/// recently changed first. The project clause keeps every result openable
/// in the project the notification names.
pub const RECENT_MENTIONS_WIQL: &str = "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = @project AND [System.Id] IN (@RecentMentions) ORDER BY [System.ChangedDate] DESC";

/// How many mentioned items one check reads comments for.
pub const MENTION_ITEMS: u32 = 20;
/// How many of each item's newest comments are scanned.
pub const MENTION_COMMENTS: u32 = 50;
/// The longest excerpt a notification carries.
pub const EXCERPT_CHARS: usize = 140;

/// One comment that mentions you.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Mention {
    /// Always "work-item" here; PR mentions are found in the webview.
    pub source: String,
    pub item_id: i32,
    /// "Work item" when the item's type could not be read.
    pub item_type: String,
    /// Empty when the item's title could not be read.
    pub item_title: String,
    pub comment_id: i32,
    pub author: String,
    /// The comment as plain text, at most `EXCERPT_CHARS` characters.
    pub excerpt: String,
    /// ISO 8601, as Azure DevOps returns it.
    pub created_date: String,
}

/// Whether a comment's HTML mentions `me`. Azure DevOps writes a mention as
/// an anchor carrying `data-vss-mention="version:2.0,{id}"`; the match
/// ignores case. An empty id matches nothing: every anchor starts with the
/// same prefix, so it would otherwise match every mention of anyone.
pub fn mentions_me(html: &str, me: &str) -> bool {
    let me = me.trim();
    if me.is_empty() {
        return false;
    }
    let needle = format!("data-vss-mention=\"version:2.0,{me}\"").to_lowercase();
    html.to_lowercase().contains(&needle)
}

/// A comment's text for a notification: HTML stripped, whitespace
/// collapsed, at most `EXCERPT_CHARS` characters, the last one an ellipsis
/// when it had to be cut.
pub fn excerpt(html: &str) -> String {
    let flat = crate::steps_xml::html_to_text(html)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if flat.chars().count() <= EXCERPT_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(EXCERPT_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The comments in one comments-API answer that mention `me` and were not
/// written by `me`.
pub fn mentions_in(
    item_id: i32,
    item_type: &str,
    item_title: &str,
    answer: &serde_json::Value,
    me: &str,
) -> Vec<Mention> {
    let me = me.trim();
    answer["comments"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter(|c| c["isDeleted"].as_bool() != Some(true))
        .filter(|c| !c["createdBy"]["id"].as_str().unwrap_or_default().eq_ignore_ascii_case(me))
        .filter(|c| mentions_me(c["text"].as_str().unwrap_or_default(), me))
        .map(|c| Mention {
            source: "work-item".to_string(),
            item_id,
            item_type: item_type.to_string(),
            item_title: item_title.to_string(),
            comment_id: c["id"].as_i64().unwrap_or_default() as i32,
            author: c["createdBy"]["displayName"].as_str().unwrap_or_default().to_string(),
            excerpt: excerpt(c["text"].as_str().unwrap_or_default()),
            created_date: c["createdDate"].as_str().unwrap_or_default().to_string(),
        })
        .collect()
}

impl AdoClient {
    /// `connected_user`, read once per organization per session and kept
    /// in memory only (the session tier of the one cache). Read only.
    pub async fn connected_user_cached(&self, org: &str) -> Result<ConnectedUser, AdoError> {
        let key = keys::connected_user(&self.base_url, org);
        if let Some(user) = cache::session_fresh::<ConnectedUser>(&key, keys::CONNECTED_USER_TTL) {
            return Ok(user);
        }
        let user = self.connected_user(org).await?;
        if !user.id.trim().is_empty() {
            cache::session_put(&key, user.clone());
        }
        Ok(user)
    }

    /// Comments that mention `me` on the items Azure DevOps lists as
    /// recently mentioning them. The query failing fails the check; one
    /// item's comments failing skips that item only. Read only.
    pub async fn recent_mentions(
        &self,
        org: &str,
        project: &str,
        me: &str,
    ) -> Result<Vec<Mention>, AdoError> {
        if me.trim().is_empty() {
            crate::applog::warn("mentions: the signed-in identity has no id, so no mention can be matched");
            return Ok(vec![]);
        }
        let ids = self
            .query_work_items(org, project, RECENT_MENTIONS_WIQL, MENTION_ITEMS)
            .await?;
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let titles = self.read_titles(org, project, &ids).await;
        let mut out = vec![];
        for id in ids {
            let url = format!(
                "{}/{}/{}/_apis/wit/workItems/{}/comments?order=desc&$top={}&api-version=7.1-preview.4",
                self.base_url, org, project, id, MENTION_COMMENTS
            );
            let answer = match self.get_json(url).await {
                Ok(a) => a,
                Err(e) => {
                    crate::applog::warn(format!("mentions: skipped #{id}, its comments could not be read: {e}"));
                    continue;
                }
            };
            let (title, wtype) = titles.get(&id).cloned().unwrap_or_default();
            let wtype = if wtype.is_empty() { "Work item".to_string() } else { wtype };
            out.extend(mentions_in(id, &wtype, &title, &answer, me));
        }
        Ok(out)
    }
}
