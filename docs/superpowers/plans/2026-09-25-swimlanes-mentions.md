# Work Manager Swimlanes and a Mentions Inbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Work Manager board can group its cards into swimlanes by direct parent, and the notification bell reports @mentions of you in work-item discussions and in the threads of PRs you are on.

**Architecture:** Part 1: `fetch_board` asks for `System.Parent`, reads each distinct parent's title and type once (batches of 200, `errorPolicy=omit`, never failing the board) and returns it on `BoardItem.parent`; a pure module `src/lib/boardLanes.ts` groups filtered cards into lanes and owns the two localStorage preferences; `WorkBoard.tsx` renders one column grid per lane behind a Swimlanes switch. Part 2: a new Rust module `work_board/mentions.rs` runs the `@RecentMentions` WIQL, scans each item's newest comments for your mention anchor and returns `Mention`s through a `recent_mentions` command; the webview turns those, and `@<id>` tokens found in the PR threads `usePrAttention` already reads, into a new `mention` notification kind, with a per-organisation first-run baseline and one toast or OS notification per check.

**Tech Stack:** Rust (Tauri 2, tauri-specta, serde_json, wiremock for tests), React 19 + TypeScript, TanStack Query, Tailwind 4 theme tokens, vitest + Testing Library + axe-core.

**Spec:** `docs/superpowers/specs/2026-09-25-swimlanes-mentions-design.md` (binding; read it before any task).

**Decisions this plan makes that the spec leaves open (a reviewer should check them):**
- **The mentions WIQL is scoped to the project.** The spec's query has no project clause, so it would return items from every project in the organisation, while the comments URL, the drawer and the notification's `target.project` are all per project. The query sent is `SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = @project AND [System.Id] IN (@RecentMentions) ORDER BY [System.ChangedDate] DESC`, which is the spec's text with the project clause added.
- **`PrComment` gains `author_id`.** Today a PR comment carries only the author's display name, so "skipping comments you wrote" by identity id is impossible without it. Task 4 adds it (from `author.id`) and regenerates the bindings.
- **"Once per organisation per session, in memory only"** is the Rust session cache (`cache::session_fresh/session_put`, key `keys::connected_user`). Both `connected_user` and `recent_mentions` read through it. In the webview, the PR scan shares the comments panel's existing `["connected-user", org]` query (`staleTime: Infinity`, memory only).
- **Parents that are themselves cards on the board are not read again.** Their title and type come from the card read. Only the other distinct parent ids are batch-read.
- **The baseline is a timestamp.** `tcm-v2-mentions-baseline:<org>` holds the time (ms) of the first check ever for that organisation. On every check, from either source, a mention created more than 24 hours before that time is recorded as seen and not shown. A mention whose date cannot be read counts as old. With this rule, the work-item check and the PR check cannot race over which one counts as "first".
- **One toast per check.** When several mentions are new at once, there is one toast (or OS notification) titled `N new mentions` listing up to three, like the assigned summary. Each one is still its own bell entry.
- **The Mention colour is `bg-danger/15 text-danger`.** It is the one token no other kind uses.
- **Excerpts end with `…` when cut:** 139 characters plus the ellipsis, 140 in all. In a PR excerpt, `@<id>` tokens read `@you` for your id and `@someone` for any other id, because PR comments carry ids, not names.
- **A mentioned item whose title cannot be read** keeps the mention, with type `Work item` and an empty title.
- **An empty identity id matches nothing.** Every mention anchor starts `data-vss-mention="version:2.0,`, so an empty id would otherwise match every mention of anyone.
- **The mentions query polls in the background** (`refetchIntervalInBackground: true`). Otherwise a minimised app would stop checking, and the OS notification the spec asks for could never fire.
- **Collapse all** collapses the lanes on screen. **Expand all** clears every remembered collapsed lane for the organisation and project.

## Global Constraints

- **Branch:** `feat/swimlanes-mentions`. Stay on it. No release, no version bump, no `changelog.ts` entry, no README change in this plan.
- **Rust tests live only in `src-tauri/tests/`**, never a `#[cfg(test)]` module inside `src/`. Reuse the wiremock pattern already in `tests/work_board.rs` (`MockServer`, `AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())`).
- **One build or test command at a time** (shared machine). Before ANY cargo command, run the dev-app check with the PowerShell tool: `Get-Process v2, cargo -ErrorAction SilentlyContinue | Select-Object Name, Path`. If a `cargo` is listed, wait for it. Never kill a `v2` process. If one's `Path` is under `target\gate`, ask the controller.
- **Rust commands** run from `src-tauri/` as `CARGO_TARGET_DIR=target/gate cargo test --test <name>` (Bash tool). **Frontend:** `npx vitest run --exclude "**/.claude/**" <files>` and `npx tsc --noEmit` from the repo root. Use the Grep and Read tools for searching (bash `grep` hangs here).
- **`src/bindings.ts` is generated** by `CARGO_TARGET_DIR=target/gate cargo test --test bindings`. Never hand-edit it. If it shows as modified but `git diff --ignore-all-space --ignore-cr-at-eol -- src/bindings.ts` is empty, run `git checkout -- src/bindings.ts` and do not commit it.
- **No HTTP DELETE to Azure DevOps.** Everything added here is a GET or the existing WIQL POST.
- **User-facing errors name no URL.** Transport failures keep going through `network_error` in `ado/transport.rs`. New log lines use `AdoError`'s `Display`, which carries no URL.
- **One cache implementation per side.** In Rust, only `crate::cache` (key and TTL in `cache/keys.rs`). In the webview, nothing new is cached. The new localStorage keys are preferences and markers, not cache entries. `src/lib/cache.test.ts` and `tests/cache.rs` must stay green.
- **Theme tokens only.** The lane type badge reuses `WorkBoard`'s existing `typeColor` map (that file is on the hex allowlist). `src/ui-consistency.test.ts` and `src/a11y.test.tsx` are never weakened; this plan adds an a11y case.
- **Icons** come from `src/lib/actionIcons.ts` as `<IconX aria-hidden />` inside buttons. The lane chevrons follow the existing ViewCases group toggle (lucide `ChevronDown`/`ChevronRight` with `aria-hidden`, in a button that has an `aria-label`).
- **No new crates, no new npm packages.**
- **No em dashes** in any text a user reads.
- **Exact values from the spec, verbatim:**
  - `BOARD_FIELDS` gains `System.Parent`. `BoardParent { id: i32, title: String, work_item_type: String }`. `BoardItem.parent: Option<BoardParent>`. The parent read uses batches of 200 with `errorPolicy=omit`. An unreadable parent gets title `""` and its lane reads `#1234`.
  - localStorage: `tcm-v2-board-swimlanes` (starts off), `tcm-v2-board-lanes-collapsed:<org>/<project>` (a list of parent ids, `0` for No parent), `tcm-v2-hidden-cols` (shared by every lane), `tcm-v2-mentions-baseline:<org>`.
  - Lane toggle accessible name: `Leave requests, 4 cards, collapse` / `... expand`. The lane with no parent is `No parent` and always comes last. Buttons: **Swimlanes** (switch), **Collapse all**, **Expand all**.
  - `recent_mentions(organization, project) -> Result<Vec<Mention>, AdoError>`. The 20 most recently changed items. Comments come from `/_apis/wit/workItems/{id}/comments?order=desc&$top=50`. The anchor is `data-vss-mention="version:2.0,{your id}"`, compared case-insensitively. Your own comments are skipped. `Mention { source: "work-item", item_id, item_type, item_title, comment_id, author, excerpt, created_date }`. The excerpt is at most 140 characters.
  - PR scan: `@<{your id}>`, case-insensitive, skipping your own comments. No new request.
  - Notification kind `"mention"`, label **Mention**. Ids `mention:wi:<item>:<comment>` and `mention:pr:<repo>:<pr>:<thread>:<comment>`. Titles `<author> mentioned you on <Type> #<id>` and `<author> mentioned you on PR #<id>`. The body is the excerpt. The target is the work item or the PR. `href` is Azure DevOps' own page.
  - Work-item mentions are checked every 5 minutes and at start. The first-run window is 24 hours. A failure is logged and retried at the next check, with no toast.
- **Commits:** Bash heredoc, `git commit -q -F - <<'EOF' ... EOF`, confirm with `git log -1`. The trailer names the model that writes the commit (the plan shows `Claude Opus 5.5`; write your own model's name if it differs). Keep each edited file's existing line endings (they are CRLF).

## Review Focus

1. **Board data without a `parent` field at all** (the tour's board, or any data shaped before this change). Expected: those cards land in the No parent lane and nothing throws. Test: Task 2, `a card with no parent field at all goes to No parent`.
2. **More than 200 distinct parents, and many cards sharing one parent.** Expected: parents are read in batches of 200, and each id is read once. Tests: Task 1, `more_than_two_hundred_parents_are_read_in_batches_of_two_hundred` and `board_cards_carry_their_direct_parent_read_once_per_id`.
3. **The identity lookup answers with an empty id.** Expected: no mentions at all, rather than every mention of anyone. Tests: Task 4, `an_identity_without_an_id_matches_nothing`, and Task 5, `an empty identity id finds nothing`.
4. **Several mentions arrive in one check.** Expected: one toast or OS notification that summarises them, not one per mention. Test: Task 5, `several new mentions make one toast, not one each`.
5. **On the first run, a mention whose date cannot be read.** Expected: it is recorded as seen and not shown, so a malformed date cannot defeat the flood guard. Test: Task 5, `a mention with no readable date counts as old`.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/src/work_board/mod.rs` (modify) | `System.Parent` in `BOARD_FIELDS`; `TITLE_FIELDS`; `BoardParent`; `BoardItem.parent`; `pub mod mentions;` |
| `src-tauri/src/work_board/board.rs` (modify) | `read_titles` (batches of 200, `errorPolicy=omit`, never fails); `fetch_board` fills `parent` |
| `src-tauri/src/work_board/mentions.rs` (create) | `Mention`, the WIQL, `mentions_me`, `excerpt`, `mentions_in`, `connected_user_cached`, `recent_mentions` |
| `src-tauri/src/cache/keys.rs` (modify) | `CONNECTED_USER_TTL`, `connected_user(base_url, org)` |
| `src-tauri/src/commands/board.rs` (modify) | `connected_user` reads through the cache; new `recent_mentions` command |
| `src-tauri/src/lib.rs` (modify) | Register `board::recent_mentions` |
| `src-tauri/src/ado_git.rs` (modify) | `PrComment.author_id` |
| `src-tauri/tests/work_board.rs`, `tests/ado_git.rs` (modify), `tests/mentions.rs` (create) | Rust tests |
| `src/bindings.ts` (generated) | Regenerated in Tasks 1 and 4 |
| `src/lib/boardLanes.ts` (create) + test | Lane grouping, names, the swimlanes and collapsed-lane preferences |
| `src/screens/WorkBoard.tsx` (modify) + test | Switch, Collapse all / Expand all, lane rows, per-lane grids, lane-bound drops |
| `src/lib/actionIcons.ts` (modify) | `IconExpandAll` |
| `src/a11y.test.tsx` (modify) | The board with swimlanes is audited |
| `src/lib/notifications.ts` (modify) + test | `"mention"` kind, `markSeen` |
| `src/lib/mentions.ts` (create) + test | Ids, text, the PR scan, the baseline, the toast |
| `src/components/NotificationBell.tsx` (modify) + test | Mention label and colour |
| `src/hooks/useMentions.ts` (create) + test | The 5-minute work-item check |
| `src/hooks/usePrAttention.ts` (modify) + test | The PR-thread scan |
| `src/components/ContextBar.tsx` (modify), `src/components/ContextBar.test.tsx` (create) | Mount `useMentions` beside `usePrAttention` |
| `src/dev/demo.ts`, `src/tour/tourData.ts` (modify) | `parent` on typed board items; a demo mention; `author_id` on demo PR comments |

## Tasks

1. Board cards carry their direct parent (Rust)
2. Lane grouping and its preferences (pure frontend module)
3. The swimlane view on the board
4. Work-item mentions and PR comment authors (Rust)
5. Mentions become bell notifications (pure frontend and the bell)
6. Checking for mentions (hooks and mounting)

---

### Task 1: Board cards carry their direct parent (Rust)

**Files:**
- Modify: `src-tauri/src/work_board/mod.rs:33-57`
- Modify: `src-tauri/src/work_board/board.rs:6-9` (imports), add `read_titles` before `fetch_board` (~l.272), and change `fetch_board` (~l.379-410)
- Modify: `src/dev/demo.ts:169-172`, `:367`, `:768`; `src/tour/tourData.ts:218-222`
- Test: `src-tauri/tests/work_board.rs` (append)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `v2_lib::work_board::BoardParent { pub id: i32, pub title: String, pub work_item_type: String }` (`Debug, Clone, PartialEq, Serialize, specta::Type`)
  - `BoardItem.parent: Option<BoardParent>`. In TS this is `parent: BoardParent | null`.
  - `pub(crate) const TITLE_FIELDS: &str = "System.Title,System.WorkItemType"` in `work_board`
  - `impl AdoClient { pub(crate) async fn read_titles(&self, org: &str, project: &str, ids: &[i32]) -> HashMap<i32, (String, String)> }`, where the tuple is `(title, work_item_type)`. It never fails; Task 4 reuses it.

- [ ] **Step 1: Write the failing tests.** In `src-tauri/tests/work_board.rs`, change the `use v2_lib::work_board::{...}` block to:

```rust
use v2_lib::work_board::{
    column_for_state, state_for_column, team_area_clause, wiql_str, BoardParent, StateInfo,
};
```

Then append:

```rust
// ---- swimlanes: each card's direct parent --------------------------------

/// A card as the batch read returns it, optionally with a parent.
fn card(id: i32, title: &str, parent: Option<i32>) -> serde_json::Value {
    let mut fields = serde_json::json!({
        "System.Title": title,
        "System.WorkItemType": "Task",
        "System.State": "To Do",
        "System.ChangedDate": "2026-09-25T00:00:00Z"
    });
    if let Some(p) = parent {
        fields["System.Parent"] = serde_json::json!(p);
    }
    serde_json::json!({ "id": id, "fields": fields })
}

/// The parent read is the batch GET that carries errorPolicy=omit; the
/// card read never does.
fn is_parent_read(r: &wiremock::Request) -> bool {
    r.url.query().unwrap_or("").contains("errorPolicy=omit")
}

/// WIQL answering with the cards' ids, the card read answering with the
/// cards, and the Task states.
async fn mount_board(server: &MockServer, cards: Vec<serde_json::Value>) {
    let ids: Vec<serde_json::Value> =
        cards.iter().map(|c| serde_json::json!({ "id": c["id"] })).collect();
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "workItems": ids })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems"))
        .and(|r: &wiremock::Request| !is_parent_read(r))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": cards })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Task/states"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "name": "To Do", "color": "b2b2b2", "category": "Proposed" }]
        })))
        .mount(server)
        .await;
}

async fn mount_parents(server: &MockServer, answer: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems"))
        .and(is_parent_read)
        .respond_with(answer)
        .mount(server)
        .await;
}

/// The `ids` of every parent read, in the order they were sent.
async fn parent_reads(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| is_parent_read(r))
        .map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "ids")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default()
        })
        .collect()
}

fn board_client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
}

/// Swimlanes group by the DIRECT parent. The card read asks for
/// System.Parent, and the parents come from one batch read of the distinct
/// ids: two cards under #500 cost one id, not two.
#[tokio::test]
async fn board_cards_carry_their_direct_parent_read_once_per_id() {
    let server = MockServer::start().await;
    mount_board(
        &server,
        vec![
            card(11, "Draft the form", Some(500)),
            card(12, "Review the form", Some(500)),
            card(13, "Loose task", None),
        ],
    )
    .await;
    mount_parents(
        &server,
        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "count": 1, "value": [
            { "id": 500, "fields": {
                "System.Title": "Leave requests",
                "System.WorkItemType": "Product Backlog Item"
            } }
        ] })),
    )
    .await;

    let board = board_client(&server).fetch_board("org", "proj", None, None, false).await.unwrap();

    let leave = BoardParent {
        id: 500,
        title: "Leave requests".into(),
        work_item_type: "Product Backlog Item".into(),
    };
    assert_eq!(board.items[0].parent, Some(leave.clone()));
    assert_eq!(board.items[1].parent, Some(leave));
    assert_eq!(board.items[2].parent, None, "no parent, no lane header to fill");
    assert_eq!(parent_reads(&server).await, vec!["500".to_string()], "one read, each id once");

    let card_read = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method.as_str() == "GET" && !is_parent_read(r) && r.url.path() == "/org/proj/_apis/wit/workitems")
        .expect("the card read");
    assert!(card_read.url.query().unwrap_or("").contains("System.Parent"), "the card read asks for the parent");
}

/// Deleted, in another project, or not permitted: errorPolicy=omit sends a
/// null in its place, and the lane falls back to `#id`.
#[tokio::test]
async fn an_unreadable_parent_keeps_its_id_with_an_empty_title() {
    let server = MockServer::start().await;
    mount_board(&server, vec![card(11, "Orphan", Some(900))]).await;
    mount_parents(
        &server,
        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "count": 1, "value": [null] })),
    )
    .await;

    let board = board_client(&server).fetch_board("org", "proj", None, None, false).await.unwrap();

    assert_eq!(
        board.items[0].parent,
        Some(BoardParent { id: 900, title: String::new(), work_item_type: String::new() })
    );
}

/// The parent read is decoration: when it fails outright, the board still
/// loads and every card keeps its parent id.
#[tokio::test]
async fn a_failed_parent_read_never_fails_the_board() {
    let server = MockServer::start().await;
    mount_board(&server, vec![card(11, "Draft the form", Some(500))]).await;
    mount_parents(&server, ResponseTemplate::new(500).set_body_string("boom")).await;

    let board = board_client(&server).fetch_board("org", "proj", None, None, false).await.unwrap();

    assert_eq!(board.items.len(), 1);
    assert_eq!(
        board.items[0].parent,
        Some(BoardParent { id: 500, title: String::new(), work_item_type: String::new() })
    );
}

/// A PBI card sits in its Feature's lane while its tasks sit in the PBI's:
/// the PBI's own title comes from the card read, and only the Feature is
/// read as a parent.
#[tokio::test]
async fn a_parent_that_is_on_the_board_is_not_read_again() {
    let server = MockServer::start().await;
    mount_board(
        &server,
        vec![card(11, "Draft the form", Some(500)), card(500, "Leave requests", Some(50))],
    )
    .await;
    mount_parents(
        &server,
        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "id": 50, "fields": { "System.Title": "Absence", "System.WorkItemType": "Feature" } }
        ] })),
    )
    .await;

    let board = board_client(&server).fetch_board("org", "proj", None, None, false).await.unwrap();

    assert_eq!(parent_reads(&server).await, vec!["50".to_string()]);
    assert_eq!(
        board.items[0].parent,
        Some(BoardParent { id: 500, title: "Leave requests".into(), work_item_type: "Task".into() })
    );
    assert_eq!(
        board.items[1].parent,
        Some(BoardParent { id: 50, title: "Absence".into(), work_item_type: "Feature".into() })
    );
}

/// Review focus 2: Azure DevOps caps a batch read at 200 ids, so 201
/// distinct parents are two reads, and none of them is asked twice.
#[tokio::test]
async fn more_than_two_hundred_parents_are_read_in_batches_of_two_hundred() {
    let server = MockServer::start().await;
    let cards: Vec<serde_json::Value> =
        (1..=201).map(|i| card(i, &format!("Card {i}"), Some(1000 + i))).collect();
    mount_board(&server, cards).await;
    mount_parents(
        &server,
        ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })),
    )
    .await;

    let board = board_client(&server).fetch_board("org", "proj", None, None, false).await.unwrap();

    let reads = parent_reads(&server).await;
    let sizes: Vec<usize> = reads.iter().map(|ids| ids.split(',').count()).collect();
    assert_eq!(sizes, vec![200, 1]);
    let mut all: Vec<&str> = reads.iter().flat_map(|ids| ids.split(',')).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 201, "each parent id asked for exactly once");
    assert_eq!(board.items.len(), 201);
}
```

- [ ] **Step 2: Run to see them fail** (dev-app check first). From `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test work_board`. Expected: compile error `no BoardParent in work_board` (and `no field parent`).

- [ ] **Step 3: Implement `mod.rs`.** Replace lines 33-57 (the `BOARD_FIELDS` doc comment through the end of `BoardItem`) with:

```rust
/// Fields the board fetch asks for (rich-text fields deliberately excluded -
/// they are heavy and only the detail editor needs them). System.Parent is
/// the plain id of the card's direct parent, for the swimlanes.
const BOARD_FIELDS: &str = "System.Id,System.Title,System.WorkItemType,System.State,System.AssignedTo,System.ChangedDate,System.Tags,Microsoft.VSTS.Common.Priority,System.Parent";

/// What a lane header needs of a work item, and nothing more.
pub(crate) const TITLE_FIELDS: &str = "System.Title,System.WorkItemType";

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct StateInfo {
    pub name: String,
    pub color: String,
    pub category: String,
}

/// A card's direct parent, as its swimlane shows it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct BoardParent {
    pub id: i32,
    /// Empty when the parent could not be read (deleted, in another
    /// project, or no permission): the lane then reads `#id`.
    pub title: String,
    /// Empty whenever `title` is.
    pub work_item_type: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BoardItem {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub state_color: String,
    /// "To Do" | "In Progress" | "Done"; None = hidden (Removed).
    pub column: Option<String>,
    pub assigned_to: String,
    pub tags: String,
    pub priority: Option<i32>,
    pub changed_date: String,
    /// The direct parent (tasks and bugs under their PBI, PBIs under their
    /// Feature); None when the item has none.
    pub parent: Option<BoardParent>,
}
```

- [ ] **Step 4: Implement `board.rs`.** Replace the `use super::{...}` block (lines 6-9) with:

```rust
use super::{
    wiql_str, BoardData, BoardItem, BoardParent, Member, StateInfo, TeamRef, BOARD_FIELDS,
    EXCLUDED_TYPES, MAX_ITEMS, TITLE_FIELDS,
};
```

Directly above the `/// The board in one call, ported from v1 _fetch_work: ...` doc comment of `fetch_board`, add:

```rust
    /// Title and type of each readable id, read in batches of 200 with
    /// `errorPolicy=omit` so a deleted, moved or forbidden id comes back as
    /// a null entry instead of failing its whole batch. Never fails: a batch
    /// that errors is logged and its ids are simply absent from the answer.
    /// Read only.
    pub(crate) async fn read_titles(
        &self,
        org: &str,
        project: &str,
        ids: &[i32],
    ) -> HashMap<i32, (String, String)> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/{}/_apis/wit/workitems?ids={}&fields={}&errorPolicy=omit&api-version=7.1",
                self.base_url, org, project, ids_csv, TITLE_FIELDS
            );
            match self.get_json(url).await {
                Ok(data) => {
                    for w in data["value"].as_array().cloned().unwrap_or_default() {
                        // An omitted id is a null entry: nothing to read.
                        let Some(id) = w["id"].as_i64() else { continue };
                        let f = &w["fields"];
                        out.insert(
                            id as i32,
                            (
                                f["System.Title"].as_str().unwrap_or_default().to_string(),
                                f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                            ),
                        );
                    }
                }
                Err(e) => crate::applog::warn(format!(
                    "could not read the titles of {} work item(s); they show as ids only: {e}",
                    chunk.len()
                )),
            }
        }
        out
    }

```

In `fetch_board`, directly above `        // Preserve WIQL order (ChangedDate DESC), not batch-GET order.`, insert:

```rust
        // Each card's direct parent, for the board's swimlanes. A parent
        // that is itself on the board is already in hand; the others are
        // read once each, and one that cannot be read keeps its id with an
        // empty title. This read never fails the board.
        let mut known: HashMap<i32, (String, String)> = raw_items
            .iter()
            .filter_map(|w| {
                let f = &w["fields"];
                Some((
                    w["id"].as_i64()? as i32,
                    (
                        f["System.Title"].as_str().unwrap_or_default().to_string(),
                        f["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
                    ),
                ))
            })
            .collect();
        let mut to_read: Vec<i32> = vec![];
        let mut queued = std::collections::HashSet::new();
        for w in &raw_items {
            if let Some(p) = w["fields"]["System.Parent"].as_i64().map(|p| p as i32) {
                if !known.contains_key(&p) && queued.insert(p) {
                    to_read.push(p);
                }
            }
        }
        known.extend(self.read_titles(org, project, &to_read).await);
        let parent_of = |f: &serde_json::Value| {
            f["System.Parent"].as_i64().map(|p| {
                let id = p as i32;
                let (title, work_item_type) = known.get(&id).cloned().unwrap_or_default();
                BoardParent { id, title, work_item_type }
            })
        };

```

In the `BoardItem { ... }` literal, replace:

```rust
                    changed_date: f["System.ChangedDate"].as_str().unwrap_or_default().to_string(),
                })
```

with:

```rust
                    changed_date: f["System.ChangedDate"].as_str().unwrap_or_default().to_string(),
                    parent: parent_of(f),
                })
```

- [ ] **Step 5: Run** (one at a time, dev-app check before each): `CARGO_TARGET_DIR=target/gate cargo test --test work_board` (all pass, old and new), then `CARGO_TARGET_DIR=target/gate cargo test --test bindings`. The second regenerates `src/bindings.ts` with `BoardParent` and `BoardItem.parent: BoardParent | null`.

- [ ] **Step 6: Give the typed board literals their parent.** `npx tsc --noEmit` now fails in `src/dev/demo.ts` and `src/tour/tourData.ts`. In `src/dev/demo.ts`, replace lines 169-172 (the four `boardItems` entries) with:

```ts
  { id: 2001, title: "Demo Task - wire the login flow", work_item_type: "Task", state: "In Progress", state_color: "007acc", column: "In Progress", assigned_to: "Demo User", tags: "demo", priority: 2, changed_date: "2026-07-13T08:00:00Z", parent: { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" } },
  { id: 2002, title: "Demo Task - write test cases", work_item_type: "Task", state: "To Do", state_color: "b2b2b2", column: "To Do", assigned_to: "Demo User", tags: "", priority: 2, changed_date: "2026-07-13T07:00:00Z", parent: { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" } },
  { id: 2003, title: "Demo Bug - session timeout not enforced", work_item_type: "Bug", state: "To Do", state_color: "cc293d", column: "To Do", assigned_to: "Demo User", tags: "demo", priority: 1, changed_date: "2026-07-13T06:00:00Z", parent: null },
  { id: 2004, title: "Demo Task - done example", work_item_type: "Task", state: "Done", state_color: "339933", column: "Done", assigned_to: "Demo User", tags: "", priority: 3, changed_date: "2026-07-12T06:00:00Z", parent: { id: 1002, title: "Demo - Checkout redesign", work_item_type: "Product Backlog Item" } },
```

In the same file, in `fileBug` (~l.367) and `createWorkItem` (~l.768), change `changed_date: new Date().toISOString() });` to `changed_date: new Date().toISOString(), parent: null });` (both lines).

In `src/tour/tourData.ts`, replace lines 218-222 (the five `TOUR_BOARD.items` entries) with:

```ts
    { id: 4821, title: "Guest checkout", work_item_type: "Product Backlog Item", state: "Committed", state_color: "007acc", column: "In Progress", assigned_to: "Sam Taylor", tags: "Checkout", priority: 2, changed_date: "2026-08-28T09:15:00Z", parent: null },
    { id: 4822, title: "The basket keeps items for 30 days", work_item_type: "Product Backlog Item", state: "New", state_color: "b2b2b2", column: "To Do", assigned_to: "Sam Taylor", tags: "Basket", priority: 2, changed_date: "2026-08-27T11:02:00Z", parent: null },
    { id: 4830, title: "Write the checkout test cases", work_item_type: "Task", state: "In Progress", state_color: "007acc", column: "In Progress", assigned_to: "Sam Taylor", tags: "", priority: 1, changed_date: "2026-08-28T14:40:00Z", parent: { id: 4821, title: "Guest checkout", work_item_type: "Product Backlog Item" } },
    { id: 4831, title: "Card errors show the wrong message", work_item_type: "Bug", state: "New", state_color: "cc293d", column: "To Do", assigned_to: "Sam Taylor", tags: "Payments", priority: 1, changed_date: "2026-08-26T08:20:00Z", parent: { id: 4821, title: "Guest checkout", work_item_type: "Product Backlog Item" } },
    { id: 4805, title: "Sign-in remembers me", work_item_type: "Product Backlog Item", state: "Done", state_color: "339947", column: "Done", assigned_to: "Sam Taylor", tags: "Sign in", priority: 3, changed_date: "2026-08-21T16:05:00Z", parent: null },
```

- [ ] **Step 7: Run** `npx tsc --noEmit` (clean), then `npx vitest run --exclude "**/.claude/**" src/screens/WorkBoard.test.tsx` (unchanged, green).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/work_board/mod.rs src-tauri/src/work_board/board.rs src-tauri/tests/work_board.rs src/bindings.ts src/dev/demo.ts src/tour/tourData.ts
git commit -q -F - <<'EOF'
feat(v2): board cards carry their direct parent

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 2: Lane grouping and its preferences (pure frontend module)

**Files:**
- Create: `src/lib/boardLanes.ts`
- Test: `src/lib/boardLanes.test.ts` (create)

**Interfaces:**
- Consumes: `BoardItem`, `BoardParent` from `../bindings` (Task 1).
- Produces (in `src/lib/boardLanes.ts`):
  - `export const NO_PARENT = 0`
  - `export type Lane = { id: number; parent: BoardParent | null; items: BoardItem[] }`
  - `export function laneIdOf(item: BoardItem): number`
  - `export function groupIntoLanes(items: BoardItem[]): Lane[]`
  - `export function cardCount(n: number): string` (`"1 card"`, `"2 cards"`)
  - `export function laneLabel(lane: Lane): string`
  - `export function laneToggleName(lane: Lane, collapsed: boolean): string`
  - `export const SWIMLANES_KEY = "tcm-v2-board-swimlanes"`, `export function loadSwimlanes(): boolean`, `export function saveSwimlanes(on: boolean): void`
  - `export function collapsedLanesKey(org: string, project: string): string`, `export function loadCollapsedLanes(org: string, project: string): Set<number>`, `export function saveCollapsedLanes(org: string, project: string, ids: Set<number>): void`

- [ ] **Step 1: Write the failing tests** in `src/lib/boardLanes.test.ts`:

```ts
import { afterEach, expect, test } from "vitest";
import type { BoardItem, BoardParent } from "../bindings";
import {
  NO_PARENT,
  cardCount,
  collapsedLanesKey,
  groupIntoLanes,
  laneIdOf,
  laneLabel,
  laneToggleName,
  loadCollapsedLanes,
  loadSwimlanes,
  saveCollapsedLanes,
  saveSwimlanes,
  type Lane,
} from "./boardLanes";

afterEach(() => localStorage.clear());

function card(id: number, parent: BoardParent | null): BoardItem {
  return {
    id,
    title: `Card ${id}`,
    work_item_type: "Task",
    state: "To Do",
    state_color: "",
    column: "To Do",
    assigned_to: "",
    tags: "",
    priority: null,
    changed_date: "",
    parent,
  };
}

/** A card shaped before `parent` existed: the field is missing, not null. */
function legacy(id: number): BoardItem {
  return Object.fromEntries(
    Object.entries(card(id, null)).filter(([k]) => k !== "parent"),
  ) as unknown as BoardItem;
}

const leave: BoardParent = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
const payroll: BoardParent = { id: 600, title: "Payroll export", work_item_type: "Product Backlog Item" };

test("cards group under their parent, lanes in board order, No parent last", () => {
  const lanes = groupIntoLanes([card(1, null), card(2, leave), card(3, payroll), card(4, leave)]);
  expect(lanes.map((l) => l.id)).toEqual([500, 600, NO_PARENT]);
  expect(lanes[0].items.map((i) => i.id)).toEqual([2, 4]);
  expect(lanes[0].parent).toEqual(leave);
  expect(lanes[2].parent).toBeNull();
  expect(lanes[2].items.map((i) => i.id)).toEqual([1]);
});

test("only lanes with cards exist, so a lane the filters emptied is not there", () => {
  expect(groupIntoLanes([])).toEqual([]);
  expect(groupIntoLanes([card(3, payroll)]).map((l) => l.id)).toEqual([600]);
});

test("a card that is itself a parent sits in its own parent's lane", () => {
  const feature: BoardParent = { id: 50, title: "Absence", work_item_type: "Feature" };
  const pbi = { ...card(500, feature), title: "Leave requests", work_item_type: "Product Backlog Item" };
  const lanes = groupIntoLanes([card(2, leave), pbi]);
  expect(lanes.map((l) => [l.id, l.items.map((i) => i.id)])).toEqual([
    [500, [2]],
    [50, [500]],
  ]);
});

/// Review focus 1: the tour's board, or any data shaped before this
/// change, has no `parent` at all.
test("a card with no parent field at all goes to No parent", () => {
  expect(laneIdOf(legacy(7))).toBe(NO_PARENT);
  const lanes = groupIntoLanes([legacy(7), card(2, leave)]);
  expect(lanes.map((l) => l.id)).toEqual([500, NO_PARENT]);
  expect(lanes[1].items.map((i) => i.id)).toEqual([7]);
});

test("a lane is named by its parent's title, its id when unreadable, or No parent", () => {
  const lane = (id: number, parent: BoardParent | null, n: number): Lane => ({
    id,
    parent,
    items: Array.from({ length: n }, (_, i) => card(i + 1, parent)),
  });
  expect(laneLabel(lane(500, leave, 1))).toBe("Leave requests");
  expect(laneLabel(lane(900, { id: 900, title: "", work_item_type: "" }, 1))).toBe("#900");
  expect(laneLabel(lane(NO_PARENT, null, 1))).toBe("No parent");
  expect(laneToggleName(lane(500, leave, 4), false)).toBe("Leave requests, 4 cards, collapse");
  expect(laneToggleName(lane(NO_PARENT, null, 1), true)).toBe("No parent, 1 card, expand");
  expect(cardCount(1)).toBe("1 card");
  expect(cardCount(2)).toBe("2 cards");
});

test("collapsed lanes are remembered per organisation and project, 0 standing for No parent", () => {
  expect(collapsedLanesKey("acme", "Web")).toBe("tcm-v2-board-lanes-collapsed:acme/Web");
  saveCollapsedLanes("acme", "Web", new Set([600, NO_PARENT, 500]));
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/Web")).toBe("[0,500,600]");
  expect(loadCollapsedLanes("acme", "Web")).toEqual(new Set([0, 500, 600]));
  expect(loadCollapsedLanes("acme", "Mobile")).toEqual(new Set());
});

test("unreadable stored lanes are ignored rather than trusted", () => {
  localStorage.setItem("tcm-v2-board-lanes-collapsed:acme/Web", "{not json");
  expect(loadCollapsedLanes("acme", "Web")).toEqual(new Set());
  localStorage.setItem("tcm-v2-board-lanes-collapsed:acme/Web", JSON.stringify([500, "600", -1, 1.5]));
  expect(loadCollapsedLanes("acme", "Web")).toEqual(new Set([500]));
});

test("the Swimlanes switch starts off and is remembered on this machine", () => {
  expect(loadSwimlanes()).toBe(false);
  saveSwimlanes(true);
  expect(localStorage.getItem("tcm-v2-board-swimlanes")).toBe("on");
  expect(loadSwimlanes()).toBe(true);
  saveSwimlanes(false);
  expect(loadSwimlanes()).toBe(false);
});
```

- [ ] **Step 2: Run to see it fail:** `npx vitest run --exclude "**/.claude/**" src/lib/boardLanes.test.ts`. Expected: FAIL, cannot resolve `./boardLanes`.

- [ ] **Step 3: Implement** `src/lib/boardLanes.ts`:

```ts
// Swimlanes on the Work Manager board: cards grouped by their DIRECT
// parent (tasks and bugs under their PBI, PBIs under their Feature), the
// way Azure DevOps' own "group by parent" does - no walking up the tree.
// Pure grouping and naming, plus the two preferences the view remembers
// on this machine.

import type { BoardItem, BoardParent } from "../bindings";

/** The lane id of cards with no parent. Work item ids start at 1. */
export const NO_PARENT = 0;

export type Lane = {
  /** The parent's work item id, or NO_PARENT. */
  id: number;
  /** Null for the No parent lane. Its title is empty when the parent
   * could not be read. */
  parent: BoardParent | null;
  items: BoardItem[];
};

/** A card's lane. `parent` is missing altogether on board data shaped
 * before swimlanes existed, which reads as no parent. */
export function laneIdOf(item: BoardItem): number {
  return item.parent?.id ?? NO_PARENT;
}

/** Group cards into lanes in the order given. The board's order is most
 * recently changed first, so each lane sits where its newest card would.
 * No parent always comes last. Only lanes with cards exist, so a lane the
 * filters emptied is simply not there. */
export function groupIntoLanes(items: BoardItem[]): Lane[] {
  const lanes = new Map<number, Lane>();
  for (const item of items) {
    const id = laneIdOf(item);
    const lane = lanes.get(id);
    if (lane) lane.items.push(item);
    else lanes.set(id, { id, parent: id === NO_PARENT ? null : (item.parent ?? null), items: [item] });
  }
  const all = [...lanes.values()];
  return [...all.filter((l) => l.id !== NO_PARENT), ...all.filter((l) => l.id === NO_PARENT)];
}

export function cardCount(n: number): string {
  return `${n} card${n === 1 ? "" : "s"}`;
}

/** What a lane is called: its parent's title, `#id` when the parent could
 * not be read, or "No parent". */
export function laneLabel(lane: Lane): string {
  if (lane.id === NO_PARENT) return "No parent";
  return lane.parent?.title || `#${lane.id}`;
}

/** The lane toggle's accessible name: "Leave requests, 4 cards, collapse". */
export function laneToggleName(lane: Lane, collapsed: boolean): string {
  return `${laneLabel(lane)}, ${cardCount(lane.items.length)}, ${collapsed ? "expand" : "collapse"}`;
}

export const SWIMLANES_KEY = "tcm-v2-board-swimlanes";

export function loadSwimlanes(): boolean {
  try {
    return localStorage.getItem(SWIMLANES_KEY) === "on";
  } catch {
    return false;
  }
}

export function saveSwimlanes(on: boolean): void {
  try {
    localStorage.setItem(SWIMLANES_KEY, on ? "on" : "off");
  } catch {
    // session-only
  }
}

export function collapsedLanesKey(org: string, project: string): string {
  return `tcm-v2-board-lanes-collapsed:${org}/${project}`;
}

/** The lanes collapsed on this org/project's board: parent ids, 0 for No
 * parent. Anything unreadable is ignored rather than trusted. */
export function loadCollapsedLanes(org: string, project: string): Set<number> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(collapsedLanesKey(org, project)) ?? "[]");
    if (!Array.isArray(raw)) return new Set();
    return new Set(raw.filter((v): v is number => Number.isInteger(v) && v >= 0));
  } catch {
    return new Set();
  }
}

export function saveCollapsedLanes(org: string, project: string, ids: Set<number>): void {
  try {
    localStorage.setItem(
      collapsedLanesKey(org, project),
      JSON.stringify([...ids].sort((a, b) => a - b)),
    );
  } catch {
    // session-only
  }
}
```

- [ ] **Step 4: Run** `npx vitest run --exclude "**/.claude/**" src/lib/boardLanes.test.ts src/lib/cache.test.ts src/ui-consistency.test.ts`, then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/boardLanes.ts src/lib/boardLanes.test.ts
git commit -q -F - <<'EOF'
feat(v2): group board cards into lanes by parent

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 3: The swimlane view on the board

**Files:**
- Modify: `src/lib/actionIcons.ts:56` (add `IconExpandAll`)
- Modify: `src/screens/WorkBoard.tsx` (imports l.3-18; state after l.257; lanes and `renderGrid` after l.428; toolbar after l.539; the grid block l.582-742)
- Modify: `src/a11y.test.tsx` (one test)
- Test: `src/screens/WorkBoard.test.tsx` (append)

**Interfaces:**
- Consumes (Task 2): `NO_PARENT`, `cardCount`, `groupIntoLanes`, `laneIdOf`, `laneLabel`, `laneToggleName`, `loadCollapsedLanes`, `loadSwimlanes`, `saveCollapsedLanes`, `saveSwimlanes`. Consumes (Task 1): `BoardItem.parent`.
- Produces (DOM contract the tests use):
  - Switch `role="switch"` named `Swimlanes`.
  - Buttons `Collapse all` / `Expand all`, shown only while swimlanes are on and there is a lane.
  - Each lane is `data-testid="lane-<id>"` (`lane-0` for No parent) and holds a toggle button named by `laneToggleName`. A parent lane also has a title button named `#<id> <title>` (`#<id>` when unreadable), which opens the drawer.
  - Columns keep `data-testid="col-<column>"` inside every lane.
  - `IconExpandAll` in `actionIcons.ts`.

- [ ] **Step 1: Write the failing tests.** Append to `src/screens/WorkBoard.test.tsx`:

```tsx
// ---- swimlanes --------------------------------------------------------------

const leave = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
const payroll = { id: 600, title: "Payroll export", work_item_type: "Product Backlog Item" };

function task(id: number, title: string, column: string, changed: string, parent: unknown) {
  return {
    id,
    title,
    work_item_type: "Task",
    state: column,
    state_color: "b2b2b2",
    column,
    assigned_to: "Avin",
    tags: "",
    priority: 2,
    changed_date: changed,
    parent,
  };
}

// Board order is newest change first: 500's newest card, then the loose
// one, then 600, 500 again, then the unreadable parent 900.
const laneData = {
  items: [
    task(21, "Draft the form", "To Do", "2026-07-12T05:00:00Z", leave),
    task(22, "Loose task", "To Do", "2026-07-12T04:00:00Z", null),
    task(23, "Wire the API", "In Progress", "2026-07-12T03:00:00Z", payroll),
    task(24, "Review the form", "Done", "2026-07-12T02:00:00Z", leave),
    task(25, "Orphan task", "To Do", "2026-07-12T01:00:00Z", { id: 900, title: "", work_item_type: "" }),
  ],
  states_by_type: boardData.states_by_type,
};

function mockLanes(extra: (cmd: string, args: unknown) => unknown = () => undefined) {
  mockIPC((cmd, args) => {
    if (cmd === "fetch_board") return laneData;
    if (cmd === "classification_paths") return [];
    return extra(cmd, args);
  });
}

const laneIds = () => screen.getAllByTestId(/^lane-\d+$/).map((el) => el.getAttribute("data-testid"));

test("swimlanes are off by default and the board is exactly as before", async () => {
  mockLanes();
  renderBoard();
  await screen.findByText("Draft the form");
  expect(screen.getByRole("switch", { name: "Swimlanes" })).toHaveAttribute("aria-checked", "false");
  expect(screen.queryByTestId("lane-500")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Collapse all" })).not.toBeInTheDocument();
  expect(within(screen.getByTestId("col-To Do")).getByText("Draft the form")).toBeInTheDocument();
  expect(within(screen.getByTestId("col-To Do")).getByText("Loose task")).toBeInTheDocument();
});

test("turning Swimlanes on groups cards by parent, newest first, No parent last, and is remembered", async () => {
  mockLanes();
  renderBoard();
  await screen.findByText("Draft the form");
  fireEvent.click(screen.getByRole("switch", { name: "Swimlanes" }));

  expect(localStorage.getItem("tcm-v2-board-swimlanes")).toBe("on");
  expect(laneIds()).toEqual(["lane-500", "lane-600", "lane-900", "lane-0"]);
  const lane500 = screen.getByTestId("lane-500");
  expect(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, collapse" })).toBeInTheDocument();
  expect(within(within(lane500).getByTestId("col-To Do")).getByText("Draft the form")).toBeInTheDocument();
  expect(within(within(lane500).getByTestId("col-Done")).getByText("Review the form")).toBeInTheDocument();
  expect(within(lane500).queryByText("Wire the API")).not.toBeInTheDocument();
  // Unreadable parent: the lane reads its id.
  expect(within(screen.getByTestId("lane-900")).getByRole("button", { name: "#900, 1 card, collapse" })).toBeInTheDocument();
  // No parent has no title to open.
  const loose = screen.getByTestId("lane-0");
  expect(within(loose).getByRole("button", { name: "No parent, 1 card, collapse" })).toBeInTheDocument();
  expect(within(loose).queryByRole("button", { name: /^#/ })).not.toBeInTheDocument();
});

test("filters apply first, so a lane they empty is not shown", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  renderBoard();
  await screen.findByTestId("lane-500");
  fireEvent.change(screen.getByLabelText("Filter items"), { target: { value: "wire" } });
  expect(laneIds()).toEqual(["lane-600"]);
});

test("a lane collapses to its header and stays collapsed next time", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  const view = renderBoard();
  const lane500 = await screen.findByTestId("lane-500");
  fireEvent.click(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, collapse" }));

  expect(within(lane500).queryByText("Draft the form")).not.toBeInTheDocument();
  expect(within(lane500).getByRole("button", { name: "Leave requests, 2 cards, expand" })).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/Web")).toBe("[500]");

  view.unmount();
  renderBoard();
  const again = await screen.findByTestId("lane-500");
  expect(within(again).getByRole("button", { name: "Leave requests, 2 cards, expand" })).toBeInTheDocument();
  expect(within(again).queryByText("Draft the form")).not.toBeInTheDocument();
  expect(within(screen.getByTestId("lane-600")).getByText("Wire the API")).toBeInTheDocument();
});

test("Collapse all folds every lane on screen and Expand all opens them again", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  mockLanes();
  renderBoard();
  await screen.findByTestId("lane-500");

  fireEvent.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(screen.getAllByRole("button", { name: /, expand$/ })).toHaveLength(4);
  expect(screen.queryByText("Draft the form")).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/Web")).toBe("[0,500,600,900]");

  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  expect(screen.getAllByRole("button", { name: /, collapse$/ })).toHaveLength(4);
  expect(screen.getByText("Draft the form")).toBeInTheDocument();
});

test("a card can be dropped only in its own lane's columns", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const moves: unknown[] = [];
  mockLanes((cmd, args) => {
    if (cmd === "move_board_item") {
      moves.push(args);
      return "In Progress";
    }
  });
  renderBoard();
  const card = (await screen.findByText("Draft the form")).closest("[draggable]")!;

  fireEvent.dragStart(card);
  fireEvent.drop(within(screen.getByTestId("lane-600")).getByTestId("col-In Progress"));
  expect(moves).toEqual([]);
  expect(within(screen.getByTestId("lane-500")).getByText("Draft the form")).toBeInTheDocument();

  fireEvent.dragStart(card);
  fireEvent.drop(within(screen.getByTestId("lane-500")).getByTestId("col-In Progress"));
  await vi.waitFor(() => expect(moves).toHaveLength(1));
  expect(moves[0]).toMatchObject({ itemId: 21, column: "In Progress" });
});

test("clicking a lane's title opens that parent in the drawer", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const opened: number[] = [];
  mockLanes((cmd, args) => {
    if (cmd === "work_item_detail") {
      const id = (args as { id: number }).id;
      opened.push(id);
      return {
        id,
        title: "Leave requests",
        work_item_type: "Product Backlog Item",
        state: "Committed",
        assigned_to: "",
        assigned_to_unique: "",
        activity: "",
        tags: "",
        area_path: "P",
        iteration_path: "P\\S1",
        remaining_work: null,
        completed_work: null,
        original_estimate: null,
        start_date: "",
        target_date: "",
        description_text: "",
        description_html: "",
        description_field: "System.Description",
        extra_pages: [],
        extra_pages_error: null,
        inline_images: [],
      };
    }
    if (cmd === "list_team_members") return [];
    if (cmd === "activity_values") return [];
    if (cmd === "work_item_comments") return [];
  });
  renderBoard();
  fireEvent.click(await screen.findByRole("button", { name: "#500 Leave requests" }));
  await vi.waitFor(() => expect(opened).toContain(500));
});

test("a hidden column is hidden in every lane alike", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  localStorage.setItem("tcm-v2-hidden-cols", JSON.stringify(["Done"]));
  mockLanes();
  renderBoard();
  await screen.findByTestId("lane-500");
  for (const id of ["lane-500", "lane-600", "lane-900", "lane-0"]) {
    expect(within(screen.getByTestId(id)).getByRole("button", { name: "Open Done" })).toBeInTheDocument();
  }
});
```

In `src/a11y.test.tsx`, add `import WorkBoard from "./screens/WorkBoard";` after the `SignIn` import and append:

```tsx
test("Work Manager board with swimlanes is accessible", async () => {
  localStorage.setItem("tcm-v2-board-swimlanes", "on");
  const item = (id: number, title: string, parent: unknown) => ({
    id, title, work_item_type: "Task", state: "To Do", state_color: "b2b2b2", column: "To Do",
    assigned_to: "Avin", tags: "", priority: 2, changed_date: "2026-09-24T00:00:00Z", parent,
  });
  mockIPC((cmd) => {
    if (cmd === "fetch_board")
      return {
        items: [
          item(21, "Draft the form", { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" }),
          item(22, "Loose task", null),
        ],
        states_by_type: {},
      };
    if (cmd === "classification_paths") return [];
    if (cmd === "board_pr_links") return [];
  });
  await expectAccessible(<WorkBoard org="acme" project="Web" />);
});
```

If axe reports a violation in board markup that this task did not add (the columns, the cards, the toolbar), stop and report it to the controller. Do not change the rule set.

- [ ] **Step 2: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/screens/WorkBoard.test.tsx`. Expected: the new tests fail (no switch named `Swimlanes`); the old ones pass.

- [ ] **Step 3: Add the icon.** In `src/lib/actionIcons.ts`, replace `  ChevronsDownUp as IconCollapseAll,` with:

```ts
  ChevronsDownUp as IconCollapseAll,
  // The opposite of IconCollapseAll: every folded group or lane opens.
  ChevronsUpDown as IconExpandAll,
```

- [ ] **Step 4: WorkBoard imports.** In `src/screens/WorkBoard.tsx`:
  - Replace `import { Check, GitPullRequest, RefreshCw } from "lucide-react";` with `import { Check, ChevronDown, ChevronRight, GitPullRequest, RefreshCw } from "lucide-react";`
  - Replace `import { Badge } from "../components/ui/badge";` with:

```ts
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
```

  - Replace `import { Skeleton } from "../components/ui/skeleton";` with:

```ts
import { Skeleton } from "../components/ui/skeleton";
import { Switch } from "../components/ui/switch";
```

  - Replace `import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";` with:

```ts
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { IconCollapseAll, IconExpandAll } from "../lib/actionIcons";
import {
  NO_PARENT,
  cardCount,
  groupIntoLanes,
  laneIdOf,
  laneLabel,
  laneToggleName,
  loadCollapsedLanes,
  loadSwimlanes,
  saveCollapsedLanes,
  saveSwimlanes,
} from "../lib/boardLanes";
```

- [ ] **Step 5: The swimlane state.** Replace:

```ts
  // Per-area, session-only (an assignee list rarely transfers between areas).
  const [assigneeFilter, setAssigneeFilter] = useState<string[]>([]);
```

with:

```ts
  // Per-area, session-only (an assignee list rarely transfers between areas).
  const [assigneeFilter, setAssigneeFilter] = useState<string[]>([]);
  // Swimlanes by direct parent: a view option, off by default, remembered
  // on this machine. Collapsed lanes are remembered per org/project.
  const [swimlanes, setSwimlanes] = useState(loadSwimlanes);
  const changeSwimlanes = (on: boolean) => {
    setSwimlanes(on);
    saveSwimlanes(on);
  };
  const [collapsedLanes, setCollapsedLanes] = useState<Set<number>>(() =>
    loadCollapsedLanes(org, project),
  );
  useEffect(() => {
    setCollapsedLanes(loadCollapsedLanes(org, project));
  }, [org, project]);
  const updateCollapsed = (next: Set<number>) => {
    setCollapsedLanes(next);
    saveCollapsedLanes(org, project, next);
  };
  const toggleLane = (id: number) => {
    const next = new Set(collapsedLanes);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    updateCollapsed(next);
  };
```

- [ ] **Step 6: Lanes and the per-lane grid.** Replace:

```ts
    return true;
  });

  if (!org || !project) {
```

with the following. The JSX inside `renderGrid` is the existing grid block (l.588-741) moved here. It has exactly four changes: `data-tour` is set only when there are no lanes; `visible.filter` becomes `cards.filter`; `onDragOver` asks `canDrop`; and `onDrop` asks `canDrop`.

```tsx
    return true;
  });

  // Swimlanes: the filters above apply first, so a lane they emptied never
  // appears.
  const lanes = swimlanes ? groupIntoLanes(visible) : [];
  const collapseAllLanes = () =>
    updateCollapsed(new Set([...collapsedLanes, ...lanes.map((l) => l.id)]));
  const expandAllLanes = () => updateCollapsed(new Set());
  // A drop lands only in the dragged card's own lane: a column changes a
  // card's state, and nothing on this board changes a parent.
  const canDrop = (laneId: number | null) =>
    dragging !== null && (laneId === null || laneIdOf(dragging) === laneId);

  /** One To Do / In Progress / Done grid holding `cards`: the whole board
   * with swimlanes off (`laneId` null), or one lane's cards. */
  const renderGrid = (cards: BoardItem[], laneId: number | null) => (
    // Hidden columns collapse to a slim rail (never to nothing) so the
    // control that restores them stays visible; the track animation
    // glides the open columns wider. Cards aren't rendered while
    // collapsed - a 0-width column's wrapped cards once made the board
    // scroll far past the visible items. Every lane shares the one set of
    // hidden columns.
    <div
      data-tour={laneId === null ? "board-columns" : undefined}
      className="grid gap-3 transition-[grid-template-columns] duration-300 ease-out"
      style={{
        // All-fr on purpose: Chromium can't interpolate fr<->px track
        // lists and leaves the transition STUCK at the start value.
        // A 0fr track still floors at its content's min size - the
        // rail's fixed w-9 - so hidden columns settle at 36px.
        // A hiding column keeps its full track while its content
        // fades ("fadeOut"); only then does the track collapse.
        gridTemplateColumns: COLUMNS.map((c) =>
          hiddenCols.has(c) && colAnim[c] !== "fadeOut" ? "0fr" : "1fr",
        ).join(" "),
      }}
    >
      {COLUMNS.map((col) => {
        const collapsed = hiddenCols.has(col) && colAnim[col] !== "fadeOut";
        const contentInvisible = colAnim[col] === "fadeOut" || colAnim[col] === "grow";
        const items = cards.filter((i) => i.column === col);
        if (collapsed) {
          return (
            <div
              key={col}
              data-testid={`col-${col}`}
              // Fixed w-9 (no min-w-0): this is the 0fr track's floor.
              className="rail-in flex w-9 flex-col items-center gap-2 rounded-md border border-border bg-bg py-2"
            >
              {/* Reads top-to-bottom in the rail, same as the label
                  below it - the collapsed column is a vertical strip,
                  so the control is too. In the accent, because a
                  muted control on a 36px rail is easy to miss
                  entirely, and this is the only way back.
                  `text-center` centres the label along the axis it
                  runs down; the flex centres the box across the
                  rail.

                  The padding is spelled out PHYSICALLY on purpose.
                  Tailwind mixes the two systems - px/py are logical
                  (padding-inline/block) while pl/pr/pt/pb are
                  physical - and under vertical-rl the logical pair
                  swaps axes, so `py` silently becomes left/right.
                  Naming the sides directly means what you read is
                  where the space goes.

                  pt/pb-2 is the room above and below the word.
                  pl-0/pr-0.5 is the sliver across it, and the
                  lopsided 0/2 is measured, not eyeballed: in caps
                  the ink runs ascent 8 / descent 0 while the font
                  box is 9 / 3, so the glyphs sit 1px off the em-box
                  centre. With `leading-none` the whole control is
                  14px across - it hugs the text, which is the only
                  thing it has to fit.

                  The exact splits here and on Hide were read off the
                  RENDERED PIXELS, not derived: font metrics predict
                  the direction but not the amount, and at this size
                  half a pixel is visible. If the font or size
                  changes, measure again rather than reasoning.

                  Both stop about half a pixel short of perfect, and
                  that is a floor rather than a missing tweak: glyph
                  baselines snap to whole pixels, so fractional
                  padding below 1px moves nothing. Closing the last
                  half pixel would mean changing the box height, not
                  the padding. */}
              {/* The label is HORIZONTAL text rotated as a finished
                  box - not writing-mode text. vertical-rl rasterizes
                  each rotated glyph, and that path's baseline snap is
                  state-dependent: the first repaint after mount (or
                  WebView2's hover repaint) could re-snap the run ~1px
                  along the reading axis, so OPEN sat centred until you
                  hovered and then rode up. A transform rotates the
                  already-rasterized horizontal run as one unit - the
                  same pipeline as the Hide button, which never moved -
                  so every repaint lands identically. The box is sized
                  explicitly because a transform does not change
                  layout: h-11 reads as the old padded strip, w-4
                  spans the glyph cross-axis in the 36px rail. */}
              <button
                aria-label={`Open ${col}`}
                title={`Open ${col}`}
                className="relative h-11 w-4 self-center rounded border border-accent/60 text-accent transition-colors hover:bg-accent-soft"
                onClick={() => toggleCol(col)}
              >
                <span className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 rotate-90 whitespace-nowrap text-[10px] font-semibold uppercase leading-none tracking-wide">
                  Open
                </span>
              </button>
              <span
                className="whitespace-nowrap text-[10px] font-semibold uppercase tracking-wide text-faint"
                style={{ writingMode: "vertical-rl" }}
              >
                {col} · {items.length}
              </span>
            </div>
          );
        }
        return (
          <div
            key={col}
            data-testid={`col-${col}`}
            className="min-w-0 overflow-hidden rounded-md border border-border bg-bg p-2"
            onDragOver={(e) => {
              if (canDrop(laneId)) e.preventDefault();
            }}
            onDrop={() => {
              if (dragging && canDrop(laneId) && dragging.column !== col) {
                move.mutate({ item: dragging, column: col });
              }
              setDragging(null);
            }}
          >
            {/* Fades as one unit: out before the column shrinks, in
                after it finishes widening - card text never visibly
                re-wraps while the width animates. */}
            <div
              className={cn(
                "space-y-2 transition-opacity duration-150",
                contentInvisible ? "opacity-0" : "opacity-100",
              )}
            >
            <h3 className="flex items-center whitespace-nowrap px-1 text-xs font-semibold uppercase tracking-wide text-muted">
              {col} <span className="ml-1 text-faint">{items.length}</span>
              <button
                aria-label={`Hide ${col}`}
                title={
                  hiddenCols.size >= COLUMNS.length - 1
                    ? "At least one column must stay visible"
                    : `Hide ${col}`
                }
                disabled={hiddenCols.size >= COLUMNS.length - 1}
                className="ml-auto rounded border border-accent/60 px-1.5 py-0.5 text-center text-[10px] font-semibold uppercase tracking-wide text-accent transition-colors hover:bg-accent-soft disabled:cursor-not-allowed disabled:opacity-40"
                onClick={() => toggleCol(col)}
              >
                {/* Symmetric padding + the caps-only ink shift, not a
                    hand-tuned pt/pb pair: the old pt-[3px] pb-px was a
                    1px nudge where caps ink needs 1.5px (measured
                    -0.66px high), and its 1px bottom padding read as
                    "missing" in an inspector. Same button height. */}
                <span className="pill-label-ink">Hide</span>
              </button>
            </h3>
            {items.map((item) => (
              <Card
                key={item.id}
                item={item}
                prLinks={prByItem.get(item.id)}
                onDragStart={() => setDragging(item)}
                onOpen={() => setOpenItem(item.id)}
              />
            ))}
            </div>
          </div>
        );
      })}
    </div>
  );

  if (!org || !project) {
```

- [ ] **Step 7: The toolbar.** Replace:

```tsx
              This sprint
            </label>
          )}
```

with:

```tsx
              This sprint
            </label>
          )}
          <label className="flex items-center gap-1.5 text-xs text-muted" title="Group cards under their parent work item">
            <Switch checked={swimlanes} onCheckedChange={changeSwimlanes} ariaLabel="Swimlanes" />
            Swimlanes
          </label>
          {swimlanes && lanes.length > 0 && (
            <>
              <Button size="sm" variant="ghost" onClick={collapseAllLanes}>
                <IconCollapseAll aria-hidden />
                Collapse all
              </Button>
              <Button size="sm" variant="ghost" onClick={expandAllLanes}>
                <IconExpandAll aria-hidden />
                Expand all
              </Button>
            </>
          )}
```

- [ ] **Step 8: Render grid or lanes.** Delete the old grid block. It starts at the line `        {board.data && (` directly after the "Nothing assigned to you" paragraph's closing `        )}` (~l.582). It ends at its matching `        )}` (~l.742), the line just before `      </div>` and then `      {openItem != null && board.data && (`. Put this in its place:

```tsx
        {board.data && !swimlanes && renderGrid(visible, null)}

        {board.data && swimlanes && (
          <div data-tour="board-columns" className="space-y-3">
            {lanes.map((lane) => {
              const collapsed = collapsedLanes.has(lane.id);
              return (
                <div
                  key={lane.id}
                  data-testid={`lane-${lane.id}`}
                  className="space-y-2 rounded-md border border-border bg-surface p-2"
                >
                  {/* The header row. The toggle carries the lane's name,
                      its size and what a press will do; the title opens
                      the parent in the drawer, the way a card click does.
                      A collapsed lane is just this row. */}
                  <div className="flex min-w-0 items-center gap-2 px-1">
                    <button
                      aria-label={laneToggleName(lane, collapsed)}
                      aria-expanded={!collapsed}
                      title={collapsed ? "Expand lane" : "Collapse lane"}
                      className="shrink-0 rounded p-0.5 text-muted transition-colors hover:text-accent"
                      onClick={() => toggleLane(lane.id)}
                    >
                      {collapsed ? <ChevronRight size={15} aria-hidden /> : <ChevronDown size={15} aria-hidden />}
                    </button>
                    {lane.parent?.work_item_type && (
                      <Badge color={typeColor[lane.parent.work_item_type] ?? "#9ca3af"}>
                        {lane.parent.work_item_type}
                      </Badge>
                    )}
                    {lane.id === NO_PARENT ? (
                      <span className="text-sm font-semibold text-muted">{laneLabel(lane)}</span>
                    ) : (
                      <button
                        className="min-w-0 truncate text-left text-sm font-semibold text-text hover:text-accent hover:underline"
                        title={`Open #${lane.id}`}
                        onClick={() => setOpenItem(lane.id)}
                      >
                        <span className="id-mono text-xs text-faint">#{lane.id}</span>
                        {lane.parent?.title ? ` ${lane.parent.title}` : ""}
                      </button>
                    )}
                    <span className="shrink-0 text-xs text-faint">{cardCount(lane.items.length)}</span>
                  </div>
                  {!collapsed && renderGrid(lane.items, lane.id)}
                </div>
              );
            })}
          </div>
        )}
```

- [ ] **Step 9: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/screens/WorkBoard.test.tsx src/lib/boardLanes.test.ts src/ui-consistency.test.ts src/a11y.test.tsx`, then `npx tsc --noEmit`. All green, including every WorkBoard test that existed before this task.

- [ ] **Step 10: Commit**

```bash
git add src/lib/actionIcons.ts src/screens/WorkBoard.tsx src/screens/WorkBoard.test.tsx src/a11y.test.tsx
git commit -q -F - <<'EOF'
feat(v2): swimlanes on the Work Manager board

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 4: Work-item mentions and PR comment authors (Rust)

**Files:**
- Create: `src-tauri/src/work_board/mentions.rs`
- Modify: `src-tauri/src/work_board/mod.rs:14` (add `pub mod mentions;` after `pub mod history;`)
- Modify: `src-tauri/src/cache/keys.rs` (append)
- Modify: `src-tauri/src/commands/board.rs:242-251` (`connected_user`, and add `recent_mentions`)
- Modify: `src-tauri/src/lib.rs:139` (register after `board::connected_user,`)
- Modify: `src-tauri/src/ado_git.rs:83-95` (`PrComment.author_id`) and `:489-498` (fill it)
- Modify: `src/dev/demo.ts` (`recentMentions` stub, `author_id` on demo PR comments)
- Test: `src-tauri/tests/mentions.rs` (create), `src-tauri/tests/ado_git.rs` (append)
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes (Task 1): `AdoClient::read_titles(org, project, &[i32]) -> HashMap<i32, (String, String)>`. Existing: `AdoClient::query_work_items(org, project, wiql, top)`, `AdoClient::connected_user(org)`, `crate::steps_xml::html_to_text`, `crate::cache::{session_fresh, session_put}`.
- Produces:
  - `v2_lib::work_board::mentions::Mention { pub source: String, pub item_id: i32, pub item_type: String, pub item_title: String, pub comment_id: i32, pub author: String, pub excerpt: String, pub created_date: String }` (`Debug, Clone, PartialEq, Serialize, specta::Type`)
  - `pub const RECENT_MENTIONS_WIQL: &str`, `pub const MENTION_ITEMS: u32 = 20`, `pub const MENTION_COMMENTS: u32 = 50`, `pub const EXCERPT_CHARS: usize = 140`
  - `pub fn mentions_me(html: &str, me: &str) -> bool`, `pub fn excerpt(html: &str) -> String`, `pub fn mentions_in(item_id: i32, item_type: &str, item_title: &str, answer: &serde_json::Value, me: &str) -> Vec<Mention>`
  - `impl AdoClient { pub async fn connected_user_cached(&self, org: &str) -> Result<ConnectedUser, AdoError>; pub async fn recent_mentions(&self, org: &str, project: &str, me: &str) -> Result<Vec<Mention>, AdoError> }`
  - `cache::keys::CONNECTED_USER_TTL: Duration`, `cache::keys::connected_user(base_url: &str, org: &str) -> String`
  - Command `recent_mentions(organization, project) -> Result<Vec<Mention>, AdoError>` (TS `commands.recentMentions(organization, project)`).
  - `PrComment.author_id: String` (TS `author_id: string`).

- [ ] **Step 1: Write the failing tests.** Create `src-tauri/tests/mentions.rs`:

```rust
//! Work-item @mentions of the signed-in user, the bell's work-item source:
//! which items are read, which comments count, and what a failure skips.

use v2_lib::ado::{AdoClient, AdoError};
use v2_lib::work_board::mentions::{excerpt, mentions_me, Mention, EXCERPT_CHARS};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ME: &str = "me-guid";

/// A mention the way Azure DevOps stores one in a comment's HTML.
fn anchor(id: &str, name: &str) -> String {
    format!("<a href=\"#\" data-vss-mention=\"version:2.0,{id}\">@{name}</a>")
}

fn comment(id: i32, author_id: &str, author: &str, html: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "text": html,
        "createdBy": { "id": author_id, "displayName": author },
        "createdDate": "2026-09-25T08:00:00Z"
    })
}

/// The WIQL, matched on everything the query must say.
async fn mount_query(server: &MockServer, ids: &[i32]) {
    let items: Vec<serde_json::Value> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .and(query_param("$top", "20"))
        .and(body_string_contains("[System.TeamProject] = @project"))
        .and(body_string_contains("[System.Id] IN (@RecentMentions)"))
        .and(body_string_contains("ORDER BY [System.ChangedDate] DESC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "workItems": items })))
        .mount(server)
        .await;
}

async fn mount_titles(server: &MockServer, value: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitems"))
        .and(query_param("errorPolicy", "omit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": value })))
        .mount(server)
        .await;
}

async fn mount_comments(server: &MockServer, item: i32, comments: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/org/proj/_apis/wit/workItems/{item}/comments")))
        .and(query_param("order", "desc"))
        .and(query_param("$top", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "comments": comments })))
        .mount(server)
        .await;
}

fn client(server: &MockServer) -> AdoClient {
    AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
}

#[tokio::test]
async fn a_comment_mentioning_you_is_found_with_its_item_and_excerpt() {
    let server = MockServer::start().await;
    mount_query(&server, &[41]).await;
    mount_titles(
        &server,
        serde_json::json!([{ "id": 41, "fields": {
            "System.Title": "Leave requests", "System.WorkItemType": "Product Backlog Item"
        } }]),
    )
    .await;
    // The anchor carries the id in upper case: the match ignores case.
    let html = format!("<div>{} can you   check<br>this?</div>", anchor("ME-GUID", "Avin"));
    mount_comments(&server, 41, vec![comment(7, "u-sam", "Sam", &html)]).await;

    let found = client(&server).recent_mentions("org", "proj", ME).await.unwrap();

    assert_eq!(
        found,
        vec![Mention {
            source: "work-item".into(),
            item_id: 41,
            item_type: "Product Backlog Item".into(),
            item_title: "Leave requests".into(),
            comment_id: 7,
            author: "Sam".into(),
            excerpt: "@Avin can you check this?".into(),
            created_date: "2026-09-25T08:00:00Z".into(),
        }]
    );
}

#[tokio::test]
async fn your_own_comments_and_mentions_of_someone_else_are_ignored() {
    let server = MockServer::start().await;
    mount_query(&server, &[41]).await;
    mount_titles(&server, serde_json::json!([])).await;
    mount_comments(
        &server,
        41,
        vec![
            comment(1, "ME-GUID", "Avin", &anchor(ME, "Avin")),
            comment(2, "u-sam", "Sam", &anchor("kim-guid", "Kim")),
            comment(3, "u-sam", "Sam", &anchor(ME, "Avin")),
        ],
    )
    .await;

    let found = client(&server).recent_mentions("org", "proj", ME).await.unwrap();

    assert_eq!(found.iter().map(|m| m.comment_id).collect::<Vec<_>>(), vec![3]);
    assert_eq!(found[0].item_type, "Work item", "an item whose title could not be read");
    assert_eq!(found[0].item_title, "");
}

#[test]
fn the_excerpt_strips_html_collapses_whitespace_and_stops_at_140_characters() {
    assert_eq!(excerpt("<div><b>Tom</b> &amp; Jerry,\n\n  <i>please</i></div>"), "Tom & Jerry, please");
    let long = format!("<p>{}</p>", "word ".repeat(60));
    let cut = excerpt(&long);
    assert_eq!(cut.chars().count(), EXCERPT_CHARS);
    assert!(cut.ends_with('…'), "{cut}");
    assert!(!cut.contains('<'));
}

#[test]
fn a_mention_matches_the_exact_anchor_ignoring_case() {
    assert!(mentions_me(&anchor("ME-GUID", "Avin"), "me-guid"));
    assert!(!mentions_me(&anchor("me-guid-2", "Other"), "me-guid"));
    assert!(!mentions_me("<div>@Avin in plain text</div>", "me-guid"));
}

#[tokio::test]
async fn one_items_failed_comment_read_skips_only_that_item() {
    let server = MockServer::start().await;
    mount_query(&server, &[41, 42]).await;
    mount_titles(&server, serde_json::json!([])).await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workItems/41/comments"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    mount_comments(&server, 42, vec![comment(9, "u-sam", "Sam", &anchor(ME, "Avin"))]).await;

    let found = client(&server).recent_mentions("org", "proj", ME).await.unwrap();

    assert_eq!(found.iter().map(|m| (m.item_id, m.comment_id)).collect::<Vec<_>>(), vec![(42, 9)]);
}

#[tokio::test]
async fn a_failed_query_fails_the_check() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/org/proj/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let err = client(&server).recent_mentions("org", "proj", ME).await.unwrap_err();

    assert!(matches!(err, AdoError::Http { status: 500, .. }), "{err:?}");
}

/// Review focus 3: every mention anchor starts with the same prefix, so an
/// empty id would match every mention of anyone. It matches nothing, and
/// asks Azure DevOps nothing.
#[tokio::test]
async fn an_identity_without_an_id_matches_nothing() {
    let server = MockServer::start().await;

    let found = client(&server).recent_mentions("org", "proj", "  ").await.unwrap();

    assert!(found.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(!mentions_me(&anchor("", "Nobody"), ""));
}

/// Read once per organization per session, memory only.
#[tokio::test]
async fn the_signed_in_identity_is_read_once_per_organization() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/_apis/connectionData"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authenticatedUser": { "id": "u-ada", "providerDisplayName": "Ada Lovelace" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);

    assert_eq!(c.connected_user_cached("org").await.unwrap().id, "u-ada");
    assert_eq!(c.connected_user_cached("org").await.unwrap().id, "u-ada");
}
```

Append to `src-tauri/tests/ado_git.rs`:

```rust
/// The mention scan skips your own PR comments by identity id, so each
/// comment carries its author's id beside the display name.
#[tokio::test]
async fn pr_comments_carry_the_authors_identity_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/o/p/_apis/git/repositories/r/pullRequests/1/threads"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
            { "id": 1, "status": "active", "lastUpdatedDate": "2026-09-25T00:00:00Z",
              "comments": [
                { "id": 1, "content": "@<me-guid> look", "author": { "displayName": "Priya", "id": "u-priya" } },
                { "id": 2, "content": "no id here", "author": { "displayName": "Bot" } } ] },
        ]})))
        .mount(&server)
        .await;

    let out = AdoClient::with_base_url("t".into(), server.uri())
        .pr_threads("o", "p", "r", 1)
        .await
        .unwrap();

    assert_eq!(out[0].comments[0].author_id, "u-priya");
    assert_eq!(out[0].comments[1].author_id, "");
}
```

- [ ] **Step 2: Run to see them fail** (dev-app check first): `CARGO_TARGET_DIR=target/gate cargo test --test mentions`, then `--test ado_git`. Expected: compile errors, `could not find mentions in work_board` and `no field author_id`.

- [ ] **Step 3: The cache key.** Append to `src-tauri/src/cache/keys.rs`:

```rust

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
```

- [ ] **Step 4: The module.** Create `src-tauri/src/work_board/mentions.rs`:

```rust
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
```

In `src-tauri/src/work_board/mod.rs`, change:

```rust
pub mod history;
mod layout;
```

to:

```rust
pub mod history;
mod layout;
pub mod mentions;
```

- [ ] **Step 5: The commands.** In `src-tauri/src/commands/board.rs`, replace the whole `connected_user` command (the `/// Who is signed in, ...` doc through its closing `}`) with:

```rust
/// Who is signed in, by identity id - so Edit shows only on one's own
/// comments. Read once per organization per session, memory only.
#[tauri::command]
#[specta::specta]
pub async fn connected_user(
    app: tauri::AppHandle,
    organization: String,
) -> Result<work_board::ConnectedUser, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token).connected_user_cached(&organization).await
}

/// Work-item comments that @mention the signed-in user, for the bell.
/// Read only.
#[tauri::command]
#[specta::specta]
pub async fn recent_mentions(
    app: tauri::AppHandle,
    organization: String,
    project: String,
) -> Result<Vec<work_board::mentions::Mention>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    let client = ado::AdoClient::new(token);
    let me = client.connected_user_cached(&organization).await?;
    client.recent_mentions(&organization, &project, &me.id).await
}
```

In `src-tauri/src/lib.rs`, replace `            board::connected_user,` with:

```rust
            board::connected_user,
            board::recent_mentions,
```

- [ ] **Step 6: `PrComment.author_id`.** In `src-tauri/src/ado_git.rs`, replace:

```rust
pub struct PrComment {
    pub id: i32,
    pub author: String,
```

with:

```rust
pub struct PrComment {
    pub id: i32,
    pub author: String,
    /// The author's identity id, the one `connected_user` returns - how the
    /// mention scan tells your own comments apart. Empty when Azure DevOps
    /// sent none.
    pub author_id: String,
```

and in `pr_threads`, replace:

```rust
                    author: c["author"]["displayName"].as_str().unwrap_or("").to_string(),
```

with:

```rust
                    author: c["author"]["displayName"].as_str().unwrap_or("").to_string(),
                    author_id: c["author"]["id"].as_str().unwrap_or("").to_string(),
```

- [ ] **Step 7: Run** (one at a time, dev-app check before each): `--test mentions`, `--test ado_git`, `--test work_board`, `--test cache`, `--test ado` (the no-DELETE scan), `--test ado_network`, then `--test bindings`. The last regenerates `src/bindings.ts` with `recentMentions`, `Mention` and `PrComment.author_id`. All pass.

- [ ] **Step 8: The demo stubs.** In `src/dev/demo.ts`, replace:

```ts
    connectedUser: () => ok({ id: "demo", display_name: "Demo User" }),
```

with:

```ts
    connectedUser: () => ok({ id: "demo", display_name: "Demo User" }),
    // One mention on the demo bug, two hours old, so the bell has one to show.
    recentMentions: () =>
      ok([
        {
          source: "work-item", item_id: 2003, item_type: "Bug",
          item_title: "Demo Bug - session timeout not enforced", comment_id: 2,
          author: "Sam Doyle", excerpt: "@Demo User can you confirm the timeout on the demo build?",
          created_date: nowMinus(2 * 3600),
        },
      ]),
```

In the demo `prThreads`, change the three comment heads `id: 1, author: "Priya Raman", avatar: "", edited: false,` to `id: 1, author: "Priya Raman", author_id: "demo-priya", avatar: "", edited: false,`, `id: 2, author: "Sam Doyle", avatar: "", edited: true,` to `id: 2, author: "Sam Doyle", author_id: "demo-sam", avatar: "", edited: true,`, and `id: 3, author: "Priya Raman", avatar: "", edited: false,` to `id: 3, author: "Priya Raman", author_id: "demo-priya", avatar: "", edited: false,`.

- [ ] **Step 9: Run** `npx tsc --noEmit` (clean).

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/work_board/mentions.rs src-tauri/src/work_board/mod.rs src-tauri/src/cache/keys.rs src-tauri/src/commands/board.rs src-tauri/src/lib.rs src-tauri/src/ado_git.rs src-tauri/tests/mentions.rs src-tauri/tests/ado_git.rs src/bindings.ts src/dev/demo.ts
git commit -q -F - <<'EOF'
feat(v2): find work-item comments that mention you

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 5: Mentions become bell notifications (pure frontend and the bell)

**Files:**
- Modify: `src/lib/notifications.ts:18` (kind) and after `raise` (~l.131) (`markSeen`)
- Create: `src/lib/mentions.ts`
- Modify: `src/components/NotificationBell.tsx:25-37`
- Test: `src/lib/mentions.test.ts` (create), `src/lib/notifications.test.ts` (append), `src/components/NotificationBell.test.tsx` (append)

**Interfaces:**
- Consumes (Task 4): `Mention`, `PrThread` (with `comments[].author_id`), `PullRequest` from `../bindings`. Existing: `raise`, `AppNotification`, `appIsInView`, `osNotify`, `toast`.
- Produces:
  - `NotificationKind` includes `"mention"`.
  - `markSeen(org: string, ids: string[]): void` in `notifications.ts`.
  - In `src/lib/mentions.ts`:
    - `export type NewNotification = Omit<AppNotification, "at" | "read">`
    - `export type PrMention = { repo: string; prId: number; threadId: number; commentId: number; author: string; excerpt: string; createdDate: string }`
    - `export type FoundMention = { notification: NewNotification; created: string }`
    - `export const EXCERPT_CHARS = 140`, `export const FIRST_RUN_WINDOW_MS = 24 * 60 * 60_000`
    - `export function excerpt(text: string): string`
    - `export function workItemMentionId(m: { item_id: number; comment_id: number }): string`
    - `export function prMentionId(m: { repo: string; prId: number; threadId: number; commentId: number }): string`
    - `export function workItemNotification(org: string, project: string, m: Mention): NewNotification`
    - `export function prNotification(org: string, project: string, m: PrMention): NewNotification`
    - `export function prMentions(pr: { repo: string; id: number }, threads: PrThread[], me: string): PrMention[]`
    - `export function noteMentions(org: string, found: FoundMention[], now?: number): AppNotification[]`
    - `export function announceMentions(added: AppNotification[]): void`

- [ ] **Step 1: Write the failing tests.** Create `src/lib/mentions.test.ts`:

```ts
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Mention, PrThread } from "../bindings";
import { appIsInView, osNotify } from "./assignedAlerts";
import {
  announceMentions,
  excerpt,
  noteMentions,
  prMentionId,
  prMentions,
  prNotification,
  workItemMentionId,
  workItemNotification,
  type FoundMention,
} from "./mentions";
import { resetForTests, type AppNotification } from "./notifications";
import { toast } from "./toast";

vi.mock("./toast", () => ({ toast: { info: vi.fn() } }));
vi.mock("./assignedAlerts", () => ({
  appIsInView: vi.fn(() => true),
  osNotify: vi.fn(() => Promise.resolve(true)),
}));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
  vi.mocked(toast.info).mockClear();
  vi.mocked(appIsInView).mockReturnValue(true);
  vi.mocked(osNotify).mockClear();
  vi.mocked(osNotify).mockResolvedValue(true);
});
afterEach(() => {
  localStorage.clear();
  resetForTests();
});

const listed = (org = "acme") =>
  (JSON.parse(localStorage.getItem(`tcm-v2-notifications:${org}`) ?? "[]") as AppNotification[]).map((n) => n.id);

const wiMention: Mention = {
  source: "work-item",
  item_id: 41,
  item_type: "Product Backlog Item",
  item_title: "Leave requests",
  comment_id: 7,
  author: "Sam",
  excerpt: "@Avin can you check this?",
  created_date: "2026-09-25T08:00:00Z",
};

function thread(id: number, comments: Array<{ id: number; author: string; author_id: string; content: string }>): PrThread {
  return {
    id,
    status: "active",
    file_path: "",
    line: 0,
    last_updated: "2026-09-25T08:00:00Z",
    comments: comments.map((c) => ({ ...c, avatar: "", published: "2026-09-25T08:00:00Z", edited: false })),
  };
}

test("mention ids follow the spec's two shapes", () => {
  expect(workItemMentionId({ item_id: 41, comment_id: 7 })).toBe("mention:wi:41:7");
  expect(prMentionId({ repo: "web", prId: 12, threadId: 3, commentId: 9 })).toBe("mention:pr:web:12:3:9");
});

test("a work-item mention reads who, where, and opens the item", () => {
  expect(workItemNotification("acme", "Web", wiMention)).toEqual({
    id: "mention:wi:41:7",
    kind: "mention",
    title: "Sam mentioned you on Product Backlog Item #41",
    body: "@Avin can you check this?",
    href: "https://dev.azure.com/acme/Web/_workitems/edit/41",
    target: { kind: "work-item", id: 41, project: "Web" },
  });
});

test("a PR mention reads who, which PR, and opens the PR", () => {
  const n = prNotification("acme", "Web", {
    repo: "web", prId: 12, threadId: 3, commentId: 9, author: "Sam", excerpt: "@you please look", createdDate: "",
  });
  expect(n).toEqual({
    id: "mention:pr:web:12:3:9",
    kind: "mention",
    title: "Sam mentioned you on PR #12",
    body: "@you please look",
    href: "https://dev.azure.com/acme/Web/_git/web/pullrequest/12",
    target: { kind: "pr", repo: "web", id: 12, project: "Web" },
  });
});

test("the PR scan finds @<your id> in any case and skips your own comments", () => {
  const threads = [
    thread(3, [
      { id: 9, author: "Sam", author_id: "u-sam", content: "@<ME-GUID>   can you\nlook at @<KIM-GUID>?" },
      { id: 10, author: "Avin", author_id: "ME-GUID", content: "@<me-guid> note to self" },
      { id: 11, author: "Sam", author_id: "u-sam", content: "@<kim-guid> over to you" },
    ]),
  ];
  expect(prMentions({ repo: "web", id: 12 }, threads, "me-guid")).toEqual([
    {
      repo: "web", prId: 12, threadId: 3, commentId: 9, author: "Sam",
      excerpt: "@you can you look at @someone?", createdDate: "2026-09-25T08:00:00Z",
    },
  ]);
});

/// Review focus 3, webview side.
test("an empty identity id finds nothing", () => {
  const threads = [thread(3, [{ id: 9, author: "Sam", author_id: "u-sam", content: "@<> hi" }])];
  expect(prMentions({ repo: "web", id: 12 }, threads, "")).toEqual([]);
});

test("an excerpt collapses whitespace and stops at 140 characters", () => {
  expect(excerpt("  a \n\n b  ")).toBe("a b");
  const cut = excerpt("x".repeat(200));
  expect(cut).toHaveLength(140);
  expect(cut.endsWith("…")).toBe(true);
});

const NOW = Date.parse("2026-09-25T12:00:00Z");
const found = (id: string, created: string): FoundMention => ({
  notification: { id, kind: "mention", title: id, body: "" },
  created,
});

test("the first check ever raises only the last 24 hours and records older ones as seen", () => {
  const added = noteMentions(
    "acme",
    [found("mention:wi:1:1", "2026-09-25T10:00:00Z"), found("mention:wi:2:1", "2026-09-22T10:00:00Z")],
    NOW,
  );
  expect(added.map((n) => n.id)).toEqual(["mention:wi:1:1"]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBe(String(NOW));
  // Seen, not shown: it stays out on every later check too.
  expect(noteMentions("acme", [found("mention:wi:2:1", "2026-09-22T10:00:00Z")], NOW + 3_600_000)).toEqual([]);
  expect(listed()).toEqual(["mention:wi:1:1"]);
});

test("after the first check, a new mention raises once", () => {
  noteMentions("acme", [], NOW);
  const later = NOW + 5 * 24 * 3_600_000;
  const fresh = found("mention:pr:web:12:3:9", new Date(later - 3_600_000).toISOString());
  expect(noteMentions("acme", [fresh], later).map((n) => n.id)).toEqual(["mention:pr:web:12:3:9"]);
  expect(noteMentions("acme", [fresh], later + 300_000)).toEqual([]);
});

test("each organisation has its own first check", () => {
  noteMentions("acme", [], NOW - 10 * 24 * 3_600_000);
  const added = noteMentions("globex", [found("mention:wi:5:1", "2026-09-22T10:00:00Z")], NOW);
  expect(added).toEqual([]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:globex")).toBe(String(NOW));
});

/// Review focus 5: a malformed date cannot defeat the flood guard.
test("a mention with no readable date counts as old", () => {
  expect(noteMentions("acme", [found("mention:wi:3:1", "")], NOW)).toEqual([]);
  expect(noteMentions("acme", [found("mention:wi:3:1", "")], NOW + 60_000)).toEqual([]);
  expect(listed()).toEqual([]);
});

const added = (n: number): AppNotification[] =>
  Array.from({ length: n }, (_, i) => ({
    id: `mention:wi:${i}:1`, kind: "mention" as const, title: `Sam mentioned you on Task #${i}`,
    body: `excerpt ${i}`, at: "", read: false,
  }));

test("one new mention in view is a toast with its own words", () => {
  announceMentions(added(1));
  expect(toast.info).toHaveBeenCalledWith("Sam mentioned you on Task #0", { description: "excerpt 0", duration: 10_000 });
});

/// Review focus 4.
test("several new mentions make one toast, not one each", () => {
  announceMentions(added(5));
  expect(toast.info).toHaveBeenCalledTimes(1);
  expect(toast.info).toHaveBeenCalledWith("5 new mentions", {
    description: "Sam mentioned you on Task #0\nSam mentioned you on Task #1\nSam mentioned you on Task #2\n…and 2 more",
    duration: 10_000,
  });
});

test("out of view it is an OS notification, and a toast only when that is refused", async () => {
  vi.mocked(appIsInView).mockReturnValue(false);
  announceMentions(added(1));
  expect(osNotify).toHaveBeenCalledWith("Sam mentioned you on Task #0", "excerpt 0");
  await Promise.resolve();
  expect(toast.info).not.toHaveBeenCalled();

  vi.mocked(osNotify).mockResolvedValue(false);
  announceMentions(added(1));
  await vi.waitFor(() => expect(toast.info).toHaveBeenCalledTimes(1));
});

test("nothing new, nothing announced", () => {
  announceMentions([]);
  expect(toast.info).not.toHaveBeenCalled();
  expect(osNotify).not.toHaveBeenCalled();
});
```

Append to `src/lib/notifications.test.ts` (add `markSeen` to its import list from `./notifications`):

```ts
test("markSeen records ids without listing them, and raise skips them after", () => {
  markSeen(ORG, ["mention:wi:1:1", "mention:wi:1:1"]);
  expect(read()).toEqual([]);
  expect(raise(ORG, [{ id: "mention:wi:1:1", kind: "mention", title: "M", body: "" }])).toEqual([]);
  expect(raise(ORG, [{ id: "mention:wi:2:1", kind: "mention", title: "N", body: "" }]).map((n) => n.id)).toEqual([
    "mention:wi:2:1",
  ]);
});
```

Append to `src/components/NotificationBell.test.tsx`:

```tsx
test("a mention wears the Mention label and opens its work item in the app", () => {
  raise("acme", [
    {
      id: "mention:wi:41:7",
      kind: "mention",
      title: "Sam mentioned you on Product Backlog Item #41",
      body: "@Avin can you check this?",
      href: "https://x/41",
      target: { kind: "work-item", id: 41, project: "Web" },
    },
  ]);
  const opened: unknown[] = [];
  render(<NotificationBell org="acme" onOpen={(t) => opened.push(t)} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  expect(screen.getByText("Mention")).toHaveClass("text-danger");
  expect(screen.getByText("@Avin can you check this?")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Sam mentioned you on Product Backlog Item #41" }));
  expect(opened).toEqual([{ kind: "work-item", id: 41, project: "Web" }]);
});

test("a PR mention opens its pull request in the app", () => {
  raise("acme", [
    {
      id: "mention:pr:web:12:3:9",
      kind: "mention",
      title: "Sam mentioned you on PR #12",
      body: "@you please look",
      href: "https://x/pr/12",
      target: { kind: "pr", repo: "web", id: 12, project: "Web" },
    },
  ]);
  const opened: unknown[] = [];
  render(<NotificationBell org="acme" onOpen={(t) => opened.push(t)} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "Sam mentioned you on PR #12" }));
  expect(opened).toEqual([{ kind: "pr", repo: "web", id: 12, project: "Web" }]);
});
```

- [ ] **Step 2: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/lib/mentions.test.ts src/lib/notifications.test.ts src/components/NotificationBell.test.tsx`. Expected: `./mentions` does not resolve, `markSeen` is not exported, and `Mention` is not in the bell.

- [ ] **Step 3: `notifications.ts`.** Replace:

```ts
export type NotificationKind = "assigned" | "pr-conflict" | "pr-review" | "pr-comments";
```

with:

```ts
export type NotificationKind = "assigned" | "pr-conflict" | "pr-review" | "pr-comments" | "mention";
```

Directly after the closing `}` of `raise`, add:

```ts

/** Record ids as seen without listing them - a source's backlog on its
 * first run. raise() skips them from then on, exactly like a dismissed
 * notification. */
export function markSeen(org: string, ids: string[]): void {
  if (!org || ids.length === 0) return;
  const seen = new Set([...known(org), ...load(org).map((n) => n.id)]);
  const unseen = [...new Set(ids)].filter((id) => !seen.has(id));
  if (unseen.length > 0) remember(org, unseen);
}
```

- [ ] **Step 4: Create `src/lib/mentions.ts`:**

```ts
// Mentions of you, turned into bell notifications. Two sources: work-item
// discussions (the recent_mentions command) and the threads of the PRs you
// are on, which the PR badge already reads - so the PR half makes no
// request of its own.
//
// The first check ever for an organisation must not flood the bell with a
// month of old mentions: anything created more than a day before that
// first check is recorded as seen and never shown.

import type { Mention, PrThread } from "../bindings";
import { appIsInView, osNotify } from "./assignedAlerts";
import { markSeen, raise, type AppNotification } from "./notifications";
import { toast } from "./toast";

export type NewNotification = Omit<AppNotification, "at" | "read">;

/** A comment in a PR thread that mentions you. */
export type PrMention = {
  repo: string;
  prId: number;
  threadId: number;
  commentId: number;
  author: string;
  excerpt: string;
  createdDate: string;
};

/** A notification-to-be and when its comment was written. */
export type FoundMention = { notification: NewNotification; created: string };

export const EXCERPT_CHARS = 140;
export const FIRST_RUN_WINDOW_MS = 24 * 60 * 60_000;

const baselineKey = (org: string) => `tcm-v2-mentions-baseline:${org}`;
const enc = encodeURIComponent;

/** Whitespace collapsed, at most 140 characters, the last an ellipsis when cut. */
export function excerpt(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length <= EXCERPT_CHARS ? flat : `${flat.slice(0, EXCERPT_CHARS - 1).trimEnd()}…`;
}

export function workItemMentionId(m: { item_id: number; comment_id: number }): string {
  return `mention:wi:${m.item_id}:${m.comment_id}`;
}

export function prMentionId(m: { repo: string; prId: number; threadId: number; commentId: number }): string {
  return `mention:pr:${m.repo}:${m.prId}:${m.threadId}:${m.commentId}`;
}

export function workItemNotification(org: string, project: string, m: Mention): NewNotification {
  return {
    id: workItemMentionId(m),
    kind: "mention",
    title: `${m.author || "Someone"} mentioned you on ${m.item_type} #${m.item_id}`,
    body: m.excerpt,
    href: `https://dev.azure.com/${enc(org)}/${enc(project)}/_workitems/edit/${m.item_id}`,
    target: { kind: "work-item", id: m.item_id, project },
  };
}

export function prNotification(org: string, project: string, m: PrMention): NewNotification {
  return {
    id: prMentionId(m),
    kind: "mention",
    title: `${m.author || "Someone"} mentioned you on PR #${m.prId}`,
    body: m.excerpt,
    href: `https://dev.azure.com/${enc(org)}/${enc(project)}/_git/${enc(m.repo)}/pullrequest/${m.prId}`,
    target: { kind: "pr", repo: m.repo, id: m.prId, project },
  };
}

/** PR comments name people as `@<identity id>`. In an excerpt that reads
 * as "@you" for you and "@someone" for anyone else. */
function readable(content: string, me: string): string {
  return content.replace(/@<([^<>\s]+)>/g, (_, id: string) => (id.toLowerCase() === me ? "@you" : "@someone"));
}

/** The comments in a PR's threads that mention `me` (`@<id>`, any case)
 * and were not written by `me`. An empty id matches nothing. */
export function prMentions(pr: { repo: string; id: number }, threads: PrThread[], me: string): PrMention[] {
  const id = me.trim().toLowerCase();
  if (!id) return [];
  const token = `@<${id}>`;
  const out: PrMention[] = [];
  for (const t of threads) {
    for (const c of t.comments) {
      if ((c.author_id ?? "").toLowerCase() === id) continue;
      if (!c.content.toLowerCase().includes(token)) continue;
      out.push({
        repo: pr.repo,
        prId: pr.id,
        threadId: t.id,
        commentId: c.id,
        author: c.author,
        excerpt: excerpt(readable(c.content, id)),
        createdDate: c.published,
      });
    }
  }
  return out;
}

/** When this organisation was first checked, in ms. The first call sets it. */
function baseline(org: string, now: number): number {
  try {
    const at = Number(localStorage.getItem(baselineKey(org)) ?? "");
    if (localStorage.getItem(baselineKey(org)) !== null && Number.isFinite(at)) return at;
    localStorage.setItem(baselineKey(org), String(now));
  } catch {
    // session-only: every check then counts as the first
  }
  return now;
}

/** Raise what is new; return what was raised. A mention written more than
 * 24 hours before this organisation's first check, or with no readable
 * date, is recorded as seen and not shown. */
export function noteMentions(org: string, found: FoundMention[], now = Date.now()): AppNotification[] {
  if (!org) return [];
  const cutoff = baseline(org, now) - FIRST_RUN_WINDOW_MS;
  const old: string[] = [];
  const fresh: NewNotification[] = [];
  for (const { notification, created } of found) {
    const t = Date.parse(created);
    if (Number.isNaN(t) || t < cutoff) old.push(notification.id);
    else fresh.push(notification);
  }
  markSeen(org, old);
  return raise(org, fresh);
}

/** The moment, as a new assignment has it: a toast when the app is in
 * view, an OS notification when it is not (a toast if that is refused).
 * One per check, however many arrived. */
export function announceMentions(added: AppNotification[]): void {
  if (added.length === 0) return;
  const title = added.length === 1 ? added[0].title : `${added.length} new mentions`;
  const shown = added.slice(0, 3).map((n) => n.title);
  const rest = added.length - shown.length;
  const body = added.length === 1 ? added[0].body : rest > 0 ? `${shown.join("\n")}\n…and ${rest} more` : shown.join("\n");
  if (appIsInView()) {
    toast.info(title, { description: body, duration: 10_000 });
    return;
  }
  osNotify(title, body)
    .then((sent) => {
      if (!sent) toast.info(title, { description: body, duration: 10_000 });
    })
    .catch(() => {});
}
```

- [ ] **Step 5: The bell.** In `src/components/NotificationBell.tsx`, replace the two maps (l.25-37) with:

```ts
const KIND_LABEL: Record<AppNotification["kind"], string> = {
  assigned: "Assigned",
  "pr-conflict": "Conflicts",
  "pr-review": "Review",
  "pr-comments": "Comments",
  mention: "Mention",
};

// Mention wears the one status token no other kind uses, so it reads as
// its own thing at a glance.
const KIND_CLASS: Record<AppNotification["kind"], string> = {
  assigned: "bg-accent/15 text-accent",
  "pr-conflict": "bg-warning/15 text-warning",
  "pr-review": "bg-success/15 text-success",
  "pr-comments": "bg-warning/15 text-warning",
  mention: "bg-danger/15 text-danger",
};
```

- [ ] **Step 6: Run** `npx vitest run --exclude "**/.claude/**" src/lib/mentions.test.ts src/lib/notifications.test.ts src/components/NotificationBell.test.tsx src/lib/assignedAlerts.test.ts src/ui-consistency.test.ts src/lib/cache.test.ts`, then `npx tsc --noEmit`. All green.

- [ ] **Step 7: Commit**

```bash
git add src/lib/notifications.ts src/lib/notifications.test.ts src/lib/mentions.ts src/lib/mentions.test.ts src/components/NotificationBell.tsx src/components/NotificationBell.test.tsx
git commit -q -F - <<'EOF'
feat(v2): mentions become bell notifications

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 6: Checking for mentions (hooks and mounting)

**Files:**
- Create: `src/hooks/useMentions.ts`
- Modify: `src/hooks/usePrAttention.ts`
- Modify: `src/components/ContextBar.tsx:7` (import) and `:93` (mount)
- Test: `src/hooks/useMentions.test.tsx` (create), `src/hooks/usePrAttention.test.tsx` (append), `src/components/ContextBar.test.tsx` (create)

**Interfaces:**
- Consumes (Task 5): `announceMentions`, `noteMentions`, `workItemNotification`, `prMentions`, `prNotification`. Consumes (Task 4): `commands.recentMentions`, `commands.connectedUser`. Existing: `logUi` from `../lib/uiLog`, `unwrap` from `../lib/ipc`.
- Produces: `export const MENTIONS_POLL_MS = 5 * 60_000` and `export function useMentions(org: string, project: string): void` in `src/hooks/useMentions.ts`. `usePrAttention` keeps its signature and return value.

- [ ] **Step 1: Write the failing tests.** Create `src/hooks/useMentions.test.tsx`:

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { resetForTests } from "../lib/notifications";
import { toast } from "../lib/toast";
import { useMentions } from "./useMentions";

vi.mock("../lib/toast", () => ({ toast: { info: vi.fn() } }));
vi.mock("../lib/assignedAlerts", () => ({
  appIsInView: () => true,
  osNotify: () => Promise.resolve(true),
}));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
  vi.mocked(toast.info).mockClear();
});
afterEach(() => {
  clearMocks();
  localStorage.clear();
  resetForTests();
});

function Probe() {
  useMentions("acme", "Web");
  return null;
}

function mount() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe />
    </QueryClientProvider>,
  );
}

const stored = () =>
  JSON.parse(localStorage.getItem("tcm-v2-notifications:acme") ?? "[]") as Array<{ id: string; title: string; kind: string }>;

test("a work-item mention reaches the bell and a toast, asked for this org and project", async () => {
  const asked: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "recent_mentions") {
      asked.push(args);
      return [
        {
          source: "work-item", item_id: 41, item_type: "Product Backlog Item", item_title: "Leave requests",
          comment_id: 7, author: "Sam", excerpt: "@Avin can you check this?",
          created_date: new Date(Date.now() - 3_600_000).toISOString(),
        },
      ];
    }
  });
  mount();
  await waitFor(() => expect(stored().map((n) => n.id)).toEqual(["mention:wi:41:7"]));
  expect(asked).toEqual([{ organization: "acme", project: "Web" }]);
  expect(stored()[0]).toMatchObject({ kind: "mention", title: "Sam mentioned you on Product Backlog Item #41" });
  expect(toast.info).toHaveBeenCalledWith("Sam mentioned you on Product Backlog Item #41", {
    description: "@Avin can you check this?",
    duration: 10_000,
  });
});

test("a failed check is logged, raises nothing and shows no toast", async () => {
  const logged: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "recent_mentions") throw { kind: "Network", detail: "Can't reach Azure DevOps." };
    if (cmd === "log_ui") logged.push((args as { message: string }).message);
  });
  mount();
  await waitFor(() => expect(logged.some((m) => m.startsWith("mentions: work-item check failed"))).toBe(true));
  expect(stored()).toEqual([]);
  expect(toast.info).not.toHaveBeenCalled();
});
```

In `src/hooks/usePrAttention.test.tsx`, change the vitest import to `import { afterEach, beforeEach, expect, test, vi } from "vitest";`. Add these imports and mocks below the existing imports:

```tsx
import { resetForTests } from "../lib/notifications";

vi.mock("../lib/toast", () => ({ toast: { info: vi.fn() } }));
vi.mock("../lib/assignedAlerts", () => ({
  appIsInView: () => true,
  osNotify: () => Promise.resolve(true),
}));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
});
```

and append:

```tsx
test("a PR comment that mentions you raises a Mention; yours and others' do not", async () => {
  const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
  const c = (id: number, author: string, author_id: string, content: string) => ({
    id, author, author_id, avatar: "", content, published: hourAgo, edited: false,
  });
  mockIPC((cmd) => {
    if (cmd === "connected_user") return { id: "me-guid", display_name: "Avin" };
    if (cmd === "pr_overview") return { mine: [pr(1)], awaiting: [] };
    if (cmd === "pr_threads")
      return [
        {
          id: 30, status: "active", file_path: "", line: 0, last_updated: hourAgo,
          comments: [
            c(5, "Sam", "sam-guid", "@<ME-GUID> can you look?"),
            c(6, "Avin", "ME-GUID", "@<me-guid> note to self"),
            c(7, "Sam", "sam-guid", "@<kim-guid> over to you"),
          ],
        },
      ];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const ids = () =>
    (JSON.parse(localStorage.getItem("tcm-v2-notifications:acme") ?? "[]") as Array<{ id: string }>).map((n) => n.id);
  await waitFor(() => expect(ids()).toContain("mention:pr:web:1:30:5"));
  const list = JSON.parse(localStorage.getItem("tcm-v2-notifications:acme")!) as Array<Record<string, unknown>>;
  expect(list.find((n) => n.id === "mention:pr:web:1:30:5")).toMatchObject({
    kind: "mention",
    title: "Sam mentioned you on PR #1",
    body: "@you can you look?",
    target: { kind: "pr", repo: "web", id: 1, project: "Web" },
  });
  expect(ids()).not.toContain("mention:pr:web:1:30:6");
  expect(ids()).not.toContain("mention:pr:web:1:30:7");
});
```

Create `src/components/ContextBar.test.tsx`:

```tsx
// The bar is where the app-wide background checks live: the PR badge and,
// beside it, the mentions check.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ContextBar from "./ContextBar";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

test("the context bar checks for mentions in the picked project", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ContextBar
        org="acme"
        setOrg={() => {}}
        project="Web"
        setProject={() => {}}
        pbi={null}
        setPbi={() => {}}
        account={null}
        workMode={false}
        onToggleWork={() => {}}
        onOpenSettings={() => {}}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "recent_mentions")?.args).toEqual({ organization: "acme", project: "Web" }),
  );
  expect(calls.some((c) => c.cmd === "pr_overview")).toBe(true);
});
```

- [ ] **Step 2: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/hooks/useMentions.test.tsx src/hooks/usePrAttention.test.tsx src/components/ContextBar.test.tsx`. Expected: `./useMentions` does not resolve, no PR mention is raised, and `recent_mentions` is never called.

- [ ] **Step 3: Create `src/hooks/useMentions.ts`:**

```ts
/**
 * Work-item @mentions of you, checked when the app starts and every five
 * minutes after, into the bell (and a toast or OS notification, like a new
 * assignment). Mounted beside usePrAttention, which does the same for
 * mentions in PR threads. A failed check is logged and simply tried again
 * at the next one - no toast.
 */
import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { commands } from "../bindings";
import { unwrap } from "../lib/ipc";
import { announceMentions, noteMentions, workItemNotification } from "../lib/mentions";
import { logUi } from "../lib/uiLog";

export const MENTIONS_POLL_MS = 5 * 60_000;

export function useMentions(org: string, project: string): void {
  const mentions = useQuery({
    queryKey: ["recent-mentions", org, project],
    queryFn: async () => (await unwrap(commands.recentMentions(org, project))) ?? [],
    enabled: Boolean(org && project),
    refetchInterval: MENTIONS_POLL_MS,
    // A minimised app still checks: that is when the OS notification is
    // the only way the mention is seen.
    refetchIntervalInBackground: true,
    retry: false,
  });

  useEffect(() => {
    if (!mentions.data) return;
    announceMentions(
      noteMentions(
        org,
        mentions.data.map((m) => ({
          notification: workItemNotification(org, project, m),
          created: m.created_date,
        })),
      ),
    );
  }, [mentions.data, org, project]);

  useEffect(() => {
    if (mentions.error) {
      logUi(`mentions: work-item check failed, trying again at the next check: ${mentions.error.message}`);
    }
  }, [mentions.error, mentions.errorUpdatedAt]);
}
```

- [ ] **Step 4: The PR scan in `usePrAttention.ts`.** Replace:

```ts
import { notePrComments, notePrOverview } from "../lib/notifications";
```

with:

```ts
import { announceMentions, noteMentions, prMentions, prNotification } from "../lib/mentions";
import { notePrComments, notePrOverview } from "../lib/notifications";
```

and replace:

```ts
  return prs.filter((pr, i) => pr.has_conflicts || unresolvedFor(i) > 0).length;
```

with:

```ts
  // Who "you" are, for the mention scan: the same in-memory, once-per-org
  // lookup the comments panel uses.
  const me = useQuery({
    queryKey: ["connected-user", org],
    queryFn: async () => (await unwrap(commands.connectedUser(org))) ?? null,
    enabled: Boolean(org),
    staleTime: Infinity,
    retry: false,
  });
  const myId = me.data?.id ?? "";

  // Mentions of you in these same threads, on every thread refresh - no
  // request of their own. The store dedupes, so a rescan raises only
  // what is new.
  const threadStamp = threads.map((t) => t.dataUpdatedAt).join("|");
  useEffect(() => {
    if (!myId) return;
    const found = prs.flatMap((pr, i) =>
      prMentions(pr, threads[i]?.data ?? [], myId).map((m) => ({
        notification: prNotification(org, project, m),
        created: m.createdDate,
      })),
    );
    announceMentions(noteMentions(org, found));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadStamp, myId, org, project]);

  return prs.filter((pr, i) => pr.has_conflicts || unresolvedFor(i) > 0).length;
```

- [ ] **Step 5: Mount it.** In `src/components/ContextBar.tsx`, replace `import { usePrAttention } from "../hooks/usePrAttention";` with:

```ts
import { useMentions } from "../hooks/useMentions";
import { usePrAttention } from "../hooks/usePrAttention";
```

and replace:

```ts
  const prAttention = usePrAttention(org, project);
```

with:

```ts
  const prAttention = usePrAttention(org, project);
  // Work-item @mentions of you, into the bell beside it.
  useMentions(org, project);
```

- [ ] **Step 6: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/hooks/useMentions.test.tsx src/hooks/usePrAttention.test.tsx src/components/ContextBar.test.tsx src/components/CommentsPanel.test.tsx src/screens/PrPanel.test.tsx src/lib/cache.test.ts`, then `npx vitest run --exclude "**/.claude/**" src/App.test.tsx src/a11y.test.tsx` (App.test is slow and has one documented load flake: one failure that passes on a re-run is that; two different ones are not), then `npx tsc --noEmit`. All green.

- [ ] **Step 7: Commit**

```bash
git add src/hooks/useMentions.ts src/hooks/useMentions.test.tsx src/hooks/usePrAttention.ts src/hooks/usePrAttention.test.tsx src/components/ContextBar.tsx src/components/ContextBar.test.tsx
git commit -q -F - <<'EOF'
feat(v2): check for mentions every five minutes and on PR thread refresh

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

## After execution: the gates, then checks only a person can make

- [ ] Rust suite once, from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --tests` (after the dev-app check). Then `npx vitest run --exclude "**/.claude/**"` and `npx tsc --noEmit`, one at a time. No release.

By hand, owed by the owner (spec §3; jsdom does no layout or hit testing, so only a real walk-through checks the lanes):
1. On your real sprint board, turn on Swimlanes. Tasks sit under their PBI, PBIs under their Feature, and No parent comes last. Collapse a lane, restart the app, and it is still collapsed. Hide Done, and it hides in every lane. Dragging a card into another lane's column does nothing.
2. Have someone @mention you on a work item and on a PR you are on. Both reach the bell within 5 minutes, labelled Mention. Each opens its item or PR in the app. With the app minimised, the mention arrives as a Windows notification.
3. On first sign-in to an organisation, only mentions from the last day appear.

---

## Self-review

**1. Spec coverage.**

| Spec | Where |
| --- | --- |
| §1.2 direct parent only; a view option, off by default | Tasks 1 and 2 (`System.Parent`, no tree walk); Task 3 (switch off by default) |
| §1.3 `BOARD_FIELDS` + `System.Parent`; `BoardParent`; distinct parents batch-read in 200s with `errorPolicy=omit`; unreadable parent is an empty title; the read never fails the board; bindings | Task 1 |
| §1.4 switch remembered in `tcm-v2-board-swimlanes`; lane header (type, `#id`, title, count); title opens the drawer; order by most recent card; No parent last; filters first and empty lanes hidden; collapse per lane, remembered at `tcm-v2-board-lanes-collapsed:<org>/<project>` with `0`; Collapse all / Expand all; collapsed columns shared; drops only in own lane, parent never changed; a parent card in its own parent's lane; tokens, `Switch`, `actionIcons`, accessible name | Task 2 (grouping, names, storage), Task 3 (view) |
| §1.5 not in scope | Nothing added: no cross-level grouping, no parent change by drag, no priority order |
| §2.2 work-item discussions and PRs you are on; a new kind raised like the others | Tasks 4 to 6 |
| §2.3 identity from `connectionData` via `connected_user`, once per org per session, memory only | Task 4 (`connected_user_cached`, session tier); Task 6 (the shared `["connected-user", org]` query) |
| §2.4 `recent_mentions`; WIQL, top 20; comments `order=desc&$top=50`; anchor match ignoring case; skip your own; `Mention` fields; excerpt 140; per-item failure skips; WIQL failure fails | Task 4 |
| §2.5 PR threads scanned for `@<id>`, skipping yours, no new request; `Mention` with repo, PR, thread, comment, author, excerpt, date | Task 4 (`author_id`), Task 5 (`prMentions`), Task 6 (scan in `usePrAttention`) |
| §2.6 kind, label and colour; ids; title, body, target, href; every 5 minutes and at start, hook beside `usePrAttention`; PR on thread refresh; 24-hour first run per org in localStorage; toast or OS notification; failures logged, no toast | Tasks 5 and 6 |
| §2.7 not in scope | Nothing added: no comment scrolling, no field mentions, no other PRs, no group mentions |
| §3 Rust and frontend tests; the manual live check | Tasks 1 to 6; "After execution" |

**2. Placeholder scan.** No TBD, "similar to" or "add error handling". Every code step carries its code. The one conditional instruction (Task 3, a pre-existing axe violation) says exactly what to do: stop and report it.

**3. Type consistency.** These names are the same everywhere they appear: `BoardParent`, `BoardItem.parent`, `TITLE_FIELDS`, `read_titles`, `NO_PARENT`, `Lane`, `laneIdOf`, `groupIntoLanes`, `laneLabel`, `laneToggleName`, `cardCount`, `loadSwimlanes`/`saveSwimlanes`, `loadCollapsedLanes`/`saveCollapsedLanes`, `IconExpandAll`, `Mention`, `mentions_me`, `excerpt`, `mentions_in`, `connected_user_cached`, `recent_mentions`/`recentMentions`, `PrComment.author_id`, `keys::connected_user`, `CONNECTED_USER_TTL`, `markSeen`, `NewNotification`, `PrMention`, `FoundMention` (`{ notification, created }`), `workItemMentionId`, `prMentionId`, `workItemNotification`, `prNotification`, `prMentions`, `noteMentions`, `announceMentions`, `useMentions`, `MENTIONS_POLL_MS`. The TS command call is `commands.recentMentions(org, project)`, matching the Rust argument order `(organization, project)`.

**4. Review Focus.** Five lines, each with its test in the owning task (Tasks 1, 2, 4 and 5).
