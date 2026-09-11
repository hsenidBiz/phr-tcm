# AI Findings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give AI assistants one place to record issues they notice in a test case, a specification or the code, and give the developer one place in the app (and one section in the browser report) to read, resolve and dismiss them. Keep the AI out of the two human fields: `comment` is the developer's and is never written by an assistant; `reviewer_notes` carries only where a case came from. Also close the gap that hid `transform_cases` operations from every assistant: the tool's description is built from the server's own op table, and a test keeps them in step.

**Architecture:** Findings are local app data, never Azure DevOps: a JSON file under `app_data_dir` owned by a new `findings` module (same shape as the Auto Run store: a process-wide root set at app setup so the AI bridge, which has no `AppHandle`, writes where the UI reads). The bridge gains `POST /findings` and `GET /findings`; the MCP proxy exposes them as `record_finding` and `list_findings`, always on (core tools, no switch, no slash commands: the guide tells the assistant when to use them). Three Tauri commands serve the AI Bridge tab's new card; a `FindingRecorded` event wakes the UI and raises a notification on the bell; the browser report gets an "AI Findings" section. The `set_comment` transform op is removed, the validator advises when a draft's `comment` or `reviewer_notes` carry what belongs in a finding, and the transform description is generated from one shared constant, `transform::SUPPORTED_OPS`.

**Tech Stack:** Rust (Tauri 2, tauri-specta, serde), React 19 + TypeScript, TanStack Query, vitest, Rust integration tests under `src-tauri/tests/`.

## Global Constraints

- All Rust tests are integration tests under `src-tauri/tests/`; never a `#[cfg(test)]` module inside `src/`.
- `src/bindings.ts` is generated: after any command or event change run `cargo test --test bindings` from `src-tauri/` with `$env:CARGO_TARGET_DIR="target/gate"`. Never hand-edit it.
- Run one build or test command at a time on this shared machine. Full gates: `cargo test --tests` (in `src-tauri/`), `npx tsc --noEmit`, `npx vitest run` (repo root).
- No DELETE to Azure DevOps anywhere. Findings live on disk only; removing a finding deletes a local record.
- **The `comment` field on a test case is the developer's.** No tool, transform, optimiser or guide instruction may write it. An assistant reads it only because it round-trips through the file.
- **`reviewer_notes` carries only provenance:** what the case checks, in one or two plain sentences, and where the requirement lives (`Spec:` / `Code:` pointers with quotes). Problems found go to a finding, never into a note.
- Colours only through tokens (`text-text`, `bg-surface`, `bg-surface-2`, `border-border`, `text-accent`, `bg-accent-soft`, `text-muted`, `text-faint`, `text-danger`, `text-success`); `src/ui-consistency.test.ts` fails on a hardcoded colour, an `<Icon...>` without `aria-hidden`, a lucide icon imported straight into a screen or component, or a hand-sized `<Icon size={...}>`. Icons come from `src/lib/actionIcons.ts`.
- The MCP tool list order is asserted verbatim in `src-tauri/tests/tcm_mcp.rs`; the slash-command stems in `src-tauri/tests/ai_tools.rs`; the switch rows in `src/lib/mcpTools.test.ts`; the "N of N on" count in `src/screens/AiBridge.test.tsx`. Update each in the task that changes it. The two finding tools are core: they appear in the tool list but never as a switch row, so the row list and the "5 of 5 on" count do not change.
- Changelog entries are end-user-facing: no file paths, no test names, no process notes.
- Commits use a Bash heredoc `git commit -q -F - <<'EOF' … EOF` ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Writing style inside anything shown to an end user: short sentences, no em dashes.

---

## File map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/findings.rs` (create) | The store: `Finding` type, root, load/save (atomic), `record`, `set_status`, `remove`, `list`, the app-handle slot for the event. |
| `src-tauri/src/lib.rs` (modify) | `pub mod findings;`, root + handle set in setup, commands and event registered. |
| `src-tauri/src/events.rs` (modify) | `FindingRecorded` event. |
| `src-tauri/src/commands/findings.rs` (create) | `list_findings`, `set_finding_status`, `remove_finding` commands. |
| `src-tauri/src/commands/mod.rs` (modify) | `pub mod findings;` |
| `src-tauri/src/backup.rs` (modify) | `findings.json` joins the backup roots. |
| `src-tauri/src/ai_bridge.rs` (modify) | `POST /findings`, `GET /findings`; guide rules on findings, `comment` and `reviewer_notes`; validator advisories. |
| `src-tauri/src/mcp.rs` (modify) | `record_finding`, `list_findings` tools; transform description built from `SUPPORTED_OPS`. |
| `src-tauri/src/ai_tools.rs` (modify) | The two tools join `CORE_TOOLS`. No new commands. |
| `src-tauri/src/transform.rs` (modify) | `set_comment` removed; `pub const SUPPORTED_OPS`; the unknown-op message built from it. |
| `src-tauri/src/import_parser/html.rs` + `commands/queue.rs` (modify) | The browser report's "AI Findings" section. |
| `src-tauri/tests/findings.rs`, `findings_bridge.rs` (create); `transform.rs`, `tcm_mcp.rs`, `ai_tools.rs`, `ai_bridge.rs`, `draft_comments.rs`-adjacent html test (modify) | Assertions that pin the new surface. |
| `src/lib/mcpTools.ts` + test (modify) | Tool entries; both names in `CORE_TOOLS`. |
| `src/lib/notifications.ts` + test (modify) | `ai-finding` kind, `noteFinding`. |
| `src/components/FindingsCard.tsx` + test (create) | The card on the AI Bridge tab. |
| `src/screens/AiBridge.tsx` + test (modify) | Card placed. |
| `src/App.tsx` (modify) | `FindingRecorded` listener → notification + query invalidation. |
| `src/lib/changelog.ts` (modify, at ship time only) | One end-user entry. |

---

### Task 1: The findings store

**Files:**
- Create: `src-tauri/src/findings.rs`
- Modify: `src-tauri/src/lib.rs` (module declaration and setup), `src-tauri/src/events.rs`, `src-tauri/src/backup.rs:23`
- Test: `src-tauri/tests/findings.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const KINDS: [&str; 3] = ["test_case", "spec", "code"];
  pub const STATUSES: [&str; 2] = ["open", "resolved"];
  pub const CAP: usize = 500;
  #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
  pub struct Finding { pub id: String, pub org: String, pub project: String, pub kind: String, pub subject: String, pub title: String, pub detail: String, pub created_at: String, pub status: String }
  pub struct NewFinding { pub org: String, pub project: String, pub kind: String, pub subject: String, pub title: String, pub detail: String }
  pub fn set_root(root: PathBuf); pub fn configured_root() -> Option<PathBuf>;
  pub fn set_app_handle(app: tauri::AppHandle);
  pub fn list(root: &Path, org: &str, project: &str) -> Vec<Finding>;           // newest first
  pub fn record(root: &Path, new: NewFinding) -> Result<Finding, String>;      // validates, prepends, caps, emits
  pub fn set_status(root: &Path, id: &str, status: &str) -> Result<Finding, String>;
  pub fn remove(root: &Path, id: &str) -> Result<(), String>;
  ```

- [ ] **Step 1: Write the failing tests**

`src-tauri/tests/findings.rs`:

```rust
//! The findings store: what an assistant records is on disk, scoped to an
//! org and project, newest first, validated on the way in, and bounded.

use v2_lib::findings::{list, record, remove, set_status, Finding, NewFinding, CAP};

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-findings-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_finding(kind: &str, title: &str) -> NewFinding {
    NewFinding {
        org: "acme".into(),
        project: "Web".into(),
        kind: kind.into(),
        subject: "155170".into(),
        title: title.into(),
        detail: "Step 3 expects a toast the spec never mentions.".into(),
    }
}

#[test]
fn a_recorded_finding_is_listed_newest_first_and_open() {
    let dir = TempDir::new();
    let a = record(dir.path(), new_finding("test_case", "First")).unwrap();
    let b = record(dir.path(), new_finding("spec", "Second")).unwrap();
    assert_ne!(a.id, b.id, "ids are unique within a store");
    assert_eq!(a.status, "open");
    assert!(!a.created_at.is_empty());
    let all = list(dir.path(), "acme", "Web");
    assert_eq!(all.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), vec!["Second", "First"]);
    let raw = std::fs::read_to_string(dir.path().join("findings.json")).unwrap();
    let parsed: Vec<Finding> = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.len(), 2);
}

#[test]
fn listing_is_scoped_to_the_org_and_project() {
    let dir = TempDir::new();
    record(dir.path(), new_finding("code", "Ours")).unwrap();
    record(dir.path(), NewFinding { project: "Mobile".into(), ..new_finding("code", "Theirs") }).unwrap();
    let ours = list(dir.path(), "acme", "Web");
    assert_eq!(ours.len(), 1);
    assert_eq!(ours[0].title, "Ours");
    assert!(list(dir.path(), "acme", "Nope").is_empty());
}

#[test]
fn kind_and_title_are_validated() {
    let dir = TempDir::new();
    let err = record(dir.path(), new_finding("vibes", "x")).unwrap_err();
    assert!(err.contains("test_case, spec or code"), "{err}");
    let err = record(dir.path(), new_finding("spec", "   ")).unwrap_err();
    assert!(err.contains("title"), "{err}");
    assert!(list(dir.path(), "acme", "Web").is_empty(), "nothing invalid is written");
}

#[test]
fn status_changes_and_removal_are_by_id() {
    let dir = TempDir::new();
    let f = record(dir.path(), new_finding("spec", "Ambiguous cut-off")).unwrap();
    let done = set_status(dir.path(), &f.id, "resolved").unwrap();
    assert_eq!(done.status, "resolved");
    assert_eq!(list(dir.path(), "acme", "Web")[0].status, "resolved");
    let err = set_status(dir.path(), &f.id, "sideways").unwrap_err();
    assert!(err.contains("open or resolved"), "{err}");
    assert!(set_status(dir.path(), "no-such-id", "open").is_err());
    remove(dir.path(), &f.id).unwrap();
    assert!(list(dir.path(), "acme", "Web").is_empty());
    assert!(remove(dir.path(), &f.id).is_err(), "removing twice is an error, not silence");
}

#[test]
fn the_store_is_bounded_oldest_dropped_first() {
    let dir = TempDir::new();
    for i in 0..(CAP + 5) {
        record(dir.path(), new_finding("code", &format!("F{i}"))).unwrap();
    }
    let all = list(dir.path(), "acme", "Web");
    assert_eq!(all.len(), CAP);
    assert_eq!(all[0].title, format!("F{}", CAP + 4), "newest kept");
    assert_eq!(all[CAP - 1].title, "F5", "the five oldest are gone");
}

#[test]
fn a_corrupt_file_reads_as_empty_and_is_replaced_on_the_next_write() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("findings.json"), "{ not json").unwrap();
    assert!(list(dir.path(), "acme", "Web").is_empty());
    record(dir.path(), new_finding("spec", "After corruption")).unwrap();
    assert_eq!(list(dir.path(), "acme", "Web").len(), 1);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `src-tauri/`, `$env:CARGO_TARGET_DIR="target/gate"`): `cargo test --test findings`
Expected: compile error, `v2_lib::findings` does not exist.

- [ ] **Step 3: Write the store**

`src-tauri/src/findings.rs`:

```rust
//! AI Findings: the notes an assistant leaves when something it read is
//! wrong - a test case that contradicts its spec, a spec that contradicts
//! itself, code that does not do what either says.
//!
//! Local app data, never Azure DevOps. One JSON file under `app_data_dir`,
//! newest first, bounded. The AI bridge writes through the process-wide
//! root set at app setup (it has no `AppHandle`), the same arrangement as
//! the Auto Run store - two derivations of "where findings live" is how a
//! recorded finding ends up somewhere the AI Bridge tab never looks.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tauri_specta::Event;

pub const KINDS: [&str; 3] = ["test_case", "spec", "code"];
pub const STATUSES: [&str; 2] = ["open", "resolved"];
/// Newest kept; a store that grows without bound is a store nobody reads.
pub const CAP: usize = 500;
const FILE: &str = "findings.json";
const MAX_TEXT: usize = 8_000;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Finding {
    pub id: String,
    pub org: String,
    pub project: String,
    /// One of `KINDS`.
    pub kind: String,
    /// What it is about: a work item id, a spec path and section, a file path.
    pub subject: String,
    pub title: String,
    /// Markdown, rendered in the app and in the browser report.
    pub detail: String,
    /// RFC 3339, UTC.
    pub created_at: String,
    /// One of `STATUSES`.
    pub status: String,
}

pub struct NewFinding {
    pub org: String,
    pub project: String,
    pub kind: String,
    pub subject: String,
    pub title: String,
    pub detail: String,
}

static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Called once during app setup.
pub fn set_root(root: PathBuf) {
    if let Ok(mut slot) = ROOT.lock() {
        *slot = Some(root);
    }
}

/// `None` before setup has run - a caller with no handle must say so
/// rather than guess a path and write where nobody reads.
pub fn configured_root() -> Option<PathBuf> {
    ROOT.lock().ok().and_then(|s| s.clone())
}

/// The handle `record` emits `FindingRecorded` through. Absent in tests
/// and before setup; the write still happens, only the event is skipped.
pub fn set_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

fn file(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Every finding in the store, newest first. A missing or unreadable file
/// is an empty store: the next write replaces it.
fn load_all(root: &Path) -> Vec<Finding> {
    let Ok(raw) = std::fs::read_to_string(file(root)) else {
        return vec![];
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Temp file then rename, so a crash mid-write leaves the old file whole.
fn save_all(root: &Path, all: &[Finding]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = file(root);
    let tmp = root.join(format!("{FILE}.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

pub fn list(root: &Path, org: &str, project: &str) -> Vec<Finding> {
    load_all(root)
        .into_iter()
        .filter(|f| f.org == org && f.project == project)
        .collect()
}

/// Only the open ones - what the card, the bell and the report show first.
pub fn list_open(root: &Path, org: &str, project: &str) -> Vec<Finding> {
    list(root, org, project).into_iter().filter(|f| f.status == "open").collect()
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-date arithmetic (Howard Hinnant's algorithm), so no clock
    // crate is pulled in for one timestamp.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{millis}-{}", N.fetch_add(1, Ordering::SeqCst))
}

fn clip(s: &str) -> String {
    s.trim().chars().take(MAX_TEXT).collect()
}

/// Validate, prepend, cap, write, announce.
pub fn record(root: &Path, new: NewFinding) -> Result<Finding, String> {
    if !KINDS.contains(&new.kind.as_str()) {
        return Err(format!("kind must be one of test_case, spec or code, not \"{}\"", new.kind));
    }
    if new.title.trim().is_empty() {
        return Err("a finding needs a title".into());
    }
    if new.org.trim().is_empty() || new.project.trim().is_empty() {
        return Err("a finding belongs to an organization and project".into());
    }
    let finding = Finding {
        id: new_id(),
        org: new.org,
        project: new.project,
        kind: new.kind,
        subject: clip(&new.subject),
        title: clip(&new.title),
        detail: clip(&new.detail),
        created_at: now_rfc3339(),
        status: "open".into(),
    };
    let mut all = load_all(root);
    all.insert(0, finding.clone());
    all.truncate(CAP);
    save_all(root, &all)?;
    if let Some(app) = APP_HANDLE.get() {
        let _ = crate::events::FindingRecorded {
            id: finding.id.clone(),
            org: finding.org.clone(),
            project: finding.project.clone(),
            kind: finding.kind.clone(),
            title: finding.title.clone(),
        }
        .emit(app);
    }
    Ok(finding)
}

pub fn set_status(root: &Path, id: &str, status: &str) -> Result<Finding, String> {
    if !STATUSES.contains(&status) {
        return Err(format!("status must be open or resolved, not \"{status}\""));
    }
    let mut all = load_all(root);
    let Some(f) = all.iter_mut().find(|f| f.id == id) else {
        return Err(format!("no finding with id {id}"));
    };
    f.status = status.into();
    let out = f.clone();
    save_all(root, &all)?;
    Ok(out)
}

pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    let mut all = load_all(root);
    let before = all.len();
    all.retain(|f| f.id != id);
    if all.len() == before {
        return Err(format!("no finding with id {id}"));
    }
    save_all(root, &all)
}
```

`src-tauri/src/events.rs`, append after `SlowdownRequested`:

```rust
/// Emitted when an AI assistant has recorded a finding through the bridge.
/// The AI Bridge tab refreshes its list and the bell raises a notification.
#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct FindingRecorded {
    pub id: String,
    pub org: String,
    pub project: String,
    pub kind: String,
    pub title: String,
}
```

`src-tauri/src/lib.rs`: add `pub mod findings;` beside the other module declarations; in setup, right after `autorun::store::set_root(dir.join("autorun"));` add

```rust
                // AI Findings: the notes an assistant records. Same
                // arrangement as Auto Run - the bridge has no handle.
                findings::set_root(dir.clone());
                findings::set_app_handle(app.handle().clone());
```

and register `events::FindingRecorded` in the specta events list next to `events::DraftCommentSaved`.

`src-tauri/src/backup.rs`:

```rust
const ROOTS: [&str; 4] = ["reference-cache.json", "autorun", "shared-drafts", "findings.json"];
```

and extend the module comment's list of stores with "AI findings". If a test under `src-tauri/tests/` asserts the roots count or contents, update it to include `findings.json`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test findings` (6 passed), then `cargo test --test bindings` (3 passed; `src/bindings.ts` gains the event).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/findings.rs src-tauri/src/events.rs src-tauri/src/lib.rs src-tauri/src/backup.rs src-tauri/tests/findings.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): findings store - the notes an assistant leaves about a case, spec or code

Local app data under app_data_dir, newest first, capped, atomic writes,
scoped to org and project. Joins the backup. FindingRecorded event.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: Bridge routes and the writing guide

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (route match; new functions near `save_autorun_scripts`; guide text)
- Test: `src-tauri/tests/findings_bridge.rs` (create), `src-tauri/tests/ai_bridge.rs` (guide assertions)

**Interfaces:**
- Consumes: `findings::{record, list, configured_root, set_root, NewFinding}` from Task 1.
- Produces: `POST /findings` body `{kind, subject, title, detail}` → `200 {"id", "open": <count>}`, `400` on validation, `503` when no root; `GET /findings?status=open|resolved|all` (default `open`) → `200 {"findings":[...], "total"}`. Both scoped by `ctx.org` / `ctx.project`. Neither needs a signed-in client.

- [ ] **Step 1: Write the failing tests**

`src-tauri/tests/findings_bridge.rs`:

```rust
//! The two bridge routes behind record_finding and list_findings. Local
//! data only - neither needs a signed-in client.

use v2_lib::ai_bridge::{route, BridgeContext};
use v2_lib::findings::set_root;

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-findings-bridge-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The root is process-wide and cargo runs tests in parallel.
static ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..BridgeContext::default() }
}

#[tokio::test]
async fn a_finding_is_recorded_for_the_open_org_and_project_and_listed_back() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());

    let body = serde_json::json!({
        "kind": "spec",
        "subject": "Step10-ManagePerformanceCycle.md 7.7",
        "title": "AC-3 contradicts the table above it",
        "detail": "The table says **closed**; AC-3 says open."
    })
    .to_string();
    let (status, resp) = route(&ctx(), None, "POST", "/findings", &body, "1.0.0").await;
    assert_eq!(status, 200, "{resp}");
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["id"].as_str().is_some());
    assert_eq!(v["open"], 1);

    let (status, resp) = route(&ctx(), None, "GET", "/findings", "", "1.0.0").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["findings"][0]["kind"], "spec");
    assert_eq!(v["findings"][0]["org"], "acme");
    assert_eq!(v["findings"][0]["status"], "open");

    let other = BridgeContext { project: "Mobile".into(), ..ctx() };
    let (_, resp) = route(&other, None, "GET", "/findings", "", "1.0.0").await;
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["total"], 0, "another project's findings are not this project's");
}

#[tokio::test]
async fn a_bad_finding_is_refused_with_the_reason() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let (status, resp) =
        route(&ctx(), None, "POST", "/findings", r#"{"kind":"vibes","title":"x"}"#, "1.0.0").await;
    assert_eq!(status, 400);
    assert!(resp.contains("test_case, spec or code"), "{resp}");
    let (status, resp) = route(&ctx(), None, "POST", "/findings", "not json", "1.0.0").await;
    assert_eq!(status, 400);
    assert!(resp.contains("kind"), "says what the body should carry: {resp}");
}

#[tokio::test]
async fn listing_filters_by_status() {
    let dir = TempDir::new();
    let _root = ROOT_LOCK.lock().unwrap();
    set_root(dir.path().to_path_buf());
    let body = r#"{"kind":"code","subject":"IndexModel.cs","title":"Null check missing","detail":""}"#;
    let (_, resp) = route(&ctx(), None, "POST", "/findings", body, "1.0.0").await;
    let id = serde_json::from_str::<serde_json::Value>(&resp).unwrap()["id"].as_str().unwrap().to_string();
    v2_lib::findings::set_status(dir.path(), &id, "resolved").unwrap();

    let (_, resp) = route(&ctx(), None, "GET", "/findings", "", "1.0.0").await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&resp).unwrap()["total"], 0, "open by default");
    let (_, resp) = route(&ctx(), None, "GET", "/findings?status=resolved", "", "1.0.0").await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&resp).unwrap()["total"], 1);
    let (_, resp) = route(&ctx(), None, "GET", "/findings?status=all", "", "1.0.0").await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&resp).unwrap()["total"], 1);
}
```

In `src-tauri/tests/ai_bridge.rs`, inside `guide_carries_format_rules_and_live_modules`, after the writing-style block, add:

```rust
    // Problems go to the findings store; the two human fields stay human.
    let findings = body
        .split("## Findings")
        .nth(1)
        .and_then(|rest| rest.split("## reviewer_notes").next())
        .expect("the guide has a findings section");
    assert!(findings.contains("record_finding"), "{findings}");
    assert!(findings.contains("test_case"), "{findings}");
    assert!(findings.contains("never write `comment`"), "the comment field is the developer's: {findings}");
    let notes = body
        .split("## reviewer_notes")
        .nth(1)
        .and_then(|rest| rest.split("## One branch per case").next())
        .unwrap();
    assert!(notes.contains("record_finding"), "a problem in a note is redirected to a finding: {notes}");
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test findings_bridge` (routes unknown), then `cargo test --test ai_bridge guide_carries` (fails on "the guide has a findings section").

- [ ] **Step 3: Add the routes and the guide text**

In the `match (method, path)` of `route`, add before `("GET", "/tools") =>`:

```rust
        // Findings are local app data: no client needed, like the autorun
        // routes - a problem noticed while signed out is still a problem.
        ("POST", "/findings") => record_finding(ctx, body),
        ("GET", "/findings") => list_findings(ctx, target),
```

Add near `save_autorun_scripts`:

```rust
/// The store root as app setup published it; `None` outside the app.
fn findings_root() -> Result<std::path::PathBuf, (u16, String)> {
    crate::findings::configured_root().ok_or((
        503,
        "Test Case Manager has not finished starting - the findings store has no location yet".into(),
    ))
}

/// `record_finding`: one note about one thing, for the open org and project.
fn record_finding(ctx: &BridgeContext, body: &str) -> (u16, String) {
    let root = match findings_root() {
        Ok(r) => r,
        Err(e) => return e,
    };
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                400,
                format!("that is not a finding: {e}. Expected {{ kind: test_case|spec|code, subject, title, detail }}."),
            )
        }
    };
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    match crate::findings::record(
        &root,
        crate::findings::NewFinding {
            org: ctx.org.clone(),
            project: ctx.project.clone(),
            kind: s("kind"),
            subject: s("subject"),
            title: s("title"),
            detail: s("detail"),
        },
    ) {
        Ok(f) => {
            crate::applog::info(format!("AI recorded a {} finding: {}", f.kind, f.title));
            let open = crate::findings::list_open(&root, &ctx.org, &ctx.project).len();
            (200, serde_json::json!({ "id": f.id, "open": open }).to_string())
        }
        Err(e) => (400, e),
    }
}

/// `list_findings`: open by default; `status=resolved` or `status=all`.
fn list_findings(ctx: &BridgeContext, target: &str) -> (u16, String) {
    let root = match findings_root() {
        Ok(r) => r,
        Err(e) => return e,
    };
    let want = q(target, "status").unwrap_or_else(|| "open".into());
    let all = crate::findings::list(&root, &ctx.org, &ctx.project);
    let rows: Vec<&crate::findings::Finding> =
        all.iter().filter(|f| want == "all" || f.status == want).collect();
    (200, serde_json::json!({ "findings": rows, "total": rows.len() }).to_string())
}
```

Guide changes, all inside the `format!` string of `guide`:

1. In `## Format`, replace the clause `and optionally \`comment\` -\n an in-app note that round-trips through the file but is never sent\n to Azure DevOps.` (it spans the lines that read "and optionally `comment` -", "an in-app note that round-trips through the file but is never sent", "to Azure DevOps.") with:
   ```text
   and `comment` - the developer's own note, which round-trips\n\
   through the file and is never sent to Azure DevOps. You never write\n\
   `comment`: leave it exactly as you found it, and never add one.\n\
   ```
2. Insert a new section immediately before `## reviewer_notes\n\`:
   ```text
   ## Findings\n\
   When something you read is WRONG - a case that contradicts its spec,\n\
   a spec that contradicts itself, code that does what neither says -\n\
   call `record_finding` with `kind` (test_case, spec or code), the\n\
   `subject` (the work item id, the spec file and section, or the file\n\
   and member), a one-line `title` and the `detail` in markdown. The\n\
   developer reads findings on the AI Bridge tab and in the browser\n\
   report, and resolves them there. Call `list_findings` first so you do\n\
   not record what is already known. Do this on your own when it applies;\n\
   nobody will ask you to. Never write `comment` for this or for anything\n\
   else, and never put it in reviewer_notes: the first is the developer's\n\
   field, the second says where a case came from and nothing more. Do not\n\
   write a case around a defect as if the defect were the requirement -\n\
   record the finding and say so in the conversation.\n\n\
   ```
3. In `## reviewer_notes`, the "Leave OUT, every time:" list gains one more item, placed before "- Your reasoning, or a decision argued at length.":
   ```text
   - Anything WRONG that you noticed - a contradiction, a gap, a bug.\n\
   That is a finding: call `record_finding` and keep the note to what\n\
   the case checks and where its requirement lives.\n\
   ```
   and the existing item ending `that is a \`comment\`, not this.` becomes `that belongs in the conversation, not here - and never in \`comment\`.`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test findings_bridge` (3 passed), then `cargo test --test ai_bridge` (all passed).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai_bridge.rs src-tauri/tests/findings_bridge.rs src-tauri/tests/ai_bridge.rs
git commit -q -F - <<'EOF'
feat(v2): bridge routes to record and list AI findings; the guide keeps comment and notes human

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: MCP tools (always on), `set_comment` removed, transform description generated

**Files:**
- Modify: `src-tauri/src/transform.rs` (remove the `SetComment` op everywhere; add `pub const SUPPORTED_OPS`; unknown-op message built from it)
- Modify: `src-tauri/src/mcp.rs` (tools_list entries; tools_call arms; transform description)
- Modify: `src-tauri/src/ai_tools.rs` (`CORE_TOOLS` gains the two names; no `CommandSpec`)
- Test: `src-tauri/tests/transform.rs`, `src-tauri/tests/tcm_mcp.rs`, `src-tauri/tests/ai_tools.rs`

**Interfaces:**
- Consumes: routes from Task 2.
- Produces: `transform::SUPPORTED_OPS: [&str; 23]`; MCP tools `record_finding` (args `kind`, `subject`, `title`, `detail`; required `kind`, `title`) and `list_findings` (arg `status`), listed right after `get_run_failures`; both in `CORE_TOOLS`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/tests/transform.rs`, replace the test `replace_in_preconditions_and_set_comment_exist` with:

```rust
/// `comment` is the developer's field. No op writes it, and the op that
/// once did is gone - an assistant asking for it is told so, by name.
#[test]
fn no_op_can_write_the_comment_field() {
    let mut c = noted("A", "n");
    c.preconditions = "Signed in as Appraisee".into();
    c.comment = "Blocked on a decision".into();
    let ops = parse_ops(&serde_json::json!([
        { "op": "replace_in_preconditions", "find": "Appraisee", "replace": "Employee" },
    ]))
    .unwrap();
    let (out, _) = apply(vec![c.clone()], &ops);
    assert_eq!(out[0].preconditions, "Signed in as Employee");
    assert_eq!(out[0].comment, "Blocked on a decision", "the comment is untouched");
    let err = parse_ops(&serde_json::json!([{ "op": "set_comment", "value": "Reviewed" }])).unwrap_err();
    assert!(err.contains("unknown op"), "{err}");
    assert!(err.contains("developer's"), "says why, not just that: {err}");
    let err = parse_ops(&serde_json::json!([{ "op": "replace_in_preconditions", "find": "", "replace": "x" }])).unwrap_err();
    assert!(err.contains("find"), "{err}");
}

/// The op table is one list, used by the server's refusal AND (through
/// mcp.rs) by the tool's description. Three ops were reachable but
/// unlisted in the description; this pins that every name the parser
/// accepts is in the list, and every name in the list is accepted.
#[test]
fn supported_ops_is_the_parsers_whole_vocabulary() {
    use v2_lib::transform::SUPPORTED_OPS;
    assert_eq!(SUPPORTED_OPS.len(), 23);
    assert!(!SUPPORTED_OPS.contains(&"set_comment"));
    for name in SUPPORTED_OPS {
        let extra = match *name {
            "replace_in_title" | "replace_in_steps" | "replace_in_notes" | "replace_in_preconditions" => {
                r#","find":"a","replace":"b""#
            }
            "prepend_step" | "append_step" => r#","action":"Do it","expected":"Done""#,
            "split_step" => r#","find":"a","into":[{"action":"x","expected":"y"}]"#,
            "remove_cases" => r#","where":{"title_contains":"x"}"#,
            "insert_cases" => r#","cases":[{"title":"T","steps":[{"action":"a","expected":"b"}]}]"#,
            "sort_by" | "group_by" => r#","value":"title""#,
            _ => r#","value":"x""#,
        };
        let json = format!(r#"[{{"op":"{name}"{extra}}}]"#);
        let ops: serde_json::Value = serde_json::from_str(&json).unwrap();
        let res = parse_ops(&ops);
        assert!(!matches!(&res, Err(e) if e.contains("unknown op")), "{name}: {res:?}");
    }
    let err = parse_ops(&serde_json::json!([{ "op": "nope" }])).unwrap_err();
    for name in SUPPORTED_OPS {
        assert!(err.contains(name), "the refusal lists {name}: {err}");
    }
}
```

(`parse_ops` is called elsewhere in this file with a `&serde_json::Value`; match that.)

In `src-tauri/tests/tcm_mcp.rs`: insert `"record_finding", "list_findings",` immediately after `"get_run_failures",` in the tool-name list; change the count in `an_unreachable_bridge_disables_nothing` from `15` to `17`; and add:

```rust
/// The description is the only thing an assistant reads. It used to name
/// 21 ops while the server accepted 24; the gap hid replace_in_preconditions
/// and normalise_citations, and set_comment was reachable when it should
/// never have been. Now one list feeds both.
#[test]
fn the_transform_description_names_every_supported_op_and_nothing_else() {
    let resp = handle_message(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "1.10.3",
        &stub(200, ""),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tool = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "transform_cases")
        .unwrap()
        .clone();
    let desc = format!(
        "{} {}",
        tool["description"].as_str().unwrap(),
        tool["inputSchema"]["properties"]["operations"]["description"].as_str().unwrap()
    );
    for name in v2_lib::transform::SUPPORTED_OPS {
        assert!(desc.contains(name), "description omits {name}");
    }
    assert!(!desc.contains("set_comment"), "a removed op must not be advertised");
}

#[test]
fn record_finding_posts_the_body_and_list_findings_passes_status() {
    let calls = std::cell::RefCell::new(vec![]);
    let call = |m: &str, path: &str, body: &str| -> Result<(u16, String), String> {
        calls.borrow_mut().push((m.to_string(), path.to_string(), body.to_string()));
        Ok((200, r#"{"id":"1-0","open":1}"#.into()))
    };
    let req = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"record_finding","arguments":{"kind":"spec","subject":"S 7.7","title":"T","detail":"D"}}}"#;
    handle_message(req, "1.0.0", &call).unwrap();
    let req = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"list_findings","arguments":{"status":"all"}}}"#;
    handle_message(req, "1.0.0", &call).unwrap();
    let c = calls.borrow();
    let post = c.iter().find(|(m, p, _)| m == "POST" && p == "/findings").expect("record posts");
    let body: serde_json::Value = serde_json::from_str(&post.2).unwrap();
    assert_eq!(body["kind"], "spec");
    assert_eq!(body["title"], "T");
    assert!(c.iter().any(|(m, p, _)| m == "GET" && p == "/findings?status=all"), "{c:?}");
}

/// Always on: a disabled list naming them is ignored, like the other
/// core tools - the guide decides when they are used, not a switch.
#[test]
fn the_finding_tools_are_core() {
    let call = |_m: &str, path: &str, _b: &str| -> Result<(u16, String), String> {
        if path == "/tools" {
            return Ok((200, r#"{"disabled":["record_finding","list_findings"]}"#.into()));
        }
        Ok((200, "{}".into()))
    };
    let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"record_finding") && names.contains(&"list_findings"), "{names:?}");
}
```

In `src-tauri/tests/ai_tools.rs`, leave the `TOOLS` stems array as it is (no new commands) and add, near the `CORE_TOOLS`-related tests if any exist (else at the end):

```rust
#[test]
fn the_finding_tools_are_always_on() {
    use v2_lib::ai_tools::{effective_disabled, CORE_TOOLS};
    assert!(CORE_TOOLS.contains(&"record_finding") && CORE_TOOLS.contains(&"list_findings"));
    let off = effective_disabled(&["record_finding".to_string(), "list_findings".to_string()]);
    assert!(!off.iter().any(|n| n == "record_finding" || n == "list_findings"), "{off:?}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test transform --test tcm_mcp --test ai_tools`
Expected: compile error on `SUPPORTED_OPS`; after a stub, the name-list, count and core assertions fail.

- [ ] **Step 3: Implement**

`src-tauri/src/transform.rs`:
- Remove `SetComment(String)` from the `Op` enum and every match arm on it (`Op::SetComment(v) => c.comment = v.clone()` in apply; the label arm near line 1085; the `"set_comment"` parse arm; `"set_comment"` in `known_keys`).
- Add near the top:
  ```rust
  /// Every operation `parse_ops` accepts, in the order the tool description
  /// lists them. One list, two readers: the unknown-op refusal and the MCP
  /// tool description (mcp.rs) are both built from it, so an op can no
  /// longer be reachable and undocumented at the same time.
  pub const SUPPORTED_OPS: [&str; 23] = [
      "set_tags", "add_tags", "remove_tags", "set_module", "set_automation_status",
      "set_preconditions", "set_reviewer_notes", "prefix_title", "suffix_title",
      "replace_in_title", "replace_in_steps", "replace_in_notes", "replace_in_preconditions",
      "normalise_citations", "prepend_step", "append_step", "remove_step_matching", "split_step",
      "sort_by", "group_by", "dedupe", "remove_cases", "insert_cases",
  ];
  ```
- Replace the unknown-op arm with:
  ```rust
              "set_comment" => {
                  return Err(format!(
                      "{label}: unknown op \"set_comment\". `comment` is the developer's field and \
                       is never written by an assistant; a problem you found is a finding - call \
                       record_finding. Supported: {}.",
                      SUPPORTED_OPS.join(", ")
                  ))
              }
              other => {
                  return Err(format!(
                      "{label}: unknown op \"{other}\". Supported: {}.",
                      SUPPORTED_OPS.join(", ")
                  ))
              }
  ```

`src-tauri/src/mcp.rs`:
- In `tools_list`, build the operations description before the `json!`:
  ```rust
  let ops = crate::transform::SUPPORTED_OPS.join(", ");
  let transform_ops_desc = format!(
      "Ops applied in order. Every op the server accepts: {ops}. WHICH KEYS EACH OP READS: \
       set_tags/add_tags/remove_tags/set_module/set_automation_status/set_preconditions/\
       set_reviewer_notes/prefix_title/suffix_title/sort_by/group_by take {{value}}; \
       replace_in_title/replace_in_steps/replace_in_notes/replace_in_preconditions take \
       {{find, replace}} (replace_in_notes edits the local reviewer_notes - the bulk repair for \
       check_spec_coverage findings); normalise_citations takes only where - it moves a Spec: \
       line above its blockquote and quotes it, or writes the exemption form for a table/code \
       block, and leaves anything with two pointers or two blocks for a person; \
       prepend_step/append_step take {{action, expected}}; remove_step_matching takes \
       {{value|find|action}} (substring against step actions); split_step takes {{find, into: \
       [{{action, expected}}, ...]}} and replaces each matching step with that sequence; \
       remove_cases takes only a required `where`; insert_cases takes {{cases, and optionally \
       ONE of at_index (zero-based) | before | after (a title fragment)}} - without one it \
       appends; dedupe takes nothing (first copy wins, no merge). There is no op for `comment`: \
       it is the developer's field and is never written by an assistant. Every op accepts \
       `where` with title_contains/has_tag/module_is/at_index - at_index (zero-based position \
       in the current draft) is the selector of last resort when two cases share a title. \
       sort_by/group_by values: title, module, tags, preconditions. A key an op does not read \
       is reported in `ignored`, never silently dropped. Replace ops are literal, \
       case-sensitive and replace EVERY occurrence; the report gives the occurrence count and \
       names cases left alone for differing capitalisation."
  );
  ```
  and use `"description": transform_ops_desc` for the `operations` property (the old literal is deleted).
- Add the two tools right after the `get_run_failures` entry:
  ```rust
          {
              "name": "record_finding",
              "description": "Record a problem you found while reading, for the developer to see on the AI Bridge tab and in the browser report: a test case that contradicts its spec, a spec that contradicts itself, code that does what neither says. Use it on your own whenever it applies. Local to this app - nothing is sent to Azure DevOps. Never put a problem in a case's `comment` (the developer's field) or in reviewer_notes (provenance only). Call list_findings first so you do not record what is already known.",
              "inputSchema": schema(serde_json::json!({
                  "kind": { "type": "string", "description": "\"test_case\", \"spec\" or \"code\"" },
                  "subject": { "type": "string", "description": "What it is about: the work item id, the spec file and section, or the file and member" },
                  "title": { "type": "string", "description": "One line" },
                  "detail": { "type": "string", "description": "Markdown: what you read, what you expected, where" },
              }), &["kind", "title"]),
          },
          {
              "name": "list_findings",
              "description": "The findings recorded for the open organization and project, newest first. Open ones by default; pass status \"resolved\" or \"all\".",
              "inputSchema": schema(serde_json::json!({
                  "status": { "type": "string", "description": "\"open\" (default), \"resolved\" or \"all\"" },
              }), &[]),
          },
  ```
- In `tools_call` add after the `get_run_failures` arm:
  ```rust
          "record_finding" => call("POST", "/findings", &args.to_string()),
          "list_findings" => {
              let target = match args["status"].as_str().filter(|s| !s.trim().is_empty()) {
                  Some(s) => format!("/findings?status={}", percent_encode(s)),
                  None => "/findings".to_string(),
              };
              call("GET", &target, "")
          }
  ```

`src-tauri/src/ai_tools.rs`: in `CORE_TOOLS` add, with a comment:
```rust
    // A problem an assistant finds must have somewhere to go that is not
    // the developer's comment field or the provenance notes. A switch that
    // could close that door would reopen the old habit.
    "record_finding",
    "list_findings",
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test transform --test tcm_mcp --test ai_tools`, then `cargo test --test bindings` (the `transform` module is not on the IPC surface, so bindings should be unchanged; run it to be sure nothing else moved).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/transform.rs src-tauri/src/mcp.rs src-tauri/src/ai_tools.rs src-tauri/tests/transform.rs src-tauri/tests/tcm_mcp.rs src-tauri/tests/ai_tools.rs
git commit -q -F - <<'EOF'
feat(v2): record_finding and list_findings, always on; set_comment removed; transform lists every op it accepts

The comment field is the developer's and no tool writes it now. One
SUPPORTED_OPS list feeds both the server's refusal and the tool
description, and a test holds them together.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: Commands for the UI

**Files:**
- Create: `src-tauri/src/commands/findings.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (command registration)
- Test: `src-tauri/tests/findings.rs` (append)

**Interfaces:**
- Produces (generated into `src/bindings.ts`): `listFindings(organization, project) -> Finding[]`, `setFindingStatus(id, status) -> Result<Finding, string>`, `removeFinding(id) -> Result<null, string>`.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/tests/findings.rs`:

```rust
/// The commands are thin over the store, but the root they use is the one
/// setup published - the same one the bridge writes through.
#[test]
fn the_commands_use_the_configured_root() {
    use v2_lib::findings::{configured_root, set_root};
    let dir = TempDir::new();
    set_root(dir.path().to_path_buf());
    assert_eq!(configured_root().as_deref(), Some(dir.path()));
    record(dir.path(), new_finding("code", "Via the store")).unwrap();
    let listed = v2_lib::commands::findings::list_findings_at(dir.path(), "acme", "Web");
    assert_eq!(listed.len(), 1);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test findings the_commands_use` → compile error, `commands::findings` missing.

- [ ] **Step 3: Write the commands**

`src-tauri/src/commands/findings.rs`:

```rust
//! AI Findings for the AI Bridge tab: list, resolve or reopen, dismiss.
//! Local data only; nothing here reaches Azure DevOps.

use std::path::{Path, PathBuf};

use crate::findings::{self, Finding};

/// The store root: what setup published, else derived from the handle.
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(r) = findings::configured_root() {
        return Ok(r);
    }
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// The listing against an explicit root, so a test can hold it to account
/// without an `AppHandle`.
pub fn list_findings_at(root: &Path, organization: &str, project: &str) -> Vec<Finding> {
    findings::list(root, organization, project)
}

#[tauri::command]
#[specta::specta]
pub fn list_findings(app: tauri::AppHandle, organization: String, project: String) -> Vec<Finding> {
    match root(&app) {
        Ok(r) => list_findings_at(&r, &organization, &project),
        Err(_) => vec![],
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_finding_status(app: tauri::AppHandle, id: String, status: String) -> Result<Finding, String> {
    findings::set_status(&root(&app)?, &id, &status)
}

#[tauri::command]
#[specta::specta]
pub fn remove_finding(app: tauri::AppHandle, id: String) -> Result<(), String> {
    findings::remove(&root(&app)?, &id)
}
```

`src-tauri/src/commands/mod.rs`: add `pub mod findings;`. `src-tauri/src/lib.rs`: register `findings::list_findings, findings::set_finding_status, findings::remove_finding,` in the command list, referenced the same way the `misc::` commands are.

- [ ] **Step 4: Run the tests and regenerate bindings**

Run: `cargo test --test findings` (7 passed), then `cargo test --test bindings` (3 passed; `src/bindings.ts` gains `listFindings`, `setFindingStatus`, `removeFinding`, `Finding`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/findings.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src-tauri/tests/findings.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): findings commands for the AI Bridge tab

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 5: Tool list mirror, the notification kind, and the App listener

**Files:**
- Modify: `src/lib/mcpTools.ts`, `src/lib/mcpTools.test.ts`
- Modify: `src/lib/notifications.ts`, `src/lib/notifications.test.ts`
- Modify: `src/App.tsx` (beside the existing `events.workAssigned.listen` effect)

**Interfaces:**
- Consumes: `events.findingRecorded` and `Finding` from `src/bindings.ts` (Task 4).
- Produces: `MCP_TOOLS` entries for both names; both in `CORE_TOOLS` (so no switch row); `NotificationKind` gains `"ai-finding"`; `noteFinding(org, {id, kind, title})`.

- [ ] **Step 1: Write the failing tests**

In `src/lib/mcpTools.test.ts` add:

```ts
/// Always on, like validate and optimize: the guide decides when a
/// finding is recorded, and a switch would only reopen the habit of
/// writing problems into the developer's comment field.
test("the finding tools are core and not listed as switches", () => {
  for (const name of ["record_finding", "list_findings"]) {
    expect(MCP_TOOLS.map((t) => t.name)).toContain(name);
    expect(CORE_TOOLS as readonly string[]).toContain(name);
    expect(visibleRows().flatMap((r) => r.names)).not.toContain(name);
  }
});
```

(Import `MCP_TOOLS` alongside the file's existing imports if it is not already imported.) The existing "five rows" test stays as it is.

In `src/lib/notifications.test.ts` add (mirror the file's existing setup for an org and its `afterEach` clearing):

```ts
/// A finding an assistant records is something that happened while you
/// were not looking, so it goes on the bell like an assignment does.
test("a recorded finding raises one notification, keyed by its id", () => {
  noteFinding("acme", { id: "1-0", kind: "spec", title: "AC-3 contradicts the table" });
  noteFinding("acme", { id: "1-0", kind: "spec", title: "AC-3 contradicts the table" });
  const list = listFor("acme");
  expect(list).toHaveLength(1);
  expect(list[0].kind).toBe("ai-finding");
  expect(list[0].title).toBe("AI finding: spec");
  expect(list[0].body).toBe("AC-3 contradicts the table");
});
```

(`listFor` stands for whatever this test file already uses to read an org's current list; use that name.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/lib/mcpTools.test.ts src/lib/notifications.test.ts`
Expected: the core assertion fails; `noteFinding` is not exported.

- [ ] **Step 3: Implement**

`src/lib/mcpTools.ts`:
- In `MCP_TOOLS`, after the `get_run_failures` entry add
  ```ts
  { name: "record_finding", label: "Record a finding", summary: "A problem an assistant found in a case, a spec or the code." },
  { name: "list_findings", label: "List findings", summary: "The findings recorded for this project." },
  ```
- Append `"record_finding", "list_findings"` to `CORE_TOOLS`, with a comment mirroring the Rust one.

`src/lib/notifications.ts`:
- `export type NotificationKind = "assigned" | "pr-conflict" | "pr-review" | "pr-comments" | "ai-finding";`
- Next to `noteAssigned`:
  ```ts
  /** A finding recorded through the AI bridge. One per finding id. */
  export function noteFinding(org: string, f: { id: string; kind: string; title: string }): void {
    const kind = f.kind === "test_case" ? "test case" : f.kind;
    raise(org, [{ id: `ai-finding:${f.id}`, kind: "ai-finding", title: `AI finding: ${kind}`, body: f.title }]);
  }
  ```
  (`raise` is this module's existing dedupe-and-prepend entry point; call it with the shape the other `note*` helpers use.)

`src/App.tsx`: next to the `workAssigned` listener effect add

```tsx
  // A finding recorded through the bridge: the bell says so, and the AI
  // Bridge tab's list refreshes even when it is not the open tab.
  useEffect(() => {
    if (!org) return;
    let un: (() => void) | undefined;
    events.findingRecorded
      .listen((e) => {
        if (e.payload.org !== org) return;
        noteFinding(org, e.payload);
        qc.invalidateQueries({ queryKey: ["findings", org, project] });
      })
      .then((u) => (un = u));
    return () => un?.();
  }, [org, project, qc]);
```

Import `noteFinding` from `./lib/notifications`. If the `workAssigned` effect uses a `detach` helper for its unlisten, use the same here.

- [ ] **Step 4: Run the tests**

Run: `npx tsc --noEmit` then `npx vitest run src/lib/mcpTools.test.ts src/lib/notifications.test.ts src/screens/AiBridge.test.tsx`
Expected: all pass (the AI Bridge tab's switch rows are unchanged).

- [ ] **Step 5: Commit**

```bash
git add src/lib/mcpTools.ts src/lib/mcpTools.test.ts src/lib/notifications.ts src/lib/notifications.test.ts src/App.tsx
git commit -q -F - <<'EOF'
feat(v2): finding tools mirrored as core; a recorded finding reaches the bell

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 6: The Findings card on the AI Bridge tab

**Files:**
- Create: `src/components/FindingsCard.tsx`, `src/components/FindingsCard.test.tsx`
- Modify: `src/screens/AiBridge.tsx` (card placed in the left column after the tools section), `src/screens/AiBridge.test.tsx`
- Modify: `src/lib/actionIcons.ts` only if it lacks `IconFinding`: add `Lightbulb as IconFinding,` beside the other lucide aliases.

**Interfaces:**
- Consumes: `commands.listFindings`, `commands.setFindingStatus`, `commands.removeFinding`, type `Finding` (Task 4); `renderMarkdown` from `src/lib/richText`; `Badge`, `Button`, `Switch` from `src/components/ui`.
- Produces: `<FindingsCard org project />`.

- [ ] **Step 1: Write the failing tests**

`src/components/FindingsCard.test.tsx`:

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import FindingsCard from "./FindingsCard";
import type { Finding } from "../bindings";

afterEach(() => clearMocks());

const open: Finding = {
  id: "1-0", org: "acme", project: "Web", kind: "spec",
  subject: "Step10.md 7.7", title: "AC-3 contradicts the table",
  detail: "The table says **closed**.", created_at: "2026-09-11T10:00:00Z", status: "open",
};
const done: Finding = { ...open, id: "1-1", kind: "test_case", subject: "155170", title: "Step 3 expects a toast", status: "resolved" };

function renderCard(findings: Finding[], calls: string[] = []) {
  mockIPC((cmd, args) => {
    calls.push(cmd);
    if (cmd === "list_findings") return findings;
    if (cmd === "set_finding_status") {
      const a = args as { id: string; status: string };
      return { status: "ok", data: { ...findings.find((f) => f.id === a.id)!, status: a.status } };
    }
    if (cmd === "remove_finding") return { status: "ok", data: null };
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <FindingsCard org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

/// Open findings are the default view: kind, subject, title, and the
/// detail rendered as markdown. Resolved ones wait behind a switch.
test("open findings are listed with their kind and rendered detail", async () => {
  renderCard([open, done]);
  expect(await screen.findByText("AC-3 contradicts the table")).toBeInTheDocument();
  expect(screen.getByText("Spec")).toBeInTheDocument();
  expect(screen.getByText("Step10.md 7.7")).toBeInTheDocument();
  expect(screen.getByText("closed").tagName).toBe("STRONG");
  expect(screen.queryByText("Step 3 expects a toast")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch", { name: "Show resolved" }));
  expect(screen.getByText("Step 3 expects a toast")).toBeInTheDocument();
  expect(screen.getByText("Test case")).toBeInTheDocument();
});

test("Resolve and Dismiss call their commands with the finding's id", async () => {
  const calls: string[] = [];
  renderCard([open], calls);
  const row = (await screen.findByText("AC-3 contradicts the table")).closest("li")!;
  fireEvent.click(within(row).getByRole("button", { name: "Resolve" }));
  await waitFor(() => expect(calls).toContain("set_finding_status"));
  fireEvent.click(within(row).getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(calls).toContain("remove_finding"));
});

test("with nothing recorded the card says so", async () => {
  renderCard([]);
  expect(await screen.findByText(/No findings yet/)).toBeInTheDocument();
});
```

In `src/screens/AiBridge.test.tsx` add:

```tsx
test("the findings card is on the tab", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") return [];
    if (cmd === "list_findings") return [];
    return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);
  expect(await screen.findByRole("heading", { name: "AI Findings" })).toBeInTheDocument();
});
```

(If the tab's `renderBridge` helper renders `AiBridge` without `org`/`project` props, look at how the tab already obtains them for its other cards and thread the same values into the card.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/components/FindingsCard.test.tsx src/screens/AiBridge.test.tsx`
Expected: `FindingsCard` module not found; the heading is absent.

- [ ] **Step 3: Write the card**

`src/components/FindingsCard.tsx`:

```tsx
// AI Findings: the problems an assistant recorded while reading - a test
// case against its spec, a spec against itself, code against both. Local
// to this app. The developer reads them here, resolves them when acted
// on, dismisses the noise.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type Finding } from "../bindings";
import { IconConfirm, IconFinding, IconRemove } from "../lib/actionIcons";
import { cn } from "../lib/cn";
import { renderMarkdown } from "../lib/richText";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Switch } from "./ui/switch";

const KIND_LABEL: Record<string, string> = { test_case: "Test case", spec: "Spec", code: "Code" };

export default function FindingsCard({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [showResolved, setShowResolved] = useState(false);
  const findings = useQuery({
    queryKey: ["findings", org, project],
    queryFn: () => commands.listFindings(org, project),
    enabled: Boolean(org && project),
  });
  const refresh = () => qc.invalidateQueries({ queryKey: ["findings", org, project] });
  const setStatus = useMutation({
    mutationFn: async ({ id, status }: { id: string; status: string }) => {
      const r = await commands.setFindingStatus(id, status);
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: refresh,
    onError: (e) => toast.error(`Could not update the finding: ${e.message}`),
  });
  const remove = useMutation({
    mutationFn: async (id: string) => {
      const r = await commands.removeFinding(id);
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: refresh,
    onError: (e) => toast.error(`Could not dismiss the finding: ${e.message}`),
  });

  const all = findings.data ?? [];
  const shown = showResolved ? all : all.filter((f) => f.status === "open");
  const openCount = all.filter((f) => f.status === "open").length;

  return (
    <section data-tour="ai-findings" className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center gap-2">
        <IconFinding aria-hidden className="size-3.5 shrink-0 text-muted" />
        <h2 className="text-sm font-semibold text-text">AI Findings</h2>
        {openCount > 0 && <Badge className="bg-accent-soft text-accent">{openCount} open</Badge>}
        <label className="ml-auto flex items-center gap-2 text-xs text-muted">
          <Switch checked={showResolved} onCheckedChange={setShowResolved} ariaLabel="Show resolved" />
          Show resolved
        </label>
      </div>
      <p className="text-xs text-muted">
        Problems an assistant found while reading a test case, a spec or the code. Kept in this
        app only. Resolve one when you have acted on it. Dismiss one that is wrong.
      </p>
      {shown.length === 0 ? (
        <p className="text-xs text-faint">
          {all.length === 0
            ? "No findings yet. An assistant records one when something it reads is wrong."
            : "Nothing open. Switch on Show resolved to see the rest."}
        </p>
      ) : (
        <ul className="space-y-2">
          {shown.map((f) => (
            <li
              key={f.id}
              className={cn("space-y-1 rounded-md border border-border p-3", f.status === "resolved" && "opacity-70")}
            >
              <div className="flex flex-wrap items-center gap-2">
                <Badge className="bg-surface-2 text-muted">{KIND_LABEL[f.kind] ?? f.kind}</Badge>
                {f.subject && <span className="id-mono text-xs text-muted">{f.subject}</span>}
                <span className="ml-auto text-[11px] text-faint">{f.created_at.slice(0, 10)}</span>
              </div>
              <p className="text-sm font-medium text-text">{f.title}</p>
              {f.detail && (
                <div className="text-xs text-muted" dangerouslySetInnerHTML={{ __html: renderMarkdown(f.detail) }} />
              )}
              <div className="flex gap-2 pt-1">
                {f.status === "open" ? (
                  <Button size="sm" variant="outline" onClick={() => setStatus.mutate({ id: f.id, status: "resolved" })}>
                    <IconConfirm aria-hidden />
                    Resolve
                  </Button>
                ) : (
                  <Button size="sm" variant="outline" onClick={() => setStatus.mutate({ id: f.id, status: "open" })}>
                    Reopen
                  </Button>
                )}
                <Button size="sm" variant="ghost" onClick={() => remove.mutate(f.id)}>
                  <IconRemove aria-hidden />
                  Dismiss
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
```

Notes: `renderMarkdown` in `src/lib/richText.ts` sanitises with DOMPurify, which is why `dangerouslySetInnerHTML` is acceptable (the same pattern `CommentsPanel.tsx` uses). If `IconFinding` does not exist in `src/lib/actionIcons.ts`, add `Lightbulb as IconFinding,` there. If `Badge` or `Switch` take different prop names, match how `AiBridge.tsx` already uses them.

`src/screens/AiBridge.tsx`: import `FindingsCard` and render `<FindingsCard org={...} project={...} />` in the left column directly after the `data-tour="ai-tools"` section closes, with the org and project values the tab already holds. Do not add a "How it works" bullet: that list is the inventory of switchable tools, and these are not switchable.

- [ ] **Step 4: Run the tests**

Run: `npx tsc --noEmit`, then `npx vitest run src/components/FindingsCard.test.tsx src/screens/AiBridge.test.tsx src/ui-consistency.test.ts`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/components/FindingsCard.tsx src/components/FindingsCard.test.tsx src/screens/AiBridge.tsx src/screens/AiBridge.test.tsx src/lib/actionIcons.ts
git commit -q -F - <<'EOF'
feat(v2): AI Findings card on the AI Bridge tab

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 7: The validator keeps the human fields human; the browser report shows findings

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (the advisories block in `validate_json`, around the `both_branches_reason` and `parse_citations` advisories)
- Modify: `src-tauri/src/import_parser/html.rs` (`export_queue_to_html` gains a `findings: &[crate::findings::Finding]` parameter; an "AI Findings" section)
- Modify: `src-tauri/src/commands/queue.rs` (the three callers of `export_queue_to_html`)
- Test: `src-tauri/tests/ai_bridge.rs` (validate advisories), a new `src-tauri/tests/findings_report.rs`

**Interfaces:**
- Consumes: `findings::{Finding, list_open, configured_root}`.
- Produces: `export_queue_to_html(queue, path, subtitle, ctx, palette, findings)`; validate `advisories` entries for a `comment` on a new case and for finding-like `reviewer_notes`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/tests/ai_bridge.rs` add:

```rust
/// The two human fields. A `comment` on a case with no id was written by
/// the assistant (a case with an id may carry the developer's own,
/// round-tripped), and a note that reports a problem is a finding in the
/// wrong place. Both are advisories - judgement calls, said out loud.
#[tokio::test]
async fn validate_advises_when_the_human_fields_carry_the_assistants_words() {
    let draft = serde_json::json!({
        "test_cases": [
            {
                "title": "New case with a comment",
                "automation_status": "Not Automated",
                "comment": "Spec and code disagree here",
                "steps": [{ "action": "Open the page.", "expected": "It opens." }]
            },
            {
                "id": 155170,
                "title": "Existing case with the developer's comment",
                "automation_status": "Not Automated",
                "comment": "Blocked until the API lands",
                "steps": [{ "action": "Open the page.", "expected": "It opens." }]
            },
            {
                "title": "Note that reports a problem",
                "automation_status": "Not Automated",
                "reviewer_notes": "Checks the cut-off. Spec: S.md 7.7\n> \"closed at cut-off\"\nNote: the code contradicts the spec here, the status stays open.",
                "steps": [{ "action": "Open the page.", "expected": "It opens." }]
            }
        ]
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let adv: Vec<String> = v["advisories"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert!(adv.iter().any(|a| a.contains("Test case 1") && a.contains("comment") && a.contains("record_finding")), "{adv:?}");
    assert!(!adv.iter().any(|a| a.contains("Test case 2") && a.contains("comment")), "the developer's own comment on an existing case is not questioned: {adv:?}");
    assert!(adv.iter().any(|a| a.contains("Test case 3") && a.contains("reviewer_notes") && a.contains("record_finding")), "{adv:?}");
}
```

`src-tauri/tests/findings_report.rs`:

```rust
//! The browser report carries the open findings for the project in a
//! section of its own - never inside a case's notes or comment.

use v2_lib::findings::Finding;
use v2_lib::import_parser::export_queue_to_html;
use v2_lib::model::TestCase;
use v2_lib::steps_xml::Step;

fn case(title: &str) -> TestCase {
    TestCase {
        title: title.into(),
        steps: vec![Step { action: "Open".into(), expected: "Shown".into() }],
        ..Default::default()
    }
}

fn finding(kind: &str, title: &str, status: &str) -> Finding {
    Finding {
        id: format!("id-{title}"),
        org: "acme".into(),
        project: "Web".into(),
        kind: kind.into(),
        subject: "155170".into(),
        title: title.into(),
        detail: "The table says **closed**.".into(),
        created_at: "2026-09-11T10:00:00Z".into(),
        status: status.into(),
    }
}

fn render(findings: &[Finding]) -> String {
    let path = std::env::temp_dir().join(format!("tcm-findings-report-{}-{}.html", std::process::id(), findings.len()));
    export_queue_to_html(&[case("A")], path.to_str().unwrap(), "Sub", None, &Default::default(), findings).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    html
}

#[test]
fn open_findings_get_their_own_section_with_kind_and_markdown() {
    let html = render(&[finding("spec", "AC-3 contradicts the table", "open"), finding("code", "Resolved one", "resolved")]);
    let section = html.split("<section class='findings'").nth(1).expect("a findings section");
    assert!(section.contains("AI Findings"));
    assert!(section.contains("AC-3 contradicts the table"));
    assert!(section.contains("<strong>closed</strong>"), "detail is markdown: {section}");
    assert!(section.contains("Spec"));
    assert!(!section.contains("Resolved one"), "only open findings: {section}");
    // The section is its own block, not inside the case card.
    let case_at = html.find("<div class='case'>").unwrap();
    let findings_at = html.find("<section class='findings'").unwrap();
    assert!(findings_at < case_at, "findings come before the cases");
}

#[test]
fn no_open_findings_means_no_section() {
    let html = render(&[finding("code", "Done", "resolved")]);
    assert!(!html.contains("<section class='findings'"));
    let html = render(&[]);
    assert!(!html.contains("AI Findings"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test findings_report` (compile error: wrong arity), then `cargo test --test ai_bridge validate_advises` (fails, no advisories).

- [ ] **Step 3: Implement**

`src-tauri/src/ai_bridge.rs`, inside the `for (i, tc) in cases.iter().enumerate()` loop of the advisories block, after the `parse_citations` advisory:

```rust
                // The two human fields. A comment on a case with NO id was
                // written by the assistant - a case with an id may carry the
                // developer's own, round-tripped through the file - and a
                // note that reports a problem is a finding in the wrong
                // place. Both judgement calls: said, not blocked.
                if tc.update_id.is_none() && !tc.comment.trim().is_empty() {
                    advisories.push(format!(
                        "Test case {} ('{}') carries a `comment`. That field is the developer's and \
                         an assistant never writes it. If this is a problem you found, remove it here \
                         and call record_finding.",
                        i + 1,
                        tc.title
                    ));
                }
                if let Some(why) = finding_like_note(&tc.reviewer_notes) {
                    advisories.push(format!(
                        "Test case {} ('{}'): its reviewer_notes read like a problem report ({why}). \
                         reviewer_notes say only what the case checks and where the requirement \
                         lives; a problem is a finding - call record_finding and take it out of the note.",
                        i + 1,
                        tc.title
                    ));
                }
```

and add, near `validate_json`:

```rust
/// A reviewer note that reports a problem rather than a provenance. The
/// phrases are the ones assistants actually used when they wrote defects
/// into notes; a hit is an advisory, never a block.
fn finding_like_note(notes: &str) -> Option<String> {
    const PHRASES: [&str; 10] = [
        "contradict", "does not match", "doesn't match", "mismatch", "discrepan",
        "inconsisten", "bug:", "defect", "the code does not", "spec says",
    ];
    let lower = notes.to_lowercase();
    PHRASES
        .iter()
        .find(|p| lower.contains(*p))
        .map(|p| format!("it says \"{p}\""))
}
```

`src-tauri/src/import_parser/html.rs`:
- `export_queue_to_html` gains a final parameter `findings: &[crate::findings::Finding]`.
- Right after the `"<p id='tc-no-match' ...>"` part is pushed and before the `for (idx, tc) in queue.iter().enumerate()` loop, add:
  ```rust
      // AI Findings: what an assistant found wrong, in a block of its own
      // above the cases - never inside a case's notes (provenance) or its
      // comment box (the developer's). Open ones only; resolved is done.
      let open: Vec<&crate::findings::Finding> = findings.iter().filter(|f| f.status == "open").collect();
      if !open.is_empty() {
          parts.push(format!(
              "<section class='findings'><h2>AI Findings <span class='count'>{}</span></h2>\
               <p class='lead'>Problems an assistant found while reading. Resolve or dismiss them on the AI Bridge tab.</p>",
              open.len()
          ));
          for f in open {
              let kind = match f.kind.as_str() {
                  "test_case" => "Test case",
                  "spec" => "Spec",
                  "code" => "Code",
                  other => other,
              };
              parts.push(format!(
                  "<article class='finding'><div class='meta'><span class='kind'>{}</span>\
                   <span class='subject'>{}</span><span class='when'>{}</span></div>\
                   <p class='ftitle'>{}</p><div class='fdetail'>{}</div></article>",
                  esc(kind),
                  esc(&f.subject),
                  esc(&f.created_at[..f.created_at.len().min(10)]),
                  esc(&f.title),
                  crate::markdown::to_html(&f.detail)
              ));
          }
          parts.push("</section>".into());
      }
  ```
- In the page's `<style>` block add, using the page's existing CSS variables for colours (look at how `.rev` and `.case` are styled and reuse those variable names; never a raw hex that the page does not already define):
  ```css
  .findings{margin:0 0 20px;padding:14px 16px;border:1px solid var(--border);border-radius:10px;background:var(--card)}
  .findings h2{margin:0 0 4px;font-size:15px}
  .findings .count{font-weight:normal;opacity:.7;font-size:12px;margin-left:6px}
  .findings .lead{margin:0 0 10px;font-size:12px;opacity:.75}
  .finding{padding:10px 0;border-top:1px solid var(--border)}
  .finding .meta{display:flex;gap:10px;font-size:11px;opacity:.75;align-items:center}
  .finding .kind{font-weight:600;text-transform:uppercase;letter-spacing:.04em}
  .finding .when{margin-left:auto}
  .finding .ftitle{margin:4px 0 2px;font-weight:600}
  .finding .fdetail{font-size:13px}
  ```
  (Substitute `var(--border)` / `var(--card)` with whatever names the page already uses for its card border and surface; read the existing `<style>` first.)

`src-tauri/src/commands/queue.rs`: each of the three `export_queue_to_html(...)` calls gains the findings argument:
- `export_queue_html` (a file that leaves the machine): pass `&[]` — a shared export carries no local findings.
- `view_queue_html` (has `organization`; add `project: String` to its parameters if it does not already have one, and update the single frontend caller in `src/screens/ViewCases/index.tsx` to pass the project): pass `&open_findings(&organization, &project)`.
- `view_draft_html` (the draft page from the Import tab): add `organization: String, project: String` parameters if absent, update its frontend caller in `src/components/QueueSection.tsx` to pass them, and pass `&open_findings(&organization, &project)`.

with this helper in `queue.rs`:

```rust
/// The open findings for the page's org and project, or none outside the
/// app (no root published yet).
fn open_findings(organization: &str, project: &str) -> Vec<crate::findings::Finding> {
    match crate::findings::configured_root() {
        Some(root) if !organization.is_empty() => crate::findings::list_open(&root, organization, project),
        _ => vec![],
    }
}
```

After changing command signatures run `cargo test --test bindings` and fix the frontend callers the typecheck names.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test findings_report --test ai_bridge --test draft_comments` (the draft-comments tests call the html export too; adjust their calls to pass `&[]`), then `cargo test --test bindings`, then `npx tsc --noEmit`, then `npx vitest run src/screens/ViewCases src/components/QueueSection.test.tsx`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai_bridge.rs src-tauri/src/import_parser/html.rs src-tauri/src/commands/queue.rs src-tauri/tests/ai_bridge.rs src-tauri/tests/findings_report.rs src-tauri/tests/draft_comments.rs src/bindings.ts src/screens/ViewCases/index.tsx src/components/QueueSection.tsx
git commit -q -F - <<'EOF'
feat(v2): validator keeps comment and reviewer notes human; the browser report shows AI findings

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 8: Slash commands trimmed to what a person types

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` (the `COMMANDS` array and the `write` spec)
- Test: `src-tauri/tests/ai_tools.rs`
- Check (grep, no code change unless a hit): `src/` for the literal `/tcm:` to update any user-facing mention of a removed or renamed command (tour copy, How it works, empty states).

**Interfaces:**
- Produces: `COMMANDS` stems, in this order: `begin-test-case-writing`, `failures`, `autorun`, `script`, `optimize`, `get-wiki-info`, `page`. Renamed: `write` → `begin-test-case-writing`, `wiki` → `get-wiki-info`. Removed: `fanout`, `guide`, `examples`, `suites`, `suite-cases`, `coverage`, `validate`, `transform`, `tags`, `pbis`. The tools behind the removed commands stay available; the guide and the tool descriptions already tell the assistant when to call them.

- [ ] **Step 1: Write the failing test**

In `src-tauri/tests/ai_tools.rs` replace the `TOOLS` stems array with

```rust
    // Only what a person reaches for by name. Everything else the
    // assistant calls on its own when the guide says so; a command per
    // tool made the picker a list of things nobody should have to know.
    const TOOLS: [&str; 7] = [
        "begin-test-case-writing", "failures", "autorun", "script", "optimize", "get-wiki-info", "page",
    ];
```

Any test in that file that looks up a removed stem by name (there is one that finds `"fanout"`) is deleted, and the fan-out guidance it checked moves into the `begin-test-case-writing` body (Step 3). Add:

```rust
/// The rename: the one command everyone uses says what it does. Spaces
/// are not usable in a slash-command name, so the words are hyphenated.
#[test]
fn the_writing_command_is_named_for_what_it_does() {
    let c = COMMANDS.iter().find(|c| c.stem == "begin-test-case-writing").unwrap();
    assert_eq!(c.tool, "begin_test_case_writing");
    assert!(COMMANDS.iter().all(|c| c.stem != "write"));
    let w = COMMANDS.iter().find(|c| c.stem == "get-wiki-info").unwrap();
    assert_eq!(w.tool, "search_wiki");
    assert!(COMMANDS.iter().all(|c| c.stem != "wiki"));
    for gone in ["fanout", "guide", "examples", "suites", "suite-cases", "coverage", "validate", "transform", "tags", "pbis"] {
        assert!(COMMANDS.iter().all(|c| c.stem != gone), "{gone} is no longer a command");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test ai_tools` → the stems assertion fails.

- [ ] **Step 3: Implement**

In `src-tauri/src/ai_tools.rs`:
- Change the first `CommandSpec`'s `stem` from `"write"` to `"begin-test-case-writing"`. Keep its `tool`, `desc` and `hint`. Append to its `body` (after the existing last line) the fan-out guidance the removed `fanout` command carried, condensed:
  ```rust
            "",
            "For a large specification, fan the job out yourself: after intake, call",
            "`check_spec_coverage` with an empty draft (`json: \"[]\"`) and the plan's spec",
            "paths - every section comes back in `uncovered`, which IS the slice list. One",
            "subagent per slice, each with its own section range and output file",
            "(`<output-stem>-slice-<n>.json`), each calling `get_writing_guide` itself. Merge",
            "with `merge_case_files`, never by hand; then `check_spec_coverage` once on the",
            "merged file, then `optimize_cases` exactly once before handing over.",
  ```
- Change the `wiki` `CommandSpec`'s `stem` to `"get-wiki-info"` (tool, desc, hint and body unchanged).
- Delete the `CommandSpec` entries with stems `fanout`, `guide`, `examples`, `suites`, `suite-cases`, `coverage`, `validate`, `transform`, `tags`, `pbis`.
- Update the doc comment above `COMMANDS` to say: one command per thing a person reaches for by name; every other tool is called by the assistant when the guide says so.
- Grep `src/` for `/tcm:` and update any copy that names a removed or renamed command so it names `/tcm:begin-test-case-writing` or drops the reference.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test ai_tools`, then `npx vitest run` if any `src/` copy changed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai_tools.rs src-tauri/tests/ai_tools.rs
git commit -q -F - <<'EOF'
feat(v2): slash commands trimmed to what a person types; write becomes begin-test-case-writing

The assistant calls the guide, examples, suites, tags, PBI search,
coverage, validate and transform on its own; the picker no longer lists
them.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 9: Changelog entry (at ship time)

- [ ] Add the newest entry to `src/lib/changelog.ts` (version = next free patch after `git fetch`; today's date), bump `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, run `cargo check --lib` for the lockfile, then `scripts/release-v2.ps1 -Version X.Y.Z`:

```ts
  {
    version: "1.23.X",
    date: "2026-09-XX",
    items: [
      "AI Findings. When an assistant finds a problem while reading a test case, a spec or the code, it records a finding instead of writing it into the case. Findings appear in a card on the AI Bridge tab, in their own section of the browser report, and on the notification bell. Resolve or dismiss them on the AI Bridge tab. They stay in this app.",
      "The comment on a test case is yours. Assistants no longer have any way to write it, and the validator says so if a draft tries. Reviewer notes are for where a case came from; a problem reported in one is pointed at the findings instead.",
      "Bulk edits by an assistant now reach every operation the app supports. The tool's description used to list fewer than the app accepted.",
      "Fewer slash commands. /tcm:write is now /tcm:begin-test-case-writing, /tcm:wiki is now /tcm:get-wiki-info, and the commands for the guide, examples, suites, tags, PBI search, coverage, validate and transform are gone: the assistant calls those tools on its own when they are needed.",
    ],
  },
```

---

## Self-review

**Spec coverage.** Store + scoping (Task 1); the assistant can write and read, and the guide tells it when, on its own (Task 2); always-on tools with no switch and no slash commands (Tasks 3, 5); the developer's area with resolve/dismiss (Task 6); the bell (Task 5); a separate section in the browser report (Task 7); `comment` never written by AI: op removed (Task 3), guide (Task 2), validator (Task 7); `reviewer_notes` provenance only: guide (Task 2), validator (Task 7); transform description gap closed and tested (Task 3).

**Placeholders.** None. Where the plan says "match the existing name/prop" it names the file and the thing to read.

**Type consistency.** `Finding` fields identical in Tasks 1, 2, 4, 6, 7. `NewFinding` in Tasks 1, 2. `list_open` defined in Task 1, used in Tasks 2 and 7. Commands `list_findings / set_finding_status / remove_finding` (Task 4) → bindings `listFindings / setFindingStatus / removeFinding` (Task 6). Tools `record_finding / list_findings` in Tasks 3, 5. Event `FindingRecorded { id, org, project, kind, title }` in Tasks 1, 5. Query key `["findings", org, project]` in Tasks 5, 6. `SUPPORTED_OPS` length 23 in Task 3's const and both tests. `export_queue_to_html` arity: six arguments everywhere in Task 7.
