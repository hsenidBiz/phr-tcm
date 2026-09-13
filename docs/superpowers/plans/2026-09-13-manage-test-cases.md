# Manage Test Cases Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new "Manage Test Cases" section (development builds only, like Auto Run) where the user picks a suite from the plan tree and works on its test cases in bulk: re-order them (drag and drop, or apply the `tester_order` from a draft `.json`), move selected cases to a different PBI, and copy selected cases into "folders" (static child suites) created from the app.

**Architecture:** Four new Azure DevOps Test Plan calls in Rust (`GET`/`PATCH` suiteentry, `POST` static suite, `POST` suite test cases), each behind a thin Tauri command. The frontend adds a `manage` Section gated by `MANAGE_CASES_ENABLED = import.meta.env.DEV` exactly the way `autorun` is gated by `AUTO_RUN_ENABLED`, a `ManageCases` screen built from a suite picker (reusing the cached `plans-suites` query), an orderable case list (native HTML5 drag events, no library), and three actions that reuse what exists: `relinkTestCases` + `RelinkDialog` for the PBI move, `parseImportFile` for the tester order, and the plan tree for folder targets.

**Tech Stack:** Rust (Tauri 2, tauri-specta, reqwest, serde_json, wiremock tests), React 19 + TypeScript, TanStack Query, vitest + Testing Library.

## Global Constraints

- All Rust tests are integration tests under `src-tauri/tests/`; never a `#[cfg(test)]` module inside `src/`.
- `src/bindings.ts` is generated: after any command or IPC-type change run, from `src-tauri/`, `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test bindings`. Never hand-edit it. A phantom `M src/bindings.ts` with a whitespace-only diff after a build is reverted with `git checkout -- src/bindings.ts`.
- Run one build or test command at a time on this shared machine. Gates: `cargo test --tests` (in `src-tauri/`), `npx tsc --noEmit`, `npx vitest run` (repo root). Run each task's named tests while working and the full gate that task's file map touches before committing.
- No DELETE to Azure DevOps anywhere. The new client calls are GET, POST and PATCH only.
- Every Azure DevOps write is logged with `crate::applog::warn` naming the ids, the way `relink_test_cases` does.
- The section is development-build only: `MANAGE_CASES_ENABLED = import.meta.env.DEV` in `src/components/Sidebar.tsx`; the sidebar row, the App route, the keyboard shortcut and the prefs list follow the `autorun` pattern exactly. The `SECTIONS` list in `src/lib/prefs.ts` does NOT include `manage` (it does not include `autorun` either: a persisted dev-only section must fall back to `manual` in a release build).
- Colours come from tokens only (`text-muted`, `bg-surface-2`, `border-accent`, `text-warning`, `bg-accent-soft`); never a Tailwind palette colour or a hex value.
- Button icons come from `src/lib/actionIcons.ts` as `<IconX aria-hidden />`, never with a `size` prop; new intents are added to that file. Icons that are not inside a button (a drag grip, a folder glyph in a list row) may come straight from `lucide-react` with `size={14}`, as `src/screens/Suites.tsx` does.
- Icon-only interactive elements carry an `aria-label`. `src/ui-consistency.test.ts` enforces all of the above and must not be weakened.
- No em dashes in any text a user reads.
- Commits use a Bash heredoc `git commit -q -F - <<'EOF' … EOF` ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Sidebar labels and tool switch rows use human names: "Manage Test Cases", "In Dev", "In Development".
- Azure DevOps rule to honour in the UI: child suites (folders) can only be created under a static suite or the plan root. A requirement-based suite cannot hold child suites, so "New folder" targets a static parent picked by the user, defaulting to the plan root.

---

## File map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/ado/transport.rs` (modify) | `patch_json`: PATCH with a plain JSON body (the suiteentry reorder is an array, not a json-patch document). |
| `src-tauri/src/ado_testplan/entries.rs` (create) | `SuiteEntry`; `get_suite_entries`, `reorder_suite_cases`, `create_static_suite`, `add_test_cases_to_suite`; pure `ordered_case_ids`. |
| `src-tauri/src/ado_testplan/mod.rs` (modify) | `mod entries; pub use entries::{ordered_case_ids, SuiteEntry};`. |
| `src-tauri/tests/suite_manage.rs` (create) | wiremock tests for the four calls and the pure ordering. |
| `src-tauri/src/commands/testplan.rs` (modify) | Commands `list_suite_entries`, `reorder_suite_cases`, `create_static_suite`, `add_cases_to_suite`. |
| `src-tauri/src/lib.rs` (modify) | Register the four commands. |
| `src/bindings.ts` (generated) | Regenerated. |
| `src/lib/suiteTree.ts` (create) + `.test.ts` | `buildTree`, `flattenTree` (shared by Suites and the picker). |
| `src/screens/Suites.tsx` (modify) | Imports `buildTree` from `lib/suiteTree` instead of its local copy. |
| `src/lib/suiteOrder.ts` (create) + `.test.ts` | Pure list helpers: `moveItem`, `orderFromFile`. |
| `src/components/Sidebar.tsx` (modify) | `manage` Section, `MANAGE_CASES_ENABLED`, the row, the filter. |
| `src/App.tsx` (modify) | Title, note, shortcut order, route. |
| `src/index.css` (modify) | `.nav-ico-manage` hover animation. |
| `src/lib/actionIcons.ts` (modify) | `IconMoveUp`, `IconMoveDown`, `IconNewFolder`, `IconAddToFolder`. |
| `src/screens/ManageCases/index.tsx` (create) | The screen: picker, list, actions. |
| `src/screens/ManageCases/SuitePicker.tsx` (create) | Plan + suite dropdowns over the cached plan tree. |
| `src/screens/ManageCases/CaseOrderList.tsx` (create) | The orderable, selectable list. |
| `src/screens/ManageCases/NewFolderDialog.tsx` (create) | Name + parent, optional "and add N cases". |
| `src/screens/ManageCases/testSupport.tsx`, `src/screens/ManageCases/*.test.tsx` (create) | Shared IPC fixture and the screen tests. |
| `src/components/RelinkDialog.tsx` (modify) | `cases` prop widened to `{ id, title }[]`. |
| `src/components/Sidebar.test.tsx`, `src/App.test.tsx` (modify) | Row, flag and shortcut assertions. |

No changelog entry: the section is hidden in release builds and the changelog is end-user-facing.

---

### Task 1: Rust client calls for suite entries, static suites and suite test cases

**Files:**
- Modify: `src-tauri/src/ado/transport.rs` (after `post_json`, around line 237)
- Create: `src-tauri/src/ado_testplan/entries.rs`
- Modify: `src-tauri/src/ado_testplan/mod.rs:13-15`
- Test: `src-tauri/tests/suite_manage.rs`

**Interfaces:**
- Consumes: `AdoClient::tp_base(org, project)` (private helper in `ado_testplan/mod.rs`, usable from a sibling module), `get_json`, `post_json`, `send`, `handle_json` in `ado/transport.rs`, `SuiteRef` in `ado_testplan/mod.rs`.
- Produces:
  - `pub struct SuiteEntry { pub id: i32, pub sequence_number: i32, pub entry_type: String }` (Serialize + specta::Type)
  - `AdoClient::get_suite_entries(&self, org: &str, project: &str, suite_id: i32) -> Result<Vec<SuiteEntry>, AdoError>`
  - `AdoClient::reorder_suite_cases(&self, org: &str, project: &str, suite_id: i32, case_ids: &[i32]) -> Result<Vec<i32>, AdoError>`
  - `AdoClient::create_static_suite(&self, org: &str, project: &str, plan_id: i32, parent_suite_id: i32, name: &str) -> Result<SuiteRef, AdoError>`
  - `AdoClient::add_test_cases_to_suite(&self, org: &str, project: &str, plan_id: i32, suite_id: i32, case_ids: &[i32]) -> Result<Vec<i32>, AdoError>`
  - `pub fn ordered_case_ids(current: &[SuiteEntry], wanted: &[i32]) -> Vec<i32>`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/tests/suite_manage.rs`:

```rust
//! The Manage Test Cases calls: suite entry order, static child suites and
//! adding cases to a suite. GET / POST / PATCH only.

use v2_lib::ado::AdoClient;
use v2_lib::ado_testplan::{ordered_case_ids, SuiteEntry};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn entry(id: i32, seq: i32, kind: &str) -> SuiteEntry {
    SuiteEntry { id, sequence_number: seq, entry_type: kind.to_string() }
}

#[test]
fn ordered_case_ids_puts_named_cases_first_then_the_rest_in_place() {
    let current = vec![
        entry(3, 0, "suite"),
        entry(8, 1, "testCase"),
        entry(9, 2, "testCase"),
        entry(10, 3, "testCase"),
    ];
    // Named ids lead in the order given; 9 was not named and keeps its
    // place after them; 77 is not in the suite and is dropped; a repeat
    // of 8 counts once.
    assert_eq!(ordered_case_ids(&current, &[10, 8, 77, 8]), vec![10, 8, 9]);
    // Nothing named: the current order comes back untouched.
    assert_eq!(ordered_case_ids(&current, &[]), vec![8, 9, 10]);
}

#[tokio::test]
async fn suite_entries_come_back_sorted_by_sequence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"suiteId": 5, "sequenceNumber": 2, "id": 9, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"},
                {"suiteId": 5, "sequenceNumber": 1, "id": 8, "suiteEntryType": "testCase"}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let entries = client.get_suite_entries("org", "proj", 5).await.unwrap();
    assert_eq!(entries, vec![entry(3, 0, "suite"), entry(8, 1, "testCase"), entry(9, 2, "testCase")]);
}

#[tokio::test]
async fn reorder_sends_cases_after_the_child_suites_as_plain_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"},
                {"suiteId": 5, "sequenceNumber": 1, "id": 8, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 2, "id": 9, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 3, "id": 10, "suiteEntryType": "testCase"}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"},
                {"suiteId": 5, "sequenceNumber": 3, "id": 9, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 1, "id": 10, "suiteEntryType": "testCase"},
                {"suiteId": 5, "sequenceNumber": 2, "id": 8, "suiteEntryType": "testCase"}
            ]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let order = client.reorder_suite_cases("org", "proj", 5, &[10, 8]).await.unwrap();
    // The server's answer, read back in sequence order, child suites left out.
    assert_eq!(order, vec![10, 8, 9]);

    let reqs = server.received_requests().await.unwrap();
    let patch = reqs.iter().find(|r| r.method.as_str() == "PATCH").expect("one PATCH");
    let ct = patch.headers.get("content-type").expect("content-type").to_str().unwrap();
    assert!(ct.starts_with("application/json"), "plain JSON body, got {ct}");
    assert!(!ct.contains("json-patch"), "suiteentry takes an entry array, not json-patch");
    let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
    // One child suite sits at 0, so the cases start at sequence 1; the
    // unnamed case 9 keeps its place after the named ones.
    assert_eq!(
        body,
        serde_json::json!([
            {"id": 10, "sequenceNumber": 1, "suiteEntryType": "testCase"},
            {"id": 8, "sequenceNumber": 2, "suiteEntryType": "testCase"},
            {"id": 9, "sequenceNumber": 3, "suiteEntryType": "testCase"}
        ])
    );
}

#[tokio::test]
async fn reorder_with_no_cases_in_the_suite_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/testplan/suiteentry/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"suiteId": 5, "sequenceNumber": 0, "id": 3, "suiteEntryType": "suite"}]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let order = client.reorder_suite_cases("org", "proj", 5, &[10]).await.unwrap();
    assert!(order.is_empty());
    let patches = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "PATCH")
        .count();
    assert_eq!(patches, 0);
}

#[tokio::test]
async fn create_static_suite_posts_the_documented_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 87,
            "name": "Smoke",
            "suiteType": "staticTestSuite",
            "parentSuite": {"id": 85, "name": "root"}
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let suite = client.create_static_suite("org", "proj", 9, 85, "Smoke").await.unwrap();
    assert_eq!(suite.id, 87);
    assert_eq!(suite.name, "Smoke");
    assert_eq!(suite.suite_type, "staticTestSuite");
    assert_eq!(suite.parent_id, Some(85));
    assert_eq!(suite.requirement_id, None);

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"suiteType": "staticTestSuite", "name": "Smoke", "parentSuite": {"id": 85}})
    );
}

#[tokio::test]
async fn add_test_cases_posts_work_item_ids_and_returns_what_landed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/testplan/Plans/9/Suites/87/TestCase"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"workItem": {"id": 201, "name": "Valid login"}},
                {"workItem": {"id": 202, "name": "Bad password"}}
            ]
        })))
        .mount(&server)
        .await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let added = client
        .add_test_cases_to_suite("org", "proj", 9, 87, &[201, 202])
        .await
        .unwrap();
    assert_eq!(added, vec![201, 202]);

    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(
        body,
        serde_json::json!([{"workItem": {"id": 201}}, {"workItem": {"id": 202}}])
    );
}

#[tokio::test]
async fn add_test_cases_with_nothing_to_add_does_not_call_the_server() {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    let added = client.add_test_cases_to_suite("org", "proj", 9, 87, &[]).await.unwrap();
    assert!(added.is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test suite_manage`
Expected: compile error, `ordered_case_ids` and `SuiteEntry` not found in `v2_lib::ado_testplan`.

- [ ] **Step 3: Add `patch_json` to the transport**

In `src-tauri/src/ado/transport.rs`, directly after `post_json` (which ends around line 237), add:

```rust
    /// PATCH with a plain JSON body. Not a json-patch document: the Test
    /// Plan suiteentry reorder takes an array of entries, and the only
    /// other PATCH helper (`send_json_patch`) sets a content type that
    /// endpoint refuses.
    pub(crate) async fn patch_json(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(reqwest::Method::PATCH, &url, |r| {
                r.header("Accept", "application/json").json(body)
            })
            .await?;
        Self::handle_json(resp).await
    }
```

- [ ] **Step 4: Write the entries module**

Create `src-tauri/src/ado_testplan/entries.rs`:

```rust
//! Suite entries (the order of test cases and child suites inside a
//! suite), static child suites ("folders"), and adding cases to a suite.
//! The Manage Test Cases screen's calls. GET, POST and PATCH only.

use super::SuiteRef;
use crate::ado::{AdoClient, AdoError};
use serde::Serialize;

/// One row of a suite's ordering: a test case or a child suite and the
/// position Azure DevOps shows it at.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SuiteEntry {
    pub id: i32,
    pub sequence_number: i32,
    /// "testCase" or "suite", exactly as Azure DevOps names them.
    pub entry_type: String,
}

/// The order to send: every id in `wanted` that really is a case in the
/// suite, in that order (a repeat counts once, an id the suite does not
/// hold is dropped), then the suite's remaining cases in the order they
/// already had. Child suites are never part of it.
pub fn ordered_case_ids(current: &[SuiteEntry], wanted: &[i32]) -> Vec<i32> {
    let present: Vec<i32> = current
        .iter()
        .filter(|e| e.entry_type == "testCase")
        .map(|e| e.id)
        .collect();
    let mut out: Vec<i32> = Vec::with_capacity(present.len());
    for id in wanted {
        if present.contains(id) && !out.contains(id) {
            out.push(*id);
        }
    }
    for id in present {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn entries_of(data: &serde_json::Value) -> Vec<SuiteEntry> {
    let mut entries: Vec<SuiteEntry> = data["value"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|e| SuiteEntry {
            id: e["id"].as_i64().unwrap_or_default() as i32,
            sequence_number: e["sequenceNumber"].as_i64().unwrap_or_default() as i32,
            entry_type: e["suiteEntryType"].as_str().unwrap_or_default().to_string(),
        })
        .collect();
    entries.sort_by_key(|e| e.sequence_number);
    entries
}

impl AdoClient {
    /// Every entry of a suite, sorted by its sequence number. Read only.
    pub async fn get_suite_entries(
        &self,
        org: &str,
        project: &str,
        suite_id: i32,
    ) -> Result<Vec<SuiteEntry>, AdoError> {
        let url = format!(
            "{}/testplan/suiteentry/{}?api-version=7.1",
            self.tp_base(org, project),
            suite_id
        );
        let data = self.get_json(url).await?;
        Ok(entries_of(&data))
    }

    /// Put the suite's test cases in `case_ids` order. Child suites keep
    /// their places at the top (Azure DevOps lists them first and this
    /// never names them); cases the caller did not name follow the named
    /// ones in the order they already had. Returns the case order as the
    /// server reports it back. A suite with no cases sends nothing.
    pub async fn reorder_suite_cases(
        &self,
        org: &str,
        project: &str,
        suite_id: i32,
        case_ids: &[i32],
    ) -> Result<Vec<i32>, AdoError> {
        let current = self.get_suite_entries(org, project, suite_id).await?;
        let ordered = ordered_case_ids(&current, case_ids);
        if ordered.is_empty() {
            return Ok(vec![]);
        }
        let suites = current.iter().filter(|e| e.entry_type == "suite").count() as i32;
        let body: Vec<serde_json::Value> = ordered
            .iter()
            .enumerate()
            .map(|(i, id)| {
                serde_json::json!({
                    "id": id,
                    "sequenceNumber": suites + i as i32,
                    "suiteEntryType": "testCase",
                })
            })
            .collect();
        let url = format!(
            "{}/testplan/suiteentry/{}?api-version=7.1",
            self.tp_base(org, project),
            suite_id
        );
        let data = self.patch_json(url, &serde_json::Value::Array(body)).await?;
        Ok(entries_of(&data)
            .into_iter()
            .filter(|e| e.entry_type == "testCase")
            .map(|e| e.id)
            .collect())
    }

    /// A static child suite (a "folder") under `parent_suite_id`. Azure
    /// DevOps only allows one under a static suite or the plan root; the
    /// server's refusal for any other parent comes back as the error.
    pub async fn create_static_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        parent_suite_id: i32,
        name: &str,
    ) -> Result<SuiteRef, AdoError> {
        let body = serde_json::json!({
            "suiteType": "staticTestSuite",
            "name": name,
            "parentSuite": {"id": parent_suite_id},
        });
        let url = format!(
            "{}/testplan/Plans/{}/suites?api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.post_json(url, &body).await?;
        Ok(SuiteRef {
            id: data["id"].as_i64().unwrap_or_default() as i32,
            name: data["name"].as_str().unwrap_or(name).to_string(),
            suite_type: data["suiteType"].as_str().unwrap_or("staticTestSuite").to_string(),
            requirement_id: None,
            parent_id: Some(parent_suite_id),
        })
    }

    /// Add existing test cases to a suite. A case can sit in many suites,
    /// so this is a copy: it stays wherever it already was. Returns the
    /// ids the server reports as now in the suite.
    pub async fn add_test_cases_to_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        suite_id: i32,
        case_ids: &[i32],
    ) -> Result<Vec<i32>, AdoError> {
        if case_ids.is_empty() {
            return Ok(vec![]);
        }
        let body: Vec<serde_json::Value> = case_ids
            .iter()
            .map(|id| serde_json::json!({"workItem": {"id": id}}))
            .collect();
        let url = format!(
            "{}/testplan/Plans/{}/Suites/{}/TestCase?api-version=7.1",
            self.tp_base(org, project),
            plan_id,
            suite_id
        );
        let data = self.post_json(url, &serde_json::Value::Array(body)).await?;
        Ok(data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v["workItem"]["id"].as_i64().map(|i| i as i32))
            .collect())
    }
}
```

In `src-tauri/src/ado_testplan/mod.rs` change lines 13-15 to:

```rust
mod entries;
mod history;
mod plans;
mod runs;

pub use entries::{ordered_case_ids, SuiteEntry};
```

(Keep the existing `use crate::ado::AdoClient;` and `use serde::Serialize;` lines that follow.)

- [ ] **Step 5: Run the tests to verify they pass**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test suite_manage`
Expected: 7 passed.

- [ ] **Step 6: Run the full Rust gate**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --tests`
Expected: all pass. (`tests/ado.rs` has a "no DELETE" scan and a request-body guard; neither should trip, since only GET/POST/PATCH are used and no case field is written.)

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/ado/transport.rs src-tauri/src/ado_testplan/entries.rs src-tauri/src/ado_testplan/mod.rs src-tauri/tests/suite_manage.rs
git commit -q -F - <<'EOF'
feat(v2): suite entry order, static child suites and add-to-suite calls

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: Tauri commands and regenerated bindings

**Files:**
- Modify: `src-tauri/src/commands/testplan.rs` (append at end)
- Modify: `src-tauri/src/lib.rs:163` (after `testplan::find_pbi_suite,`)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: Task 1's `AdoClient` methods and `SuiteEntry`; `get_fresh_token(&app)`.
- Produces (TypeScript, via bindings):
  - `commands.listSuiteEntries(organization: string, project: string, suiteId: number) => Result<SuiteEntry[], AdoError>`
  - `commands.reorderSuiteCases(organization: string, project: string, suiteId: number, caseIds: number[]) => Result<number[], AdoError>`
  - `commands.createStaticSuite(organization: string, project: string, planId: number, parentSuiteId: number, name: string) => Result<SuiteRef, AdoError>`
  - `commands.addCasesToSuite(organization: string, project: string, planId: number, suiteId: number, caseIds: number[]) => Result<number[], AdoError>`
  - `export type SuiteEntry = { id: number, sequence_number: number, entry_type: string }`

- [ ] **Step 1: Add the commands**

Append to `src-tauri/src/commands/testplan.rs`:

```rust
/// The order of a suite's entries (child suites first, then test cases),
/// for the Manage Test Cases list. Read only.
#[tauri::command]
#[specta::specta]
pub async fn list_suite_entries(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    suite_id: i32,
) -> Result<Vec<ado_testplan::SuiteEntry>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_suite_entries(&organization, &project, suite_id)
        .await
}

/// Put a suite's test cases in the given order. Cases not named keep
/// their place after the named ones; child suites are not touched.
/// Returns the order the server reports back.
#[tauri::command]
#[specta::specta]
pub async fn reorder_suite_cases(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    suite_id: i32,
    case_ids: Vec<i32>,
) -> Result<Vec<i32>, ado::AdoError> {
    crate::applog::warn(format!(
        "reordering {} test case(s) in {project} suite #{suite_id}: {case_ids:?}",
        case_ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .reorder_suite_cases(&organization, &project, suite_id, &case_ids)
        .await
}

/// A static child suite (a "folder") under a static parent or the plan
/// root. The name is trimmed; an empty one is refused here rather than
/// sent, so the message names the real problem.
#[tauri::command]
#[specta::specta]
pub async fn create_static_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    parent_suite_id: i32,
    name: String,
) -> Result<ado_testplan::SuiteRef, ado::AdoError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ado::AdoError::Http {
            status: 0,
            body: "A folder needs a name.".to_string(),
        });
    }
    crate::applog::warn(format!(
        "creating static suite \"{name}\" in {project} plan #{plan_id} under suite #{parent_suite_id}"
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .create_static_suite(&organization, &project, plan_id, parent_suite_id, &name)
        .await
}

/// Copy existing test cases into a suite: they stay wherever they already
/// were. Returns the ids the server reports as now in the suite.
#[tauri::command]
#[specta::specta]
pub async fn add_cases_to_suite(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    plan_id: i32,
    suite_id: i32,
    case_ids: Vec<i32>,
) -> Result<Vec<i32>, ado::AdoError> {
    if case_ids.is_empty() {
        return Ok(vec![]);
    }
    crate::applog::warn(format!(
        "adding {} test case(s) to {project} plan #{plan_id} suite #{suite_id}: {case_ids:?}",
        case_ids.len()
    ));
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .add_test_cases_to_suite(&organization, &project, plan_id, suite_id, &case_ids)
        .await
}
```

Check that `AdoError::Http { status, body }` is the variant's shape (it is used that way in `src-tauri/src/ado/endpoints.rs:750`). If `status` is not `u16`, match the type used there.

- [ ] **Step 2: Register the commands**

In `src-tauri/src/lib.rs`, after the line `testplan::find_pbi_suite,` (line 163) add:

```rust
            testplan::list_suite_entries,
            testplan::reorder_suite_cases,
            testplan::create_static_suite,
            testplan::add_cases_to_suite,
```

- [ ] **Step 3: Regenerate the bindings and run the bindings test**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --test bindings`
Expected: passes; `src/bindings.ts` now contains `listSuiteEntries`, `reorderSuiteCases`, `createStaticSuite`, `addCasesToSuite` and `export type SuiteEntry`. Confirm with:

```bash
grep -n "listSuiteEntries\|reorderSuiteCases\|createStaticSuite\|addCasesToSuite\|^export type SuiteEntry" src/bindings.ts
```

- [ ] **Step 4: Typecheck the frontend**

From the repo root: `npx tsc --noEmit`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/testplan.rs src-tauri/src/lib.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): commands for suite entry order, folders and add-to-suite

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: Shared tree helpers, the `manage` section, and a screen that picks a suite

**Files:**
- Create: `src/lib/suiteTree.ts`, `src/lib/suiteTree.test.ts`
- Modify: `src/screens/Suites.tsx:26-41` (use the shared `buildTree`)
- Modify: `src/components/Sidebar.tsx` (type at line 29, flag at 49, `CASE_ITEMS` at 51-64, filter at 97-99, lucide import at 1-15)
- Modify: `src/App.tsx` (import at 60, `TITLES` 141-150, `TITLE_NOTES` 154-156, shortcut order 295-302, route after 1078)
- Modify: `src/index.css:788` (after `.nav-ico-ai`)
- Create: `src/screens/ManageCases/SuitePicker.tsx`, `src/screens/ManageCases/index.tsx`, `src/screens/ManageCases/SuitePicker.test.tsx`
- Modify: `src/components/Sidebar.test.tsx`, `src/App.test.tsx:168-186`

**Interfaces:**
- Consumes: `commands.listPlansWithSuites`, `persistentQuery` + `CACHE.structure` from `src/lib/persistentQuery`, `Select` from `src/components/ui/select` (props: `value`, `onChange(e => e.target.value)`, `aria-label`, `<option>` children), `SuiteRef`, `PlanWithSuites`, `TestPlan` from bindings.
- Produces:
  - `src/lib/suiteTree.ts`: `type SuiteNode = { suite: SuiteRef; children: SuiteNode[] }`, `buildTree(suites: SuiteRef[]): SuiteNode[]`, `flattenTree(nodes: SuiteNode[]): Array<{ suite: SuiteRef; depth: number }>`.
  - `export type PickedSuite = { planId: number; planName: string; rootSuiteId: number | null; suite: SuiteRef; siblings: SuiteRef[] }` in `SuitePicker.tsx` (`siblings` = every suite of that plan, flat, for folder targets in Task 6).
  - `SuitePicker` props: `{ org: string; project: string; picked: PickedSuite | null; onPick: (p: PickedSuite | null) => void }`.
  - `ManageCases` default export, props `{ org: string; project: string }`.
  - `MANAGE_CASES_ENABLED` exported from `Sidebar.tsx`.

- [ ] **Step 1: Write the failing tree-helper test**

Create `src/lib/suiteTree.test.ts`:

```ts
import { describe, expect, test } from "vitest";
import type { SuiteRef } from "../bindings";
import { buildTree, flattenTree } from "./suiteTree";

const s = (id: number, parent_id: number | null, name = `S${id}`): SuiteRef => ({
  id,
  name,
  suite_type: "staticTestSuite",
  requirement_id: null,
  parent_id,
});

describe("buildTree", () => {
  test("nests by parent id; unknown parents become roots", () => {
    const roots = buildTree([s(1, null), s(2, 1), s(3, 2), s(4, 99)]);
    expect(roots.map((r) => r.suite.id)).toEqual([1, 4]);
    expect(roots[0].children[0].suite.id).toBe(2);
    expect(roots[0].children[0].children[0].suite.id).toBe(3);
  });
});

describe("flattenTree", () => {
  test("walks depth-first with the depth of each row", () => {
    const rows = flattenTree(buildTree([s(1, null), s(2, 1), s(3, 2), s(4, null)]));
    expect(rows.map((r) => [r.suite.id, r.depth])).toEqual([
      [1, 0],
      [2, 1],
      [3, 2],
      [4, 0],
    ]);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

`npx vitest run src/lib/suiteTree.test.ts`
Expected: FAIL, cannot resolve `./suiteTree`.

- [ ] **Step 3: Write the helper and point Suites at it**

Create `src/lib/suiteTree.ts`:

```ts
import type { SuiteRef } from "../bindings";

export type SuiteNode = { suite: SuiteRef; children: SuiteNode[] };

/** Rebuild the multi-level suite tree from parent links. The plan's root
 * suite is already stripped in Rust, so a null (or unknown) parent means
 * top level. Shared by the Test Suites browser and the Manage Test Cases
 * picker so both draw the same tree. */
export function buildTree(suites: SuiteRef[]): SuiteNode[] {
  const nodes = new Map<number, SuiteNode>();
  for (const s of suites) nodes.set(s.id, { suite: s, children: [] });
  const roots: SuiteNode[] = [];
  for (const n of nodes.values()) {
    const pid = n.suite.parent_id;
    if (pid != null && nodes.has(pid)) nodes.get(pid)!.children.push(n);
    else roots.push(n);
  }
  return roots;
}

/** Depth-first rows for a dropdown: each suite with how deep it sits, so
 * an option can be indented to read like the tree it came from. */
export function flattenTree(nodes: SuiteNode[]): Array<{ suite: SuiteRef; depth: number }> {
  const out: Array<{ suite: SuiteRef; depth: number }> = [];
  const walk = (list: SuiteNode[], depth: number) => {
    for (const n of list) {
      out.push({ suite: n.suite, depth });
      walk(n.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return out;
}
```

In `src/screens/Suites.tsx`: delete the local `type SuiteNode` and `function buildTree` (lines 26-41) and add to the imports:

```ts
import { buildTree, type SuiteNode } from "../lib/suiteTree";
```

`descendantIds` and `pruneTree` below it keep using `SuiteNode` unchanged.

- [ ] **Step 4: Run the helper test and the Suites tests**

`npx vitest run src/lib/suiteTree.test.ts src/screens/Suites.test.tsx`
Expected: all pass.

- [ ] **Step 5: Add the section to the sidebar**

In `src/components/Sidebar.tsx`:

Line 29 becomes:

```ts
export type Section = "manual" | "import" | "edit" | "view" | "run" | "autorun" | "suites" | "manage" | "ai" | "settings";
```

Add `ListOrdered` to the `lucide-react` import list (alphabetical, between `KanbanSquare` and `PenLine`).

After the `AUTO_RUN_ENABLED` declaration (line 49) add:

```ts
/** Manage Test Cases ships in development builds only until it has been
 * tried against a real project: the same `DEV` gate as Auto Run, so the
 * row exists for `tauri dev` and vitest and not for `tauri build`. */
export const MANAGE_CASES_ENABLED: boolean = import.meta.env.DEV;
```

In `CASE_ITEMS`, after the `suites` row add:

```ts
  // An ordered list, because ordering is the first thing this screen does.
  { id: "manage", label: "Manage Test Cases", icon: ListOrdered, tone: "nav-ico nav-ico-manage", note: "In Dev" },
```

The filter at line 97-99 becomes:

```ts
  const list =
    items ??
    (CASE_ITEMS.filter(
      (i) => (i.id !== "autorun" || AUTO_RUN_ENABLED) && (i.id !== "manage" || MANAGE_CASES_ENABLED),
    ) as unknown as Item<T>[]);
```

In `src/index.css`, after line 788 (`.group:hover .nav-ico-ai …`) add:

```css
.group:hover .nav-ico-manage { animation: ico-shuffle 0.5s ease; }
```

- [ ] **Step 6: Wire App**

In `src/App.tsx`:

Line 60 import becomes:

```ts
import Sidebar, { AUTO_RUN_ENABLED, MANAGE_CASES_ENABLED, WORK_ITEMS, type Section, type WorkSection } from "./components/Sidebar";
```

Add the screen import next to the other screen imports (find `import Suites from "./screens/Suites";`):

```ts
import ManageCases from "./screens/ManageCases";
```

In `TITLES` add after `suites: "Test Suites",`:

```ts
  manage: "Manage Test Cases",
```

`TITLE_NOTES` becomes:

```ts
const TITLE_NOTES: Partial<Record<Section, string>> = {
  autorun: "In Development",
  manage: "In Development",
};
```

The shortcut comment and order (lines 295-302) become:

```ts
  // Keyboard shortcuts: Ctrl+1..9 = tabs, Ctrl+Shift+M = Work Manager
  // (v1's binding). Ctrl+K (palette) is registered in CommandPalette.
  useEffect(() => {
    // Mirrors the sidebar's rows: without the dev-only rows (release
    // builds) the numbers close up, so Ctrl+6 is Test Suites there and
    // Auto Run here.
    const order: Section[] = (
      ["manual", "import", "edit", "view", "run", "autorun", "suites", "manage", "ai"] as Section[]
    ).filter((s) => (s !== "autorun" || AUTO_RUN_ENABLED) && (s !== "manage" || MANAGE_CASES_ENABLED));
```

After the `AUTO_RUN_ENABLED && section === "autorun"` block (line 1079-1081) add:

```tsx
                {MANAGE_CASES_ENABLED && section === "manage" && (
                  <ManageCases org={org} project={project} />
                )}
```

- [ ] **Step 7: Write the failing picker test**

Create `src/screens/ManageCases/SuitePicker.test.tsx`:

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const PLANS = [
  {
    plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 92, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 },
      { id: 93, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
    ],
  },
  {
    plan: { id: 10, name: "Billing - Test Plan", area_path: "Proj\\Billing", root_suite_id: 100 },
    suites: [
      { id: 101, name: "Invoices", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
    ],
  },
];

function Harness({ onPick }: { onPick: (p: PickedSuite | null) => void }) {
  const [picked, setPicked] = useState<PickedSuite | null>(null);
  return (
    <SuitePicker
      org="acme"
      project="Web"
      picked={picked}
      onPick={(p) => {
        setPicked(p);
        onPick(p);
      }}
    />
  );
}

function renderPicker(onPick: (p: PickedSuite | null) => void = () => {}) {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness onPick={onPick} />
    </QueryClientProvider>,
  );
}

test("picking a plan lists its suites indented by depth; picking a suite reports it", async () => {
  const picks: Array<PickedSuite | null> = [];
  renderPicker((p) => picks.push(p));

  const plan = await screen.findByLabelText("Test plan");
  expect(plan).toHaveValue("");
  fireEvent.change(plan, { target: { value: "9" } });

  const suite = screen.getByLabelText("Test suite");
  const labels = Array.from(suite.querySelectorAll("option")).map((o) => o.textContent);
  // Depth-first, the child indented under its parent, PBI suites tagged.
  expect(labels).toEqual(["Pick a suite", "Regression", "\u00a0\u00a0\u00a0\u00a0Smoke", "PBI 42: PBI 42 suite"]);

  fireEvent.change(suite, { target: { value: "92" } });
  expect(picks.at(-1)).toMatchObject({
    planId: 9,
    planName: "Auth - Test Plan",
    rootSuiteId: 90,
    suite: { id: 92, name: "Smoke" },
  });
  expect(picks.at(-1)!.siblings.map((s) => s.id)).toEqual([91, 92, 93]);
});

test("changing the plan clears the picked suite", async () => {
  const picks: Array<PickedSuite | null> = [];
  renderPicker((p) => picks.push(p));
  const plan = await screen.findByLabelText("Test plan");
  fireEvent.change(plan, { target: { value: "9" } });
  fireEvent.change(screen.getByLabelText("Test suite"), { target: { value: "91" } });
  fireEvent.change(plan, { target: { value: "10" } });
  expect(picks.at(-1)).toBeNull();
  expect(screen.getByLabelText("Test suite")).toHaveValue("");
});
```

Note: the `Select` component in `src/components/ui/select.tsx` renders a themed trigger + listbox over its `<option>` children. Read its file before writing the picker: if its DOM does not expose a native `<select>` that `getByLabelText` + `fireEvent.change` can drive (check how `src/screens/AutoRun/RunPane.test.tsx` drives it), use plain `<select>` elements in `SuitePicker` styled with the same classes the `Select` trigger uses, so the test above holds as written. Either way the two controls must have accessible names "Test plan" and "Test suite".

- [ ] **Step 8: Run it to verify it fails**

`npx vitest run src/screens/ManageCases/SuitePicker.test.tsx`
Expected: FAIL, cannot resolve `./SuitePicker`.

- [ ] **Step 9: Write the picker and the screen shell**

Create `src/screens/ManageCases/SuitePicker.tsx`:

```tsx
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { commands, type SuiteRef } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { CACHE, persistentQuery } from "../../lib/persistentQuery";
import { buildTree, flattenTree } from "../../lib/suiteTree";
import { unwrap } from "../../lib/ipc";

/** What the rest of the screen needs to know about the suite in hand. */
export type PickedSuite = {
  planId: number;
  planName: string;
  rootSuiteId: number | null;
  suite: SuiteRef;
  /** Every suite of the plan, flat: folder targets come from here. */
  siblings: SuiteRef[];
};

const INDENT = "\u00a0\u00a0\u00a0\u00a0";

/** Two dropdowns over the same cached plan tree the Test Suites tab
 * paints: plan, then suite (indented by depth so it reads as the tree).
 * Kept as native selects: the list can run to hundreds of suites, and a
 * native control scrolls, types-to-find and reads to a screen reader
 * without any help. */
export default function SuitePicker({
  org,
  project,
  picked,
  onPick,
}: {
  org: string;
  project: string;
  picked: PickedSuite | null;
  onPick: (p: PickedSuite | null) => void;
}) {
  const [planId, setPlanId] = useState<string>(picked ? String(picked.planId) : "");
  const plans = useQuery({
    queryKey: ["plans-suites", org, project],
    ...persistentQuery({
      key: `plans-suites:${org}/${project}`,
      fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
      ...CACHE.structure,
    }),
    enabled: Boolean(org && project),
    gcTime: 60 * 60_000,
    retry: false,
  });

  const current = useMemo(
    () => (plans.data ?? []).find((p) => String(p.plan.id) === planId) ?? null,
    [plans.data, planId],
  );
  const rows = useMemo(() => (current ? flattenTree(buildTree(current.suites)) : []), [current]);

  const selectClass =
    "w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text focus:border-accent focus:outline-none disabled:opacity-50";

  return (
    <div className="grid gap-3 md:grid-cols-2">
      <label className="block text-xs text-muted">
        Test plan
        <select
          aria-label="Test plan"
          className={`mt-1 ${selectClass}`}
          value={planId}
          disabled={!plans.data}
          onChange={(e) => {
            setPlanId(e.target.value);
            onPick(null);
          }}
        >
          <option value="">{plans.data ? "Pick a plan" : "Loading plans"}</option>
          {(plans.data ?? []).map(({ plan }) => (
            <option key={plan.id} value={plan.id}>
              {plan.name}
            </option>
          ))}
        </select>
      </label>
      <label className="block text-xs text-muted">
        Test suite
        <select
          aria-label="Test suite"
          className={`mt-1 ${selectClass}`}
          value={picked ? String(picked.suite.id) : ""}
          disabled={!current}
          onChange={(e) => {
            const suite = current?.suites.find((s) => String(s.id) === e.target.value);
            if (!current || !suite) {
              onPick(null);
              return;
            }
            onPick({
              planId: current.plan.id,
              planName: current.plan.name,
              rootSuiteId: current.plan.root_suite_id,
              suite,
              siblings: current.suites,
            });
          }}
        >
          <option value="">Pick a suite</option>
          {rows.map(({ suite, depth }) => (
            <option key={suite.id} value={suite.id}>
              {INDENT.repeat(depth)}
              {suite.suite_type === "requirementTestSuite" && suite.requirement_id != null
                ? `PBI ${suite.requirement_id}: `
                : ""}
              {suite.name}
            </option>
          ))}
        </select>
      </label>
      {plans.isFetching && !plans.data && (
        <div className="md:col-span-2">
          <ScanProgress label="Loading test plans" />
        </div>
      )}
      {plans.isError && <p className="text-sm text-danger md:col-span-2">{plans.error.message}</p>}
    </div>
  );
}
```

Create `src/screens/ManageCases/index.tsx` (the shell; Task 4 fills the list in):

```tsx
import { useState } from "react";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

/** Bulk operations on one suite's test cases: re-order them, move them to
 * another PBI, copy them into folders. The suite comes from the plan tree;
 * everything below the picker works on that one suite. */
export default function ManageCases({ org, project }: { org: string; project: string }) {
  const [picked, setPicked] = useState<PickedSuite | null>(null);

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <SuitePicker org={org} project={project} picked={picked} onPick={setPicked} />
      {!picked && (
        <p className="text-sm text-muted">
          Pick a test plan and a suite. Its test cases appear here in the order Azure DevOps shows them.
        </p>
      )}
      {picked && (
        <p className="text-sm text-muted">
          {picked.planName} / {picked.suite.name}
        </p>
      )}
    </div>
  );
}
```

- [ ] **Step 10: Update the sidebar and App tests**

In `src/components/Sidebar.test.tsx`, after the Auto Run dev/release test add:

```tsx
/// Manage Test Cases is on the same development-only gate as Auto Run.
test("Manage Test Cases is offered in dev builds and hidden in release builds", async () => {
  const { vi } = await import("vitest");

  vi.stubEnv("DEV", true);
  vi.resetModules();
  const dev = await import("./Sidebar");
  const { unmount } = render(<dev.default section="manual" onSelect={() => {}} />);
  expect(screen.getByRole("button", { name: /Manage Test Cases/ })).toBeInTheDocument();
  unmount();

  vi.stubEnv("DEV", false);
  vi.resetModules();
  const release = await import("./Sidebar");
  render(<release.default section="manual" onSelect={() => {}} />);
  expect(screen.queryByRole("button", { name: /Manage Test Cases/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "AI Bridge" })).toBeInTheDocument();

  vi.unstubAllEnvs();
  vi.resetModules();
});
```

If any existing test asserts the "In Dev" pill with `getByText("In Dev")`, change it to `getAllByText("In Dev")` with a length check, since two rows now carry the pill.

In `src/App.test.tsx`, replace the test at lines 168-186 with:

```tsx
// The sidebar's CASE_ITEMS order is manual, import, edit, view, run,
// autorun, suites, manage, ai (9 rows) - the Ctrl+N shortcut order must
// match it row for row, or a number opens the wrong screen and the last
// row loses its shortcut entirely.
test("Ctrl+6 jumps to Auto Run, Ctrl+8 to Manage Test Cases and Ctrl+9 to AI Bridge", async () => {
  signedInMocks();
  renderApp();
  await screen.findByText("a@b.com");

  fireEvent.keyDown(window, { key: "6", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "Auto Run" })).toBeInTheDocument();
  // Shipped early, and the screen says so - beside the heading, not in it.
  expect(screen.getByText("In Development")).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "7", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "Test Suites" })).toBeInTheDocument();
  // The pill belongs to the dev-only tabs alone.
  expect(screen.queryByText("In Development")).not.toBeInTheDocument();

  fireEvent.keyDown(window, { key: "8", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "Manage Test Cases" })).toBeInTheDocument();
  expect(screen.getByText("In Development")).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "9", ctrlKey: true });
  expect(screen.getByRole("heading", { name: "AI Bridge" })).toBeInTheDocument();
});
```

Check `src/App.test.tsx:82` (`for (const tab of ["Import File", "Update Test Cases", "Run Tests", "Test Suites"])`): it clicks tabs by name and needs no change. Search the file for `"8"` and `AI Bridge` for any other shortcut assertion and update it to `"9"` if found.

- [ ] **Step 11: Run the affected tests, then the gates**

`npx vitest run src/screens/ManageCases src/components/Sidebar.test.tsx src/App.test.tsx src/lib/suiteTree.test.ts src/screens/Suites.test.tsx src/ui-consistency.test.ts`
Expected: all pass (App.test has a documented load flake; one failure that passes on re-run is that).

Then `npx tsc --noEmit` (clean), then `npx vitest run` (all pass).

- [ ] **Step 12: Commit**

```bash
git add src/lib/suiteTree.ts src/lib/suiteTree.test.ts src/screens/Suites.tsx src/components/Sidebar.tsx src/components/Sidebar.test.tsx src/App.tsx src/App.test.tsx src/index.css src/screens/ManageCases
git commit -q -F - <<'EOF'
feat(v2): Manage Test Cases section (dev builds) with a plan and suite picker

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: The orderable case list with drag and drop, and "Apply order"

**Files:**
- Create: `src/lib/suiteOrder.ts`, `src/lib/suiteOrder.test.ts`
- Modify: `src/lib/actionIcons.ts` (add `IconMoveUp`, `IconMoveDown` in the "Making and changing things" group)
- Create: `src/screens/ManageCases/CaseOrderList.tsx`
- Modify: `src/screens/ManageCases/index.tsx`
- Create: `src/screens/ManageCases/testSupport.tsx`, `src/screens/ManageCases/index.test.tsx`

**Interfaces:**
- Consumes: `commands.listSuiteEntries`, `commands.listTestPoints`, `commands.reorderSuiteCases` (Task 2), `PickedSuite` (Task 3), `Checkbox` from `src/components/ui/checkbox` (`checked`, `onCheckedChange`, `ariaLabel`), `Button` from `src/components/ui/button` (`variant`, `size="sm"`).
- Produces:
  - `src/lib/suiteOrder.ts`: `type SuiteCase = { id: number; title: string }`, `moveItem<T>(list: T[], from: number, to: number): T[]`, `sameOrder(a: SuiteCase[], b: SuiteCase[]): boolean`.
  - `CaseOrderList` props: `{ cases: SuiteCase[]; selected: Set<number>; onChange: (next: SuiteCase[]) => void; onSelect: (next: Set<number>) => void; disabled?: boolean }`.
  - In `index.tsx`: the query key `["suite-cases", org, project, planId, suiteId]` and `loadSuiteCases(org, project, planId, suiteId): Promise<SuiteCase[]>`; `order`/`selected` state that Tasks 5 and 6 read.

- [ ] **Step 1: Write the failing helper tests**

Create `src/lib/suiteOrder.test.ts`:

```ts
import { describe, expect, test } from "vitest";
import { moveItem, sameOrder } from "./suiteOrder";

describe("moveItem", () => {
  test("moves an item to a new index and returns a new array", () => {
    const list = ["a", "b", "c", "d"];
    expect(moveItem(list, 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(moveItem(list, 3, 0)).toEqual(["d", "a", "b", "c"]);
    expect(list).toEqual(["a", "b", "c", "d"]);
  });
  test("out-of-range or same index leaves the order alone", () => {
    const list = ["a", "b"];
    expect(moveItem(list, 1, 1)).toEqual(["a", "b"]);
    expect(moveItem(list, 5, 0)).toEqual(["a", "b"]);
    expect(moveItem(list, 0, -1)).toEqual(["a", "b"]);
  });
});

describe("sameOrder", () => {
  test("compares by id sequence only", () => {
    const a = [{ id: 1, title: "x" }, { id: 2, title: "y" }];
    expect(sameOrder(a, [{ id: 1, title: "other" }, { id: 2, title: "y" }])).toBe(true);
    expect(sameOrder(a, [{ id: 2, title: "y" }, { id: 1, title: "x" }])).toBe(false);
    expect(sameOrder(a, [{ id: 1, title: "x" }])).toBe(false);
  });
});
```

- [ ] **Step 2: Run to verify failure**

`npx vitest run src/lib/suiteOrder.test.ts`
Expected: FAIL, cannot resolve `./suiteOrder`.

- [ ] **Step 3: Write the helpers**

Create `src/lib/suiteOrder.ts`:

```ts
/** One test case as the Manage Test Cases list shows it. */
export type SuiteCase = { id: number; title: string };

/** A copy of `list` with the item at `from` moved to `to`. Anything out of
 * range, or a move onto itself, returns an equal copy. */
export function moveItem<T>(list: T[], from: number, to: number): T[] {
  const out = list.slice();
  if (from === to || from < 0 || to < 0 || from >= out.length || to >= out.length) return out;
  const [item] = out.splice(from, 1);
  out.splice(to, 0, item);
  return out;
}

/** True when both lists carry the same ids in the same order. Titles do
 * not matter: the order is what gets saved. */
export function sameOrder(a: SuiteCase[], b: SuiteCase[]): boolean {
  return a.length === b.length && a.every((c, i) => c.id === b[i].id);
}
```

Run `npx vitest run src/lib/suiteOrder.test.ts`. Expected: pass.

- [ ] **Step 4: Add the two icons**

In `src/lib/actionIcons.ts`, in the "Making and changing things" group after `Pin as IconSetDefault,` add:

```ts
  // One step up or down in an ordered list: the keyboard's route to the
  // same move a drag makes.
  ArrowUp as IconMoveUp,
  ArrowDown as IconMoveDown,
```

- [ ] **Step 5: Write the test support file and the failing screen test**

Create `src/screens/ManageCases/testSupport.tsx` (shared by the three ManageCases test files; it registers no tests of its own):

```tsx
import { mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import ManageCases from "./index";

export const PLANS = [
  {
    plan: { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 },
    suites: [
      { id: 91, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
      { id: 93, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
    ],
  },
];

export const ENTRIES = [
  { id: 95, sequence_number: 0, entry_type: "suite" },
  { id: 201, sequence_number: 1, entry_type: "testCase" },
  { id: 202, sequence_number: 2, entry_type: "testCase" },
  { id: 203, sequence_number: 3, entry_type: "testCase" },
];

export const point = (id: number, name: string, config = "Windows 10") => ({
  point_id: id * 10,
  test_case_id: id,
  test_case_name: name,
  config_name: config,
  tester: "",
  last_outcome: "none",
  last_run_id: null,
  last_result_id: null,
});

/** Mount the screen over a plan with one static suite (91) and one PBI
 * suite (93), both holding cases 201, 202, 203. `extra` answers any other
 * command. Returns every IPC call for assertions. */
export function mountWithSuite(extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "list_suite_entries") return ENTRIES;
    if (cmd === "list_test_points")
      return [
        point(201, "Valid login"),
        point(201, "Valid login", "Windows 11"),
        point(202, "Bad password"),
        point(203, "Locked out"),
      ];
    return extra(cmd, args);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ManageCases org="acme" project="Web" />
    </QueryClientProvider>,
  );
  return { calls };
}

/** Pick plan 9 and the given suite; resolves to the case list element. */
export async function pickSuite(suiteId: number) {
  const plan = await screen.findByLabelText("Test plan");
  fireEvent.change(plan, { target: { value: "9" } });
  fireEvent.change(screen.getByLabelText("Test suite"), { target: { value: String(suiteId) } });
  return screen.findByRole("list", { name: "Test cases in order" });
}
```

Create `src/screens/ManageCases/index.test.tsx`:

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ManageCases from "./index";
import { PLANS, mountWithSuite, pickSuite } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

test("the list shows the suite's cases once each, in entry order, with positions", async () => {
  mountWithSuite();
  const l = await pickSuite(91);
  const rows = within(l).getAllByRole("listitem");
  expect(rows).toHaveLength(3);
  expect(rows[0]).toHaveTextContent("1");
  expect(rows[0]).toHaveTextContent("#201");
  expect(rows[0]).toHaveTextContent("Valid login");
  expect(rows[1]).toHaveTextContent("#202");
  expect(rows[2]).toHaveTextContent("#203");
  // The child suite entry (95) is not a case and does not appear.
  expect(within(l).queryByText(/#95/)).not.toBeInTheDocument();
  // Nothing has moved: nothing to apply.
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("dragging a row onto another moves it; Apply order sends the ids and reloads", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "reorder_suite_cases") return [203, 201, 202];
  });
  const l = await pickSuite(91);
  const rows = within(l).getAllByRole("listitem");

  fireEvent.dragStart(rows[2]);
  fireEvent.dragOver(rows[0]);
  fireEvent.drop(rows[0]);

  const after = within(l).getAllByRole("listitem");
  expect(after[0]).toHaveTextContent("#203");
  expect(after[1]).toHaveTextContent("#201");
  expect(after[2]).toHaveTextContent("#202");
  const apply = screen.getByRole("button", { name: "Apply order" });
  expect(apply).toBeEnabled();
  fireEvent.click(apply);

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "reorder_suite_cases");
    expect(call?.args).toEqual({ organization: "acme", project: "Web", suiteId: 91, caseIds: [203, 201, 202] });
  });
  // The list re-reads from the server after a save.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_suite_entries").length).toBeGreaterThan(1));
});

test("Move up and Move down step a row one place; Reset returns to the server order", async () => {
  mountWithSuite();
  const l = await pickSuite(91);
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 up" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#202");
  fireEvent.click(within(l).getByRole("button", { name: "Move #202 down" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  fireEvent.click(within(l).getByRole("button", { name: "Move #201 down" }));
  expect(screen.getByRole("button", { name: "Apply order" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Reset" }));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("an empty suite says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_plans_with_suites") return PLANS;
    if (cmd === "list_suite_entries") return [];
    if (cmd === "list_test_points") return [];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ManageCases org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const plan = await screen.findByLabelText("Test plan");
  fireEvent.change(plan, { target: { value: "9" } });
  fireEvent.change(screen.getByLabelText("Test suite"), { target: { value: "91" } });
  expect(await screen.findByText("No test cases in this suite.")).toBeInTheDocument();
});
```

- [ ] **Step 6: Run to verify failure**

`npx vitest run src/screens/ManageCases/index.test.tsx`
Expected: FAIL, no list named "Test cases in order".

- [ ] **Step 7: Write the list component**

Create `src/screens/ManageCases/CaseOrderList.tsx`:

```tsx
import { GripVertical } from "lucide-react";
import { useState } from "react";
import { Checkbox } from "../../components/ui/checkbox";
import { IconMoveDown, IconMoveUp } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { moveItem, type SuiteCase } from "../../lib/suiteOrder";

/** The suite's cases in their current order. Drag a row onto another to
 * put it there; the arrow buttons do the same one step at a time (and are
 * what a keyboard user gets). The checkbox picks rows for the bulk
 * actions; it has nothing to do with order. Native drag events, no
 * library: the app's board already works this way. */
export default function CaseOrderList({
  cases,
  selected,
  onChange,
  onSelect,
  disabled = false,
}: {
  cases: SuiteCase[];
  selected: Set<number>;
  onChange: (next: SuiteCase[]) => void;
  onSelect: (next: Set<number>) => void;
  disabled?: boolean;
}) {
  const [dragId, setDragId] = useState<number | null>(null);
  const [overId, setOverId] = useState<number | null>(null);

  const indexOf = (id: number) => cases.findIndex((c) => c.id === id);
  const dropOn = (targetId: number) => {
    if (dragId == null || dragId === targetId) return;
    onChange(moveItem(cases, indexOf(dragId), indexOf(targetId)));
  };
  const toggle = (id: number, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(id);
    else next.delete(id);
    onSelect(next);
  };
  const allOn = cases.length > 0 && cases.every((c) => selected.has(c.id));
  const someOn = cases.some((c) => selected.has(c.id));

  return (
    <div className="rounded-md border border-border bg-surface">
      <div className="flex items-center gap-3 border-b border-border px-3 py-2 text-xs text-muted">
        <Checkbox
          checked={allOn}
          indeterminate={!allOn && someOn}
          onCheckedChange={(on) => onSelect(on ? new Set(cases.map((c) => c.id)) : new Set())}
          ariaLabel="Select all test cases"
        />
        <span>
          {selected.size > 0 ? `${selected.size} of ${cases.length} selected` : `${cases.length} test cases`}
        </span>
      </div>
      <ol aria-label="Test cases in order" className="divide-y divide-border">
        {cases.map((c, i) => (
          <li
            key={c.id}
            draggable={!disabled}
            onDragStart={() => setDragId(c.id)}
            onDragEnd={() => {
              setDragId(null);
              setOverId(null);
            }}
            onDragOver={(e) => {
              e.preventDefault();
              if (overId !== c.id) setOverId(c.id);
            }}
            onDragLeave={() => setOverId((o) => (o === c.id ? null : o))}
            onDrop={(e) => {
              e.preventDefault();
              dropOn(c.id);
              setDragId(null);
              setOverId(null);
            }}
            className={cn(
              "flex items-center gap-3 px-3 py-2 text-sm",
              !disabled && "cursor-grab hover:bg-surface-2",
              dragId === c.id && "opacity-50",
              overId === c.id && dragId !== c.id && "border-t-2 border-accent",
            )}
          >
            <GripVertical size={14} className="shrink-0 text-faint" aria-hidden />
            <span className="id-mono w-8 shrink-0 text-right text-faint">{i + 1}</span>
            <Checkbox
              checked={selected.has(c.id)}
              onCheckedChange={(on) => toggle(c.id, on)}
              ariaLabel={`Select #${c.id}`}
            />
            <span className="id-mono shrink-0 text-faint">#{c.id}</span>
            <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
            <span className="flex shrink-0 items-center gap-1">
              <button
                type="button"
                aria-label={`Move #${c.id} up`}
                title="Move up"
                disabled={disabled || i === 0}
                className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                onClick={() => onChange(moveItem(cases, i, i - 1))}
              >
                <IconMoveUp aria-hidden />
              </button>
              <button
                type="button"
                aria-label={`Move #${c.id} down`}
                title="Move down"
                disabled={disabled || i === cases.length - 1}
                className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                onClick={() => onChange(moveItem(cases, i, i + 1))}
              >
                <IconMoveDown aria-hidden />
              </button>
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}
```

- [ ] **Step 8: Fill the screen in**

Replace `src/screens/ManageCases/index.tsx` with:

```tsx
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { IconConfirm, IconUndo } from "../../lib/actionIcons";
import { unwrap } from "../../lib/ipc";
import { sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "./CaseOrderList";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

/** The suite's cases in Azure DevOps' own order. The entries carry the
 * order and the points carry the names; a case with several
 * configurations has several points and one row. */
export async function loadSuiteCases(
  org: string,
  project: string,
  planId: number,
  suiteId: number,
): Promise<SuiteCase[]> {
  const [entries, points] = await Promise.all([
    unwrap(commands.listSuiteEntries(org, project, suiteId)),
    unwrap(commands.listTestPoints(org, project, planId, suiteId)),
  ]);
  const names = new Map<number, string>();
  for (const p of points) {
    if (p.test_case_id != null && !names.has(p.test_case_id)) names.set(p.test_case_id, p.test_case_name);
  }
  return entries
    .filter((e) => e.entry_type === "testCase")
    .map((e) => ({ id: e.id, title: names.get(e.id) ?? `Test case ${e.id}` }));
}

/** Bulk operations on one suite's test cases: re-order them, move them to
 * another PBI, copy them into folders. The suite comes from the plan tree;
 * everything below the picker works on that one suite. */
export default function ManageCases({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [picked, setPicked] = useState<PickedSuite | null>(null);
  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const planId = picked?.planId ?? 0;
  const suiteId = picked?.suite.id ?? 0;
  const cases = useQuery({
    queryKey: ["suite-cases", org, project, planId, suiteId],
    queryFn: () => loadSuiteCases(org, project, planId, suiteId),
    enabled: Boolean(picked),
    retry: false,
  });

  // A fresh read (new suite, or a reload after a save) replaces the
  // working order and clears the selection: rows may have gone.
  useEffect(() => {
    if (cases.data) {
      setOrder(cases.data);
      setSelected(new Set());
    }
  }, [cases.data]);

  const dirty = cases.data ? !sameOrder(order, cases.data) : false;

  const apply = useMutation({
    mutationFn: () => unwrap(commands.reorderSuiteCases(org, project, suiteId, order.map((c) => c.id))),
    onSuccess: () => {
      toast.success("Order saved.");
      qc.invalidateQueries({ queryKey: ["suite-cases", org, project, planId, suiteId] });
    },
    onError: (e) => toast.error(`Could not save the order: ${e.message}`),
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  const busy = apply.isPending;

  return (
    <div className="space-y-4">
      <SuitePicker org={org} project={project} picked={picked} onPick={setPicked} />
      {!picked && (
        <p className="text-sm text-muted">
          Pick a test plan and a suite. Its test cases appear here in the order Azure DevOps shows them.
        </p>
      )}
      {picked && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
              <IconConfirm aria-hidden />
              {apply.isPending ? "Saving" : "Apply order"}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!dirty || busy}
              onClick={() => cases.data && setOrder(cases.data)}
            >
              <IconUndo aria-hidden />
              Reset
            </Button>
          </div>
          {cases.isLoading && <ScanProgress label="Loading test cases" />}
          {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
          {cases.data && cases.data.length === 0 && (
            <p className="text-sm text-muted">No test cases in this suite.</p>
          )}
          {cases.data && cases.data.length > 0 && (
            <CaseOrderList
              cases={order}
              selected={selected}
              onChange={setOrder}
              onSelect={setSelected}
              disabled={busy}
            />
          )}
        </>
      )}
    </div>
  );
}
```

Check `Button`'s `variant` names in `src/components/ui/button.tsx` (line 12-19); use `"ghost"` if it exists (RelinkDialog uses it), otherwise the secondary variant defined there.

- [ ] **Step 9: Run the screen tests, then the gates**

`npx vitest run src/screens/ManageCases src/lib/suiteOrder.test.ts src/ui-consistency.test.ts`
Expected: all pass. Then `npx tsc --noEmit` (clean), then `npx vitest run` (all pass).

- [ ] **Step 10: Commit**

```bash
git add src/lib/suiteOrder.ts src/lib/suiteOrder.test.ts src/lib/actionIcons.ts src/screens/ManageCases
git commit -q -F - <<'EOF'
feat(v2): Manage Test Cases lists a suite's cases, drag to re-order, Apply saves

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 5: "Apply tester order from file" and "Move to PBI"

**Files:**
- Modify: `src/lib/suiteOrder.ts`, `src/lib/suiteOrder.test.ts` (add `orderFromFile`)
- Modify: `src/components/RelinkDialog.tsx:24-33` (`cases` prop type)
- Modify: `src/screens/ManageCases/index.tsx`
- Create: `src/screens/ManageCases/actions.test.tsx`

**Interfaces:**
- Consumes: `open` from `@tauri-apps/plugin-dialog` (as `src/screens/ImportFile.tsx:444` uses it), `commands.parseImportFile(path) => Result<ImportResult_Serialize, string>` with `cases[].update_id: number | null` and `cases[].tester_order?: number | null`, `unwrapStr` from `src/lib/ipc`, `RelinkDialog` (`org`, `project`, `fromPbi`, `cases`, `onClose`, `onMoved`), `IconImport`, `IconMoveToPbi` from `actionIcons`.
- Produces: `orderFromFile(current: SuiteCase[], fileCases: Array<{ update_id: number | null; tester_order?: number | null }>): { order: SuiteCase[]; matched: number }`.

- [ ] **Step 1: Write the failing helper test**

Append to `src/lib/suiteOrder.test.ts` (add `orderFromFile` to the import):

```ts
describe("orderFromFile", () => {
  const suite = [
    { id: 201, title: "a" },
    { id: 202, title: "b" },
    { id: 203, title: "c" },
    { id: 204, title: "d" },
  ];
  test("sorts the suite's cases by the file's tester_order and appends the rest", () => {
    const { order, matched } = orderFromFile(suite, [
      { update_id: 203, tester_order: 1 },
      { update_id: 201, tester_order: 3 },
      { update_id: 202, tester_order: 2 },
      { update_id: 999, tester_order: 0 }, // not in the suite
      { update_id: null, tester_order: 4 }, // never uploaded
      { update_id: 204, tester_order: null }, // in the suite, no order
    ]);
    expect(order.map((c) => c.id)).toEqual([203, 202, 201, 204]);
    expect(matched).toBe(3);
  });
  test("ties keep file position; a file with no orders matches nothing", () => {
    const tie = orderFromFile(suite, [
      { update_id: 202, tester_order: 1 },
      { update_id: 201, tester_order: 1 },
    ]);
    expect(tie.order.map((c) => c.id)).toEqual([202, 201, 203, 204]);
    expect(orderFromFile(suite, [{ update_id: 201 }]).matched).toBe(0);
  });
});
```

- [ ] **Step 2: Run to verify failure**

`npx vitest run src/lib/suiteOrder.test.ts`
Expected: FAIL, `orderFromFile` is not exported.

- [ ] **Step 3: Write the helper**

Append to `src/lib/suiteOrder.ts`:

```ts
/** The order a draft file asks for. Cases the file names by id AND gives
 * a `tester_order` come first, sorted by it (ties keep the file's own
 * order); every other case in the suite follows in the order it had.
 * `matched` is how many suite cases the file placed, so the screen can
 * say when a file had nothing to say about this suite. */
export function orderFromFile(
  current: SuiteCase[],
  fileCases: Array<{ update_id: number | null; tester_order?: number | null }>,
): { order: SuiteCase[]; matched: number } {
  const byId = new Map(current.map((c) => [c.id, c]));
  const placed = fileCases
    .map((f, i) => ({ f, i }))
    .filter(({ f }) => f.update_id != null && f.tester_order != null && byId.has(f.update_id))
    .sort((a, b) => a.f.tester_order! - b.f.tester_order! || a.i - b.i)
    .map(({ f }) => byId.get(f.update_id!)!);
  const seen = new Set(placed.map((c) => c.id));
  const first = placed.filter((c, i) => placed.findIndex((p) => p.id === c.id) === i);
  const rest = current.filter((c) => !seen.has(c.id));
  return { order: [...first, ...rest], matched: first.length };
}
```

Run `npx vitest run src/lib/suiteOrder.test.ts`. Expected: pass.

- [ ] **Step 4: Widen RelinkDialog's `cases` prop**

In `src/components/RelinkDialog.tsx`: remove `TestCaseFull` from the bindings import and change the prop to:

```ts
  /** The cases to move: id and title are all the confirmation shows. */
  cases: Array<{ id: number; title: string }>;
```

`ExistingCases/index.tsx` passes `TestCaseFull[]`, which satisfies the new type; nothing else changes. Run `npx vitest run src/components/RelinkDialog.test.tsx` to confirm it still passes.

- [ ] **Step 5: Write the failing action tests**

Create `src/screens/ManageCases/actions.test.tsx`:

```tsx
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { clearMocks } from "@tauri-apps/api/mocks";
import { mountWithSuite, pickSuite } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

// Hoisted with the mock: the factory runs when the screen imports the
// plugin, before a plain const at this position would exist.
const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

test("Apply tester order from file re-orders the list from the file's tester_order", async () => {
  openDialog.mockResolvedValue("C:\\drafts\\auth.json");
  mountWithSuite((cmd, args) => {
    if (cmd === "parse_import_file" && (args as { path: string }).path === "C:\\drafts\\auth.json")
      return {
        cases: [
          { title: "c", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 203, tester_order: 1 },
          { title: "a", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: 201, tester_order: 2 },
        ],
        warnings: [],
      };
  });
  const l = await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => {
    const rows = within(l).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("#203"),
      expect.stringContaining("#201"),
      expect.stringContaining("#202"),
    ]);
  });
  expect(toast.info).toHaveBeenCalledWith("Placed 2 of 3 test cases from the file. Apply order to save.");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeEnabled();
});

test("a file that names none of the suite's cases changes nothing and says so", async () => {
  openDialog.mockResolvedValue("C:\\drafts\\other.json");
  mountWithSuite((cmd) => {
    if (cmd === "parse_import_file")
      return { cases: [{ title: "x", steps: [], tags: "", automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null, tester_order: 1 }], warnings: [] };
  });
  const l = await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(toast.warning).toHaveBeenCalledWith("No test case in that file is in this suite. The file needs ids from an upload."));
  expect(within(l).getAllByRole("listitem")[0]).toHaveTextContent("#201");
  expect(screen.getByRole("button", { name: "Apply order" })).toBeDisabled();
});

test("cancelling the file picker does nothing", async () => {
  openDialog.mockResolvedValue(null);
  const { calls } = mountWithSuite();
  await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "Apply tester order from file" }));
  await waitFor(() => expect(openDialog).toHaveBeenCalled());
  expect(calls.some((c) => c.cmd === "parse_import_file")).toBe(false);
});

test("Move to PBI needs a PBI suite and a selection, then hands the picked cases to the dialog", async () => {
  mountWithSuite();
  const l = await pickSuite(91);
  // A static suite has no PBI to move FROM.
  expect(screen.getByRole("button", { name: "Move to PBI" })).toBeDisabled();
  expect(screen.getByText("Move to PBI works on a PBI suite.")).toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("Test suite"), { target: { value: "93" } });
  await waitFor(() => expect(screen.getByText("PBI 42 suite", { selector: "option" })).toBeInTheDocument());
  const l2 = await screen.findByRole("list", { name: "Test cases in order" });
  // A PBI suite, nothing picked yet.
  expect(screen.getByRole("button", { name: "Move to PBI" })).toBeDisabled();
  fireEvent.click(within(l2).getByRole("checkbox", { name: "Select #202" }));
  fireEvent.click(screen.getByRole("button", { name: "Move to PBI" }));

  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Move 1 test case to another PBI")).toBeInTheDocument();
  expect(within(dialog).getByText("#42")).toBeInTheDocument();
});
```

Adjust the `#42` assertion to whatever RelinkDialog renders for `fromPbi` (its copy reads `The Tested By link moves from #42 to …` inside a span with class `id-mono`; `getByText("#42")` matches that span).

- [ ] **Step 6: Run to verify failure**

`npx vitest run src/screens/ManageCases/actions.test.tsx`
Expected: FAIL, no button named "Apply tester order from file".

- [ ] **Step 7: Add the two actions to the screen**

In `src/screens/ManageCases/index.tsx`:

Add imports:

```ts
import { open } from "@tauri-apps/plugin-dialog";
import RelinkDialog from "../../components/RelinkDialog";
import { IconConfirm, IconImport, IconMoveToPbi, IconUndo } from "../../lib/actionIcons";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { orderFromFile, sameOrder, type SuiteCase } from "../../lib/suiteOrder";
```

(replace the earlier `actionIcons`, `ipc` and `suiteOrder` import lines).

Add state and the file mutation after `apply`:

```ts
  const [relinkOpen, setRelinkOpen] = useState(false);

  /** A draft .json carries each uploaded case's id and, after the
   * optimizer's grouping pass, its tester_order. The file only proposes:
   * the list re-orders on screen and Apply order is what saves it. */
  const fromFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Test case files", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const parsed = await unwrapStr(commands.parseImportFile(path));
      return orderFromFile(order, parsed.cases);
    },
    onSuccess: (result) => {
      if (!result) return;
      if (result.matched === 0) {
        toast.warning("No test case in that file is in this suite. The file needs ids from an upload.");
        return;
      }
      setOrder(result.order);
      toast.info(`Placed ${result.matched} of ${order.length} test cases from the file. Apply order to save.`);
    },
    onError: (e) => toast.error(`Could not read the file: ${e.message ?? e}`),
  });
```

Note `orderFromFile(order, …)` uses the on-screen order as the base so unplaced cases keep whatever arrangement the user already made. `order.length` in the toast is read at success time, before `setOrder`, so it is the suite's case count.

Replace `const busy = apply.isPending;` with:

```ts
  const busy = apply.isPending || fromFile.isPending;
  const pbiId =
    picked?.suite.suite_type === "requirementTestSuite" ? (picked.suite.requirement_id ?? null) : null;
  const selectedCases = order.filter((c) => selected.has(c.id));
```

Extend the toolbar `div` (after the Reset button):

```tsx
            <Button size="sm" variant="ghost" disabled={busy || order.length === 0} onClick={() => fromFile.mutate()}>
              <IconImport aria-hidden />
              {fromFile.isPending ? "Reading file" : "Apply tester order from file"}
            </Button>
            <span className="mx-1 h-5 w-px bg-border" aria-hidden />
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || pbiId == null || selectedCases.length === 0}
              onClick={() => setRelinkOpen(true)}
            >
              <IconMoveToPbi aria-hidden />
              Move to PBI
            </Button>
            {pbiId == null && (
              <span className="text-xs text-faint">Move to PBI works on a PBI suite.</span>
            )}
```

After the `CaseOrderList` block, inside the fragment, add:

```tsx
          {relinkOpen && pbiId != null && (
            <RelinkDialog
              org={org}
              project={project}
              fromPbi={pbiId}
              cases={selectedCases}
              onClose={() => setRelinkOpen(false)}
              onMoved={() => {
                // The moved cases have left this suite: read it again.
                setSelected(new Set());
                qc.invalidateQueries({ queryKey: ["suite-cases", org, project, planId, suiteId] });
              }}
            />
          )}
```

- [ ] **Step 8: Run the tests, then the gates**

`npx vitest run src/screens/ManageCases src/lib/suiteOrder.test.ts src/components/RelinkDialog.test.tsx src/ui-consistency.test.ts`
Expected: all pass. Then `npx tsc --noEmit` (clean), then `npx vitest run` (all pass).

- [ ] **Step 9: Commit**

```bash
git add src/lib/suiteOrder.ts src/lib/suiteOrder.test.ts src/components/RelinkDialog.tsx src/screens/ManageCases
git commit -q -F - <<'EOF'
feat(v2): Manage Test Cases applies a file's tester order and moves cases to a PBI

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 6: Folders: create a static child suite and copy selected cases into one

**Files:**
- Modify: `src/lib/actionIcons.ts` (add `IconNewFolder`, `IconAddToFolder`)
- Create: `src/screens/ManageCases/NewFolderDialog.tsx`
- Modify: `src/screens/ManageCases/index.tsx`
- Create: `src/screens/ManageCases/folders.test.tsx`

**Interfaces:**
- Consumes: `commands.createStaticSuite`, `commands.addCasesToSuite` (Task 2), `PickedSuite.siblings` and `.rootSuiteId` (Task 3), `Modal` (`onClose`, `className`), `Input`, `Button`, `flattenTree` + `buildTree` from `lib/suiteTree`.
- Produces: `NewFolderDialog` props `{ org: string; project: string; planId: number; parents: Array<{ id: number; label: string }>; defaultParentId: number; caseIds: number[]; onClose: () => void; onCreated: () => void }`.

Folder rules, from Azure DevOps: a static suite can be created under a static suite or the plan root, never under a requirement or query suite. So the parent choices are the plan root plus every static suite of the plan, indented as the tree; the default is the plan root. Adding cases is a copy: the cases stay in the suite they came from. Targets for "Add to folder" are the same static suites (root excluded).

- [ ] **Step 1: Add the icons**

In `src/lib/actionIcons.ts`, "Making and changing things" group, after `IconMoveDown`:

```ts
  FolderPlus as IconNewFolder,
  FolderInput as IconAddToFolder,
```

- [ ] **Step 2: Write the failing tests**

Create `src/screens/ManageCases/folders.test.tsx`:

```tsx
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "sonner";
import { clearMocks } from "@tauri-apps/api/mocks";
import { mountWithSuite, pickSuite } from "./testSupport";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

test("New folder offers the plan root and static suites as parents, creates, and refreshes the tree", async () => {
  const { calls } = mountWithSuite((cmd, args) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: (args as { name: string }).name, suite_type: "staticTestSuite", requirement_id: null, parent_id: 90 };
  });
  await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");

  const parent = within(dialog).getByLabelText("Create inside");
  const labels = Array.from(parent.querySelectorAll("option")).map((o) => o.textContent);
  // The root first, then static suites only: the PBI suite (93) is not offered.
  expect(labels).toEqual(["Plan root (Auth - Test Plan)", "Regression"]);
  expect(parent).toHaveValue("90");

  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "  Smoke  " } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create folder" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "create_static_suite");
    expect(call?.args).toEqual({ organization: "acme", project: "Web", planId: 9, parentSuiteId: 90, name: "Smoke" });
  });
  expect(toast.success).toHaveBeenCalledWith('Created folder "Smoke".');
  // The plan tree is re-read so the picker and the folder list show it.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "list_plans_with_suites").length).toBeGreaterThan(1));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("with cases selected, New folder creates and then copies them in", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "create_static_suite")
      return { id: 94, name: "Smoke", suite_type: "staticTestSuite", requirement_id: null, parent_id: 91 };
    if (cmd === "add_cases_to_suite") return [201, 203];
  });
  const l = await pickSuite(91);
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #201" }));
  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #203" }));
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Create inside"), { target: { value: "91" } });
  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "Smoke" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create folder and add 2 test cases" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 94, caseIds: [201, 203] });
  });
  expect(toast.success).toHaveBeenCalledWith('Created folder "Smoke" and added 2 test cases. They stay in Regression too.');
});

test("an empty name is refused before anything is sent", async () => {
  const { calls } = mountWithSuite();
  await pickSuite(91);
  fireEvent.click(screen.getByRole("button", { name: "New folder" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByRole("button", { name: "Create folder" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Folder name"), { target: { value: "   " } });
  expect(within(dialog).getByRole("button", { name: "Create folder" })).toBeDisabled();
  expect(calls.some((c) => c.cmd === "create_static_suite")).toBe(false);
});

test("Add to folder copies the selection into the chosen static suite", async () => {
  const { calls } = mountWithSuite((cmd) => {
    if (cmd === "add_cases_to_suite") return [202];
  });
  const l = await pickSuite(93);
  const target = screen.getByLabelText("Folder");
  // Static suites of the plan, not the one on screen, not PBI suites.
  expect(Array.from(target.querySelectorAll("option")).map((o) => o.textContent)).toEqual([
    "Pick a folder",
    "Regression",
  ]);
  expect(screen.getByRole("button", { name: "Add to folder" })).toBeDisabled();

  fireEvent.click(within(l).getByRole("checkbox", { name: "Select #202" }));
  expect(screen.getByRole("button", { name: "Add to folder" })).toBeDisabled();
  fireEvent.change(target, { target: { value: "91" } });
  fireEvent.click(screen.getByRole("button", { name: "Add to folder" }));

  await waitFor(() => {
    const add = calls.find((c) => c.cmd === "add_cases_to_suite");
    expect(add?.args).toEqual({ organization: "acme", project: "Web", planId: 9, suiteId: 91, caseIds: [202] });
  });
  expect(toast.success).toHaveBeenCalledWith("Added 1 test case to Regression. It stays in PBI 42 suite too.");
});
```

- [ ] **Step 3: Run to verify failure**

`npx vitest run src/screens/ManageCases/folders.test.tsx`
Expected: FAIL, no button named "New folder".

- [ ] **Step 4: Write the dialog**

Create `src/screens/ManageCases/NewFolderDialog.tsx`:

```tsx
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconNewFolder } from "../../lib/actionIcons";
import { unwrap } from "../../lib/ipc";

/** A folder is a static test suite. Azure DevOps lets one be created
 * under a static suite or the plan root only, so `parents` is that list.
 * With cases selected the same click copies them in afterwards: they
 * stay where they were, a case can live in many suites. */
export default function NewFolderDialog({
  org,
  project,
  planId,
  parents,
  defaultParentId,
  caseIds,
  sourceName,
  onClose,
  onCreated,
}: {
  org: string;
  project: string;
  planId: number;
  parents: Array<{ id: number; label: string }>;
  defaultParentId: number;
  caseIds: number[];
  /** The suite the selected cases are in, for the "they stay in X" line. */
  sourceName: string;
  onClose: () => void;
  onCreated: () => void;
}) {
  const [name, setName] = useState("");
  const [parentId, setParentId] = useState(String(defaultParentId));
  const trimmed = name.trim();
  const n = caseIds.length;

  const create = useMutation({
    mutationFn: async () => {
      const suite = await unwrap(commands.createStaticSuite(org, project, planId, Number(parentId), trimmed));
      const added = n > 0 ? await unwrap(commands.addCasesToSuite(org, project, planId, suite.id, caseIds)) : [];
      return { suite, added };
    },
    onSuccess: ({ suite, added }) => {
      if (n === 0) toast.success(`Created folder "${suite.name}".`);
      else
        toast.success(
          `Created folder "${suite.name}" and added ${added.length} test case${added.length === 1 ? "" : "s"}. They stay in ${sourceName} too.`,
        );
      onCreated();
      onClose();
    },
    onError: (e) => toast.error(`Could not create the folder: ${e.message}`),
  });

  const selectClass =
    "mt-1 w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text focus:border-accent focus:outline-none";

  return (
    <Modal onClose={onClose} className="w-full max-w-md space-y-3 p-5">
      <h2 className="text-sm font-semibold text-text">New folder</h2>
      <p className="text-xs text-muted">
        A folder is a static test suite. It can sit under the plan root or under another folder.
      </p>
      <label className="block text-xs text-muted">
        Folder name
        <Input
          aria-label="Folder name"
          autoFocus
          className="mt-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Smoke"
        />
      </label>
      <label className="block text-xs text-muted">
        Create inside
        <select
          aria-label="Create inside"
          className={selectClass}
          value={parentId}
          onChange={(e) => setParentId(e.target.value)}
        >
          {parents.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
      </label>
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={create.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={!trimmed || create.isPending} onClick={() => create.mutate()}>
          <IconNewFolder aria-hidden />
          {create.isPending
            ? "Creating"
            : n > 0
              ? `Create folder and add ${n} test case${n === 1 ? "" : "s"}`
              : "Create folder"}
        </Button>
      </div>
    </Modal>
  );
}
```

- [ ] **Step 5: Add the folder controls to the screen**

In `src/screens/ManageCases/index.tsx`:

Imports to add:

```ts
import { useMemo } from "react"; // merge into the existing react import
import { IconAddToFolder, IconNewFolder } from "../../lib/actionIcons"; // merge into the existing actionIcons import
import { buildTree, flattenTree } from "../../lib/suiteTree";
import NewFolderDialog from "./NewFolderDialog";
```

State and derived lists, after `relinkOpen`:

```ts
  const [folderOpen, setFolderOpen] = useState(false);
  const [targetFolder, setTargetFolder] = useState("");
  useEffect(() => setTargetFolder(""), [suiteId]);

  const INDENT = "\u00a0\u00a0\u00a0\u00a0";
  /** Static suites of the plan, tree order, indented: the only places a
   * folder can be created in or cases copied to. */
  const staticSuites = useMemo(
    () =>
      picked
        ? flattenTree(buildTree(picked.siblings))
            .filter(({ suite }) => suite.suite_type === "staticTestSuite")
            .map(({ suite, depth }) => ({ id: suite.id, label: `${INDENT.repeat(depth)}${suite.name}`, name: suite.name }))
        : [],
    [picked],
  );
  const parents = useMemo(
    () =>
      picked
        ? [
            ...(picked.rootSuiteId != null ? [{ id: picked.rootSuiteId, label: `Plan root (${picked.planName})` }] : []),
            ...staticSuites.map(({ id, label }) => ({ id, label })),
          ]
        : [],
    [picked, staticSuites],
  );
  const folderTargets = staticSuites.filter((s) => s.id !== suiteId);

  const addToFolder = useMutation({
    mutationFn: () => unwrap(commands.addCasesToSuite(org, project, planId, Number(targetFolder), selectedCases.map((c) => c.id))),
    onSuccess: (added) => {
      const folder = folderTargets.find((f) => String(f.id) === targetFolder)?.name ?? "the folder";
      toast.success(
        `Added ${added.length} test case${added.length === 1 ? "" : "s"} to ${folder}. ${added.length === 1 ? "It stays" : "They stay"} in ${picked!.suite.name} too.`,
      );
      setSelected(new Set());
    },
    onError: (e) => toast.error(`Could not add to the folder: ${e.message}`),
  });
```

`busy` becomes `apply.isPending || fromFile.isPending || addToFolder.isPending`. Move the `selectedCases`/`pbiId` block above these so `addToFolder` can read `selectedCases`.

Add a second toolbar row after the first toolbar `div`:

```tsx
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="ghost" disabled={busy || parents.length === 0} onClick={() => setFolderOpen(true)}>
              <IconNewFolder aria-hidden />
              New folder
            </Button>
            <label className="flex items-center gap-2 text-xs text-muted">
              Folder
              <select
                aria-label="Folder"
                className="rounded-md border border-border bg-surface px-2 py-1.5 text-sm text-text focus:border-accent focus:outline-none disabled:opacity-50"
                value={targetFolder}
                disabled={busy || folderTargets.length === 0}
                onChange={(e) => setTargetFolder(e.target.value)}
              >
                <option value="">Pick a folder</option>
                {folderTargets.map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.label}
                  </option>
                ))}
              </select>
            </label>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || !targetFolder || selectedCases.length === 0}
              onClick={() => addToFolder.mutate()}
            >
              <IconAddToFolder aria-hidden />
              {addToFolder.isPending ? "Adding" : "Add to folder"}
            </Button>
            <span className="text-xs text-faint">Adding copies the selected cases; they stay in this suite.</span>
          </div>
```

And the dialog, next to `RelinkDialog`:

```tsx
          {folderOpen && picked && parents.length > 0 && (
            <NewFolderDialog
              org={org}
              project={project}
              planId={planId}
              parents={parents}
              defaultParentId={parents[0].id}
              caseIds={selectedCases.map((c) => c.id)}
              sourceName={picked.suite.name}
              onClose={() => setFolderOpen(false)}
              onCreated={() => {
                // The tree has a new suite: the picker and the folder
                // list both read from this query.
                qc.invalidateQueries({ queryKey: ["plans-suites", org, project] });
                setSelected(new Set());
              }}
            />
          )}
```

`picked.siblings` is a snapshot taken at pick time, so a folder created just now only appears in `folderTargets` once the user re-picks the suite after the tree refreshes. Make that automatic: in `SuitePicker`, add a `useEffect` that, when `plans.data` changes and `picked` is set, calls `onPick` again with the fresh `siblings` for the same suite id (and `onPick(null)` if the suite is gone):

```ts
  useEffect(() => {
    if (!picked || !plans.data) return;
    const p = plans.data.find((x) => x.plan.id === picked.planId);
    const suite = p?.suites.find((s) => s.id === picked.suite.id);
    if (!p || !suite) {
      onPick(null);
      return;
    }
    if (suite !== picked.suite || p.suites !== picked.siblings) {
      onPick({ planId: p.plan.id, planName: p.plan.name, rootSuiteId: p.plan.root_suite_id, suite, siblings: p.suites });
    }
    // onPick is a state setter in the one caller; re-running on its identity would loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plans.data, picked?.planId, picked?.suite.id]);
```

The reference-equality checks keep it from re-picking on every render: once `onPick` has stored the objects from the current `plans.data`, they are the same objects and the effect does nothing. The `["suite-cases", …]` key contains the suite id and does not change, so the list is not re-read by this.

- [ ] **Step 6: Run the tests, then the gates**

`npx vitest run src/screens/ManageCases src/ui-consistency.test.ts`
Expected: all pass. Then `npx tsc --noEmit` (clean), then `npx vitest run` (all pass).

- [ ] **Step 7: Commit**

```bash
git add src/lib/actionIcons.ts src/screens/ManageCases
git commit -q -F - <<'EOF'
feat(v2): Manage Test Cases creates folders and copies selected cases into them

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 7: Full gates and a manual walk-through in the dev app

**Files:** none new.

- [ ] **Step 1: Rust gate**

From `src-tauri/`: `$env:CARGO_TARGET_DIR="target/gate"; cargo test --tests`
Expected: all pass. Then `git status`: if `src/bindings.ts` shows as modified with a whitespace-only diff, `git checkout -- src/bindings.ts`.

- [ ] **Step 2: Frontend gates, one at a time**

`npx tsc --noEmit`, then `npx vitest run`.
Expected: clean and all pass.

- [ ] **Step 3: Manual walk-through (dev app only, the user's project)**

jsdom cannot see overlays or real drag, so before the user tries this on a real plan, run `npm run tauri dev` and check on a throwaway static suite:

1. Manage Test Cases appears in the sidebar with the "In Dev" pill and "In Development" beside the heading; Ctrl+8 reaches it.
2. Pick a plan and a suite: the list matches Azure DevOps' order.
3. Drag a row, Apply order, refresh the suite in the browser: Azure DevOps shows the new order.
4. Apply tester order from a draft file whose cases were uploaded to that suite.
5. New folder under the plan root, then Add to folder with two cases selected: both suites list them.
6. On a PBI suite, Move to PBI with one case selected: the dialog opens with the right count.

Report what worked and what did not in the task report; anything that does not match goes back through the fix loop before the branch is offered for merge.
