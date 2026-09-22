//! Every key the Rust cache holds, and how long each kind stays fresh -
//! in one file so two features can never collide on a key without it
//! being visible here.
//!
//! Each key starts with its kind (`tags:`, `suite:` ...) so kinds cannot
//! overlap whatever an org or project happens to be called.

use std::time::Duration;

/// Tags change when someone types a new one - slow enough that a stale
/// read is harmless, fast enough that a week would annoy.
pub const TAGS_TTL_MS: u64 = 6 * 60 * 60 * 1000;

/// How long the AI bridge reuses a scanned plan tree. A project can hold
/// hundreds of plans and the scan is one request per plan; an assistant
/// that lists suites and then reads three of them must not pay for the
/// scan three times.
pub const SUITE_TREE_TTL: Duration = Duration::from_secs(10 * 60);

/// A project's tag names, shared by the UI and the AI bridge.
pub fn tags(org: &str, project: &str) -> String {
    format!("tags:{org}/{project}")
}

/// Work-item ids already announced as newly assigned (assigned_watch.rs).
pub fn assigned_seen(org: &str, project: &str) -> String {
    format!("assigned-seen:{org}/{project}")
}

/// A PBI's resolved requirement suite, shared by the upload, Run Tests and
/// the AI bridge. The client's base_url is in the key so parallel tests on
/// different mock servers cannot poison each other.
pub fn suite(base_url: &str, org: &str, project: &str, pbi_id: i32) -> String {
    format!("suite:{base_url}|{org}|{project}|{pbi_id}")
}

/// The AI bridge's scanned plans-and-suites tree (session tier only).
pub fn suite_tree(base_url: &str, org: &str, project: &str) -> String {
    format!("suite-tree:{base_url}|{org}|{project}")
}

/// How long the Boards fallback reuses the ids its URL needs. A project's
/// GUID never changes and a team's area scope changes when somebody
/// reorganises the project, not during an upload - and the fallback only
/// runs at all after the documented API has already refused, so paying for
/// these lookups twice in one session is pure delay.
pub const BOARDS_IDS_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// The project's GUID and default team id, for the Boards route's URL
/// (session tier only). The base_url is in the key for the same reason as
/// `suite`: parallel tests on different mock servers must not collide.
pub fn project_id(base_url: &str, org: &str, project: &str) -> String {
    format!("project-id:{base_url}|{org}|{project}")
}

/// The team whose area covers one area path, for the Boards route's
/// `teamId` (session tier only). Keyed by the area, not the PBI: two PBIs
/// in the same area share the answer.
pub fn area_team(base_url: &str, org: &str, project: &str, area: &str) -> String {
    format!("area-team:{base_url}|{org}|{project}|{area}")
}

/// A fetched wiki page, for the review page's spec pane. Ten minutes: the
/// keep-in-step refresh re-renders the page on every focus, and the wiki
/// does not move that fast.
pub const WIKI_PAGE_TTL_MS: u64 = 10 * 60 * 1000;

/// One wiki page's content, keyed by the URL the file names.
pub fn wiki_page(url: &str) -> String {
    format!("wiki-page:{url}")
}
