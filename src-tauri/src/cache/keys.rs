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

/// How long the AI bridge reuses a database lookup's answer. A schema
/// changes when a deploy script runs, not while an assistant explores it,
/// and a lookup costs up to three sqlcmd runs - so asking the same thing
/// twice in ten minutes costs nothing the second time.
pub const DB_LOOKUP_TTL: Duration = Duration::from_secs(10 * 60);

/// One database lookup's rendered answer (session tier only). The server
/// and database are in the key: switching the AI Bridge tab to another
/// connection must never answer from the last one.
pub fn db_lookup(server: &str, database: &str, limit: usize, query: &str) -> String {
    format!("db-lookup:{server}|{database}|{limit}|{query}")
}

/// A fetched wiki page, for the review page's spec pane. Ten minutes: the
/// keep-in-step refresh re-renders the page on every focus, and the wiki
/// does not move that fast.
pub const WIKI_PAGE_TTL_MS: u64 = 10 * 60 * 1000;

/// Every wiki-page key starts with this; `cache::put` evicts by it.
pub const WIKI_PAGE_PREFIX: &str = "wiki-page:";

/// A wiki page not fetched for a week is dropped from the cache file.
pub const WIKI_PAGE_KEEP_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// At most this many wiki pages are kept; the oldest go first.
pub const WIKI_PAGE_MAX: usize = 50;

/// One wiki page's content, keyed by the URL the file names.
pub fn wiki_page(url: &str) -> String {
    format!("{WIKI_PAGE_PREFIX}{url}")
}

/// How long the signed-in identity is reused: in effect the session. An
/// identity id never changes under one sign-in, and signing in as someone
/// else clears the session tier anyway (`claim_for`).
pub const CONNECTED_USER_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The signed-in identity for one organization (session tier only). The
/// base_url is in the key so parallel tests on different mock servers
/// cannot answer for each other.
pub fn connected_user(base_url: &str, org: &str) -> String {
    format!("connected-user:{base_url}|{org}")
}

/// How long what a case's setup gave is kept for its supervised steps: in
/// effect the session. A case started again makes a fresh draft anyway.
pub const SETUP_OUTPUTS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// What case `case_id`'s setup gave at its last supervised start or replay
/// to a step (session tier only, `autorun::setup::remember`).
pub fn setup_outputs(case_id: i32) -> String {
    format!("setup-outputs:{case_id}")
}

/// How long a refused suite create is remembered for an area. "Manage test
/// suites" and the Test Plans access level change when an administrator
/// grants them, which is rare - but it does happen, so after a week the
/// documented route is asked again.
pub const SUITE_REFUSED_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// An area whose documented suite create answered 403, so the upload goes
/// straight to the Boards route (uploads only - Run Tests always asks).
/// The value is just `true`; the entry's own stamp is what the TTL is
/// measured against (`cache::fresh`). Keyed by the plan's
/// area, lower-cased with backslashes - the permission lives on the area,
/// not on the plan or the PBI. The base_url is in the key for the same
/// reason as `suite`.
pub fn suite_refused(base_url: &str, org: &str, project: &str, area: &str) -> String {
    format!("suite-refused:{base_url}|{org}|{project}|{area}")
}

/// How long the Playwright tab reuses a clone's `playwright test --list`
/// output, kept with the clone's fingerprint (durable tier, and the session
/// tier so the disk copy is read once). Listing compiles every spec, so the
/// tree is not listed again for each run or after a restart; a changed
/// fingerprint, the Runner's Refresh, or a run's dry count catches a stale listing.
pub const PW_LIST_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// A kept listing this size (bytes, serialized) or larger stays in memory
/// only: the durable file is written whole on every change.
pub const PW_LIST_PERSIST_MAX: usize = 1024 * 1024;

/// Every listing key starts with this; the durable tier keeps one
/// (`cache::evict_pw_lists`).
pub const PW_LIST_PREFIX: &str = "pw-list:";

/// How long one Playwright report's stats are kept (session tier only),
/// checked against the report file's modified time on every read.
pub const PW_REPORT_STATS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// One Playwright report's stats, by the path of its `<runId>.json` and its
/// size (a rewrite that keeps the modified time rarely keeps the size too).
pub fn pw_report_stats(file: &str, size: u64) -> String {
    format!("pw-report-stats:{file}|{size}")
}

/// The Playwright tab's last listing of the clone at `clone`.
pub fn pw_list(clone: &str) -> String {
    format!("{PW_LIST_PREFIX}{clone}")
}

/// How long the cases an export wrote this run are remembered for Finish in
/// the repo (session tier only): in effect the session.
pub const PW_WRITTEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The case ids exports wrote to the clone at `clone` this run
/// (`commands::pw_export::remember_written`): a case Finish in the repo may
/// build a prompt for even when its Sync record could not be saved.
pub fn pw_written(clone: &str) -> String {
    format!("pw-written:{clone}")
}

/// How long the capture sets imported for the assistant are kept (session
/// tier only): twelve hours, a working day with room to spare, renewed by
/// each import. They also go when the person signs in or out of TCM, and
/// with everything else when a different account signs in (`claim_for`).
pub const IMPORTED_CAPTURES_TTL: Duration = Duration::from_secs(12 * 60 * 60);

/// The capture sets imported from HAR files with `import_autorun_requests`,
/// by name (session tier only, `commands::autorun::imported_captures`): at
/// most `capture_tools::MAX_IMPORTED_SETS`, read-only, readable with no
/// browser open.
pub fn imported_captures() -> String {
    "imported-captures".to_string()
}
