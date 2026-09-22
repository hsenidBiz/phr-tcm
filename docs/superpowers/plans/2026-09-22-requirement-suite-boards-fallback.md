# Requirement Suite via the Boards Route Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the documented suite-create API answers 403 for the account's access level, the upload falls back, once, to the internal route the Boards "Add Test" button uses, so the uploaded cases still land in a requirement suite; every request and reply through that route is logged whole; a development-build probe lets the route be tried once against a real PBI before anyone relies on it.

**Design:** `docs/superpowers/specs/2026-09-22-requirement-suite-boards-fallback-design.md` (read it first; its sections are referenced below). The one unknown it leaves, the body's field names (§4.1), is carried as ONE constant marked unconfirmed, and the probe (§4.5) is how it gets confirmed: a 400 from the route names the wrong field.

**Architecture:** A new `src-tauri/src/ado_testplan/boards.rs` holds everything pure enough to test with wiremock: project-id and team-id resolution (cached), the route call, and `boards_fallback(...)` which strings them together and ends by finding the suite in the plan the reply names. `submit_queue` (`commands/queue.rs`) keeps its 403 sentence aside instead of emitting `SuiteNotCreated`, runs the batch as today, then calls `boards_fallback` with the ids this upload created or updated, and emits either what a created suite emits today or `SuiteNotCreated` with the original sentence plus one line. A dev-only command `dev_probe_boards_suite` calls the same function and returns a text report; the dev panel gets a button for it.

**Tech Stack:** Rust (Tauri 2, reqwest through the existing `post_json`/`get_json`, wiremock in tests), the app's Rust cache (`cache::session_fresh`/`session_put`), React (dev panel only).

## Global Constraints

- Every Rust test is an integration test under `src-tauri/tests/`. One build or test command at a time; Rust from `src-tauri/` with `CARGO_TARGET_DIR=target/gate`. No new crates.
- GET, POST and PATCH only; no DELETE anywhere. The route is reached through `post_json`, so the pacing and `refused()` logging funnel apply unchanged.
- The Boards route is a FALLBACK: it runs only after the documented create answered 403 for the access level (the synthesized `AdoError::Http { status: 403, .. }` from `ensure_requirement_suite`), never first, never retried, never in a loop, and only when this upload created or updated at least one case.
- Logging contract (§4.7): the tidied URL, the body and the reply's first 2000 chars at INFO, under the prefix `boards suite route`. No token, no cookie, no header value ever in a log line.
- User-facing sentences name no URL. The message path (§2.4's sentence) stays exactly as it is when the fallback also fails; one line is appended.
- Caching has one implementation per side: new keys go in `cache/keys.rs`; per-project id and per-area team id use the session tier.
- The body constant is one `const` with a comment naming the design section and the date, marked UNCONFIRMED until the probe answers 200; the test fixture uses the same constant so they cannot drift.
- No em dashes in user-facing text. Commits via Bash heredoc with the model's own trailer.

**Out of scope:** letting Run Tests use the route (§4.4 last paragraph); deleting the two stub test cases in Azure DevOps (a person does that); the changelog line (it belongs to the next version bump the person makes; the sentence is in §4.7).

---

### Task 1: The route, the ids it needs, and the fallback as a pure function

**Files:**
- Create: `src-tauri/src/ado_testplan/boards.rs`; add `pub mod boards;` in `src-tauri/src/ado_testplan/mod.rs`
- Modify: `src-tauri/src/cache/keys.rs` (two keys, two TTLs)
- Test: `src-tauri/tests/ado_boards.rs` (create)

**Interfaces (in `v2_lib::ado_testplan::boards`, all `impl AdoClient` methods unless noted):**

```rust
/// The Boards "Add Test" body as far as it is known (design §4.1): the
/// controller's sibling call sends `{"userStoryIds":"[145386]"}`, JSON whose
/// array values are JSON strings. Field names UNCONFIRMED until the probe
/// answers 200; a 400 names the wrong one. Seen 2026-09-22, `__v=5`.
pub const BOARDS_ROUTE_VERSION: &str = "5";
pub fn boards_body(pbi_id: i32, case_ids: &[i32]) -> serde_json::Value;
// = json!({ "requirementId": pbi_id, "testCaseIds": format!("[{}]", ids joined by ",") })

pub async fn project_id(&self, org: &str, project: &str) -> Result<String, AdoError>;
// GET {base}/{org}/_apis/projects/{project}?api-version=7.1 -> "id"; session cache key `keys::project_id(base, org, project)`, TTL 6 h

pub async fn team_for_area(&self, org: &str, project: &str, project_id: &str, area_path: &str) -> Result<String, AdoError>;
// list_teams + get_team_scope per team (reuse work_board::board's methods; they are on AdoClient); pick the team whose
// area value covers the PBI's area (equal, or a prefix with includeChildren; compare case-insensitively with `/` and `\` unified),
// longest match wins; none -> the project's defaultTeam id (read from the projects call: `defaultTeam.id`);
// session cache key `keys::area_team(base, org, project, area)`, TTL 6 h

pub async fn boards_add_to_requirement_suite(&self, org: &str, project_id: &str, team_id: &str, pbi_id: i32, case_ids: &[i32]) -> Result<i32, AdoError>;
// exactly design §4.3: URL {base}/{org}/{project_id}/_api/_testManagement/AddWitTestCasesToRequirementSuite?teamId={team_id}&__v=5,
// post_json(boards_body), INFO log before and after, returns testPlanId or Http{status:0, body:"the Boards route answered without a testPlanId: <raw>"}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardsOutcome { pub suite: EnsuredSuite, pub project_id: String, pub team_id: String }

pub async fn boards_fallback(&self, org: &str, project: &str, pbi_id: i32, area_path: &str, case_ids: &[i32]) -> Result<BoardsOutcome, AdoError>;
// project_id -> team_for_area -> boards_add_to_requirement_suite -> find_requirement_suite(org, project, plan_id, pbi_id)
// (None -> Http{status:0, body:"the Boards route named plan P but that plan has no requirement suite for #N"})
// -> get_test_plan for the name -> EnsuredSuite { plan_id, plan_name, suite_id, created_plan: false }; logs
// "requirement suite for #{pbi} created through the Boards route: plan {p}, suite {s}"; the caller remembers it.
```

`case_ids` empty is refused before any request: `Http { status: 0, body: "the Boards route needs at least one test case id" }`.

- [ ] **Step 1: Write the failing tests** in `src-tauri/tests/ado_boards.rs` (wiremock, `AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())` as `tests/ado_testplan.rs` does; each test its own `MockServer`):
  - `the_body_follows_the_controllers_convention`: `boards_body(145386, &[157941, 157801])` equals `{"requirementId":145386,"testCaseIds":"[157941,157801]"}` (pure).
  - `boards_route_posts_the_body_and_reads_the_plan_id` (§4.6): mock `POST /PeoplesHR/73d6b311-d948-40cc-8c86-fc600c1edb87/_api/_testManagement/AddWitTestCasesToRequirementSuite` with `query_param("teamId", "71215fd9-79d5-4742-8756-01821a02e014")` and `query_param("__v", "5")`, `body_json(boards_body(145386, &[157941]))`, reply 200 `{"requirementId":145386,"testPlanId":157942,"testPoints":[]}`; assert `Ok(157942)` and `.expect(1)` + verify.
  - `boards_route_without_a_plan_id_is_an_error_not_a_guess`: reply `{}` -> `Err(Http{status:0, ..})` whose body contains `testPlanId`.
  - `boards_route_refusals_come_back_as_the_usual_variants`: 403 -> `Forbidden`; 400 with a body naming a field -> `Http{400, body}` containing that body (this is what the probe reads).
  - `the_project_id_is_read_once_and_cached`: mock `GET /PeoplesHR/_apis/projects/HRM` `.expect(1)`; call twice; same id. (Cache is process-wide: use a base_url unique to the test by way of the server's own URI, which the key includes; and call `cache::clear()`-equivalent if the file's other tests need it: read `tests/ado_testplan.rs:705` for the cache pattern.)
  - `the_team_is_the_one_whose_area_covers_the_pbi_longest_match_wins`: two teams, `Alpha` with area `HRM` includeChildren and `Gamma Guardians` with `HRM\Gamma Guardians` includeChildren; area `HRM\Gamma Guardians\Sub` picks Gamma's id; area `HRM\Other` picks Alpha's; `HRM/Gamma Guardians` (forward slash, different case) picks Gamma's.
  - `no_covering_team_means_the_default_team`: teams with areas that do not cover; projects call carries `defaultTeam.id`; that id is returned.
  - `the_fallback_ends_with_the_suite_the_plan_holds` (§4.6's submit test, minus the command): projects + teams + teamfieldvalues + the route (200, `testPlanId: 157942`) + `GET .../testplan/Plans/157942/suites` listing a `requirementTestSuite` with `requirementId 145386`, id 157944 + `GET .../testplan/plans/157942` with a name; assert `BoardsOutcome { suite: EnsuredSuite { plan_id: 157942, plan_name, suite_id: 157944, created_plan: false }, .. }` and every request method is GET or POST.
  - `the_fallback_refuses_an_empty_id_list_before_any_request`: `received_requests()` empty.
  - `the_fallback_says_when_the_named_plan_has_no_suite`: suites list empty -> `Err` mentioning `157942` and `#145386`.

- [ ] **Step 2: Run to verify failure:** `cargo test --test ado_boards` (compile errors).
- [ ] **Step 3: Implement** `boards.rs`, the two keys (`project_id`, `area_team`, both with `SESSION_TTL: Duration = 6 h` in `keys.rs` beside the others), and register the module. Reuse `list_teams`, `get_team_scope`, `default_team`-style reads from `work_board/board.rs` (if `default_team` returns a NAME not an id, read `defaultTeam.id` in `project_id`'s own projects call and cache both as a small struct). `percent_encode_segment` for `org` in the URL; `project_id` is a GUID, used raw.
- [ ] **Step 4: Run:** `cargo test --test ado_boards`, `cargo test --test ado_testplan`, `cargo test --test cache` (if it exists), a warnings check on the test build.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/tests/ado_boards.rs
git commit -q -F - <<'EOF'
feat(v2): the Boards route that creates a requirement suite is callable, logged whole, and ends with the suite it made

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: The upload falls back once, and a development build can probe the route

**Files:**
- Modify: `src-tauri/src/commands/queue.rs` (`submit_queue`), `src-tauri/src/commands/misc.rs` or a new `src-tauri/src/commands/dev.rs` (`dev_probe_boards_suite`), `src-tauri/src/lib.rs` (register)
- Modify: `src/dev/DevPanel.tsx` (one button + result box)
- Test: `src-tauri/tests/ado_boards.rs` (the probe's pure report), `src/dev/DevPanel.test.tsx` if one exists (else none: the panel is dev-only and untested today; say so)
- Generated: `src/bindings.ts`

**`submit_queue` ordering (design §4.4), exactly:**
1. Where the suite is resolved today: on `Err(AdoError::Http { status: 403, body })` from `ensure_requirement_suite`, do NOT emit `SuiteNotCreated`; set `suite_pending = Some(body)`; log `warn` as today. Every other `Err` keeps today's behaviour.
2. The batch runs unchanged.
3. After the loop: if `suite_pending` is `Some(sentence)`, collect `ids: Vec<i32>` from `results` where `id.is_some()` and `action != "failed"`. If `ids` is empty: emit `SuiteNotCreated { reason: sentence }` as before (nothing to add). Otherwise call `client.boards_fallback(&organization, &project, pbi_id, &pbi_area, &ids)`:
   - `Ok(out)`: `remember_suite(...)`, and log the line `boards_fallback` already logs; emit nothing new (a found suite emits nothing today; `PlanCreated` is only for the documented create, and this route makes the team's sprint plan, which the log names).
   - `Err(e)`: log `warn` with `{e}` (the `refused()` funnel already logged the body for a 4xx), then emit `SuiteNotCreated { reason: format!("{sentence} The Boards route did not work either - Settings, Logs has what it said.") }`.
4. Never retry.

**The probe (design §4.5):** `#[tauri::command] #[specta::specta] pub async fn dev_probe_boards_suite(app, organization: String, project: String, pbi_id: i32, case_id: i32) -> Result<String, String>`: refused with `only in a development build` unless `crate::ai_tools::dev_build()`; otherwise `get_fresh_token`, `get_work_item_paths` for the area, `boards_fallback(.., &[case_id])`, and return a plain-text report built by a pure `pub fn probe_report(out: &Result<BoardsOutcome, AdoError>) -> String` in `boards.rs`: on `Ok`, `200: plan P "name", suite S, project id X, team id Y - wire it`; on `Err(Http{400, body})`, `400: the body's field names are wrong - Azure DevOps said: <body first 600 chars>`; on `Forbidden`, `403: the route refused a bearer token - the design stops here (§4.1 assumption)`; other errors `{e}`. Test `probe_report` for the four shapes in `ado_boards.rs`.

**The dev panel:** a small "Boards suite route probe" section with PBI id and case id inputs (org/project from the app's prefs, the way the panel reads them elsewhere), a "Probe" button calling `commands.devProbeBoardsSuite`, the report in a `<pre>`. Tokens and the shared `Button`/`Input`; no em dashes.

- [ ] **Step 1: Write the failing test** `probe_report_reads_the_four_answers` (pure).
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** the three pieces. In `submit_queue`, keep the region's existing comments and add one sentence on why the emit moved after the batch (the route needs created ids).
- [ ] **Step 4: Run:** `cargo test --test ado_boards`, `cargo test --test bindings` (+ the line-ending check), `npx tsc --noEmit`, `npx vitest run src/dev --exclude "**/.claude/**"`, then the full `cargo test --tests`, `npx vitest run --exclude "**/.claude/**"`, `npm run build`, one at a time.
- [ ] **Step 5: Commit**

```bash
git add src-tauri/src src-tauri/tests src/bindings.ts src/dev
git commit -q -F - <<'EOF'
feat(v2): when Azure DevOps refuses to create a suite, the upload takes the Boards route once, and a dev build can probe it

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

## After execution

The route's body is unconfirmed until the probe runs on a PBI with no suite (design §4.1, §4.5). Probe sequence for the person: dev build, Dev panel, PBI id of a card with no suite and an existing Tested-By case id, Probe. `200` means wire is live; `400` names the field to rename in `boards_body` (one constant, one test); `403` means the bearer assumption failed and the fallback stays a message path.
