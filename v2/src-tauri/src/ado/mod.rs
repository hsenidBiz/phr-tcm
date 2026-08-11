//! Azure DevOps REST client.
//!
//! SAFETY INVARIANT: this client exposes GET (and later POST/PATCH) only.
//! Never add a DELETE method — enforced by tests/ado.rs, which scans every
//! file of this module (and the other `impl AdoClient` extensions).
//!
//! Layout: this file owns the client type, its constructors and the wire
//! DTOs; `transport` owns the HTTP verbs + error mapping; `endpoints` owns
//! the work-item-tracking API calls. Test plans and the Work Manager extend
//! the same client from `ado_testplan` / `work_board`.

mod endpoints;
/// The single, audited exception to the no-DELETE rule. See its header.
pub mod deletion;
pub mod throttle;
mod transport;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Org {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PbiHit {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestCaseSummary {
    pub id: i32,
    pub title: String,
    pub tags: String,
    pub automation_status: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FieldRef {
    pub name: String,
    pub reference_name: String,
}

/// One wiki-search hit: enough to let the AI pick a page, then call
/// `get_wiki_page` with `wiki_id` + `path` for the full content.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WikiHit {
    pub file_name: String,
    pub path: String,
    pub wiki_name: String,
    pub wiki_id: String,
    /// Joined highlight fragments from all matched fields (may be empty).
    pub highlights: String,
}

/// A wiki page's full content, fetched after a `WikiHit` narrows the path.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WikiPage {
    pub path: String,
    pub content: String,
}

/// A fully-loaded Test Case for the editor: steps parsed from the XML blob,
/// preconditions flattened to plain text. `id` doubles as update_id when the
/// editor saves.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TestCaseFull {
    pub id: i32,
    pub title: String,
    pub tags: String,
    pub automation_status: String,
    pub steps: Vec<crate::steps_xml::Step>,
    /// Real ADO step ids (document order, aligned with `steps`) - the runner
    /// needs them to build iterationDetails.
    pub step_ids: Vec<String>,
    /// The Steps field EXACTLY as Azure DevOps holds it.
    ///
    /// `steps` above is a lossy read: parse_steps_xml strips every tag, so
    /// bold, links and embedded screenshots do not survive it. Writing that
    /// back would delete them from the work item. Keeping the original
    /// lets a save ask "did the user actually change the steps?" and, when
    /// the answer is no, leave the field out of the patch entirely.
    pub steps_xml: String,
    pub module_value: String,
    pub preconditions: String,
}

/// What a blank value means on an update.
///
/// The two callers genuinely disagree, and conflating them was a bug in both
/// directions. An IMPORT must never erase: a blank column in a file is the
/// absence of an opinion, and letting it through would wipe data the file
/// never mentioned. A FORM the user emptied is the opposite: they deleted
/// the tags on purpose, and skipping the write left the old value in Azure
/// DevOps while the app said "Updated".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlankPolicy {
    /// Imports: a blank leaves whatever Azure DevOps already has.
    Skip,
    /// Editor and Bulk Edit: a blank clears the field.
    Clear,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BugTypeInfo {
    pub wi_type: String,
    pub repro_field: String,
    pub has_severity: bool,
}

// Clone so a failed operation can be REPORTED as well as returned -
// DeleteOutcome carries the real error per item instead of a flattened
// string, and that struct is Clone.
#[derive(Debug, Clone, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "kind", content = "detail")]
pub enum AdoError {
    #[error("unauthorized")]
    Unauthorized,
    // u32 (not u64): specta forbids BigInt-style types crossing IPC.
    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u32 },
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("http {status}")]
    Http { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
}

pub(crate) fn tc_ids_i32(ids: &[i64]) -> Vec<i32> {
    ids.iter().map(|i| *i as i32).collect()
}

pub struct AdoClient {
    pub(crate) http: reqwest::Client,
    pub(crate) token: String,
    pub(crate) base_url: String, // "https://dev.azure.com" in prod, mock server in tests
    pub(crate) vssps_base_url: String, // "https://app.vssps.visualstudio.com" in prod
}

impl AdoClient {
    pub fn new(access_token: String) -> Self {
        Self::with_base_urls(
            access_token,
            "https://dev.azure.com".to_string(),
            "https://app.vssps.visualstudio.com".to_string(),
        )
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        let vssps = base_url.clone();
        Self::with_base_urls(access_token, base_url, vssps)
    }

    pub fn with_base_urls(access_token: String, base_url: String, vssps_base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token: access_token,
            base_url,
            vssps_base_url,
        }
    }
}
