//! Azure DevOps REST client.
//!
//! SAFETY INVARIANT: this client exposes GET (and later POST/PATCH) only.
//! Never add a DELETE method — enforced by tests/ado.rs, which scans every
//! source file under src/ except ado/deletion.rs.
//!
//! Layout: this file owns the client type, its constructors and the wire
//! DTOs; `transport` owns the HTTP verbs + error mapping; `endpoints` owns
//! the work-item-tracking API calls. Test plans and the Work Manager extend
//! the same client from `ado_testplan` / `work_board`.

pub mod endpoints;
pub mod wit_batch;
pub use endpoints::{tags_write_ops, RelinkOutcome};
/// The single, audited exception to the no-DELETE rule. See its header.
pub mod deletion;
pub mod permissions;
pub mod throttle;
mod transport;
pub use transport::{network_error, NET_GENERIC, NET_TIMEOUT, NET_UNREACHABLE};
pub(crate) use transport::tidy;

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

impl AdoError {
    /// What to show a person for this error. `Display` of `Http` is the
    /// bare "http 400" - this app's own name for the status - while the
    /// sentence Azure DevOps sent explaining itself sits in `body`. A
    /// failure list built from `to_string()` showed 69 cases refused with
    /// "http 0" and not a word of why (2026-09-22); this is what it should
    /// have shown.
    pub fn user_text(&self) -> String {
        match self {
            AdoError::Http { body, .. } if !body.trim().is_empty() => body.trim().to_string(),
            other => other.to_string(),
        }
    }
}

pub(crate) fn tc_ids_i32(ids: &[i64]) -> Vec<i32> {
    ids.iter().map(|i| *i as i32).collect()
}

/// How long connecting may take before a request gives up.
pub const HTTP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// How long any one request may take, start to last byte.
pub const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// The `$batch` POST alone: up to 200 creates executed server-side in one call.
pub const BATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// The deadline settings every ADO/sign-in HTTP client shares, so the
/// pooled production client and the unpooled loopback-only one below
/// cannot drift apart.
fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(HTTP_TIMEOUT)
}

/// The one HTTP client every Azure DevOps and sign-in request uses.
///
/// reqwest has no timeout by default, so a half-open connection (sleep and
/// resume, a VPN flap, a stalled proxy) used to wait forever - holding the
/// submit claim, or the token refresh every command waits on. One client
/// also keeps the connection pool and TLS sessions, which a client per
/// command threw away.
pub fn http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            client_builder()
                .build()
                .unwrap_or_else(|e| {
                    crate::applog::error(format!("could not build the HTTP client with deadlines: {e}"));
                    // Not `Client::new()`: tests/ado_network.rs scans for it.
                    reqwest::ClientBuilder::new().build().unwrap_or_default()
                })
        })
        .clone()
}

/// True when `url`'s host is loopback (127.0.0.1, localhost, ::1) - the
/// only place a `wiremock::MockServer` ever listens. Parsed with the same
/// `reqwest::Url` reqwest itself will connect with, so a lookalike host
/// (`127.0.0.1.evil.example`) cannot be mistaken for the real thing.
pub fn is_loopback_base(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        // IPv6 hosts come back bracketed ("[::1]"): strip them before
        // comparing, rather than adding a second literal to keep in sync.
        .is_some_and(|h| matches!(h.trim_start_matches('[').trim_end_matches(']'), "127.0.0.1" | "localhost" | "::1"))
}

/// A client for a loopback base URL: the same deadlines as `http_client()`,
/// but no idle-connection pool.
///
/// `http_client()` is one process-wide, pooled client, and every test in a
/// binary shares its keep-alive pool - while each test's `MockServer` shuts
/// down at the end of that test. A pooled idle connection to a now-dead
/// mock can be handed out to a different, still-running test, which then
/// sees a phantom "connection to Azure DevOps failed". Real Azure DevOps
/// and Entra hosts are never loopback, so this path is test-only.
pub(crate) fn unpooled_loopback_client() -> reqwest::Client {
    client_builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap_or_else(|e| {
            crate::applog::error(format!("could not build the loopback HTTP client: {e}"));
            reqwest::ClientBuilder::new().build().unwrap_or_default()
        })
}

/// The right client for a base URL: unpooled on loopback (see
/// `unpooled_loopback_client`), the shared pooled client everywhere else.
fn client_for(base_url: &str) -> reqwest::Client {
    if is_loopback_base(base_url) {
        unpooled_loopback_client()
    } else {
        http_client()
    }
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
            http: client_for(&base_url),
            token: access_token,
            base_url,
            vssps_base_url,
        }
    }
}
