# Requirement suite via the Boards route — issue, findings, proposed fix

Date: 2026-09-22. Written for the code change to be made on the other machine.
Everything below is from the logs, the code and one watched Boards session; the
one thing NOT observed is marked as such (§4.1) and must be captured first.

## 1. The issue as it surfaced

Uploading 69 cases (`report-1-individual-performance-overview-report.json`) to
PBI #138416 failed with every case saying `http 0`. After moving to PBI #145386
the cases uploaded, but the app could not create the requirement suite: it tried
48 test plans one after another (70 s of 403s) and ended with "no test suite
could be created". Until a suite exists the cases are linked to the PBI but do
not appear in Run Tests.

## 2. What was found (with evidence)

### 2.1 `http 0` on #138416 = the 1000-link limit
The `$batch` POST answered 200 with ONE item instead of 69; the app's own
count-mismatch guard fired, and `AdoError::Http`'s Display dropped the body.
Once the body was kept (1.25.15) it read:

> TF237201: Cannot add a new link because one of the work items being linked
> will exceed the 1000 link limit.

#138416 carries 931 test cases plus its other links. Nothing more fits on it.
New PMS sets go to other PBIs (report cases → #145386 "[PMS] - PMS Reports").

### 2.2 The suite 403 is the ACCESS LEVEL, not the area permission
`POST …/_apis/testplan/Plans/{id}/suites` answered, for every plan:

```
403 {"message":"You are not authorized to access this API. Please contact your
project administrator","typeKey":"UnauthorizedAccessException"}
```

That wording is Azure DevOps' access-level gate. Microsoft's docs
(Manual test access and permissions; Create and manage test suites): creating
test plans and test suites needs **Basic + Test Plans** (or VS Enterprise /
Test Professional). "Manage test suites" on the area path is a second,
separate requirement, and its refusal names the permission instead.

Ruled out: the request (byte-for-byte what v1 sent, `api-version=7.1`, same
body), the token scopes (`499b84ac-…/.default` in both apps), the plan choice
(the first plan tried was the sprint plan Boards itself uses). Across all logs
this account has never had a suite create succeed; #138416's suite 144624 was
FOUND, not made — Boards' "Add test" made it.

### 2.3 What Boards does — watched in the browser
On #145386's card → "Add Test" opens a full NEW Test Case form (project rules:
Steps and Module required; Module for PMS = `Performance`). Saving it created
test case #157941, and then the portal called:

```
POST https://dev.azure.com/PeoplesHR/{projectId}/_api/_testManagement/AddWitTestCasesToRequirementSuite?teamId={teamId}&__v=5
     projectId = 73d6b311-d948-40cc-8c86-fc600c1edb87   (HRM)
     teamId    = 71215fd9-79d5-4742-8756-01821a02e014   (Gamma Guardians)
→ 200
{"requirementId":145386,"testPlanId":157942,
 "testPoints":[{"outcome":"Active","sequenceNumber":0,"testCaseId":157801,
                "testCaseTitle":"…","testPointId":251509}, … 136 entries …]}
```

Effects, confirmed through the app's bridge afterwards:
- a NEW test plan **157942** "Gamma Guardians_Stories_26R2_SP04_Gamma_Guardians_Aug_31_September_11_26"
  (the team's current sprint), not the existing sprint plan 144622;
- requirement suite **157944** "145386 : [RM_Gamma_Guardians_26R1_90]-[PMS] - PMS Reports";
- EVERY Tested-By-linked case on the PBI got a test point — all 69 + 66 already
  uploaded, not only the one just created. The reply lists them all.
- The reply carries `testPlanId` but NOT the suite id; the suite is found by
  listing the plan's suites (the app already does this: `find_requirement_suite`).

So the portal reaches, with a Basic account, the outcome the public API refuses.
It is an INTERNAL controller (`_api`, not `_apis`), undocumented, session-bound
in the portal, versioned by `__v=5`. It can change without notice.

### 2.4 Already committed on `main` (unreleased as of writing)
- `db1640f` — a whole-batch refusal's sentence is read from where ADO nests it
  (`value.Message`, capital M) instead of "HTTP 500".
- `a0b038b` — one suite attempt per AREA (the permission is per area path), the
  error names the area and the skipped plans. No more 48-plan walks.
- `9f95b0a` — every 401/403/404 logs ADO's body (`refused()` in
  `src-tauri/src/ado/transport.rs`). This is what settled §2.2.
- `b7aa1fe` — the no-suite message names Basic + Test Plans first, then the
  area permission, then the Boards route.

## 3. The decision
Use the internal portal controller as a **guarded fallback** so nobody has to
create the suite by hand, and log it thoroughly so that when it breaks we see
how and fix it. It is NOT the primary path: the documented API stays first, and
the fallback runs only after that API answers 403.

## 4. Proposed change

### 4.1 Capture first (five minutes, on the other machine)
The request BODY was not observed (the tool records replies only, and the page
blocks cross-origin reads of its script bundles). A second watched save on
2026-09-22 (#157953, a stub) added two facts:
- **Boards only makes this call when the PBI has NO requirement suite yet.**
  With suite 157944 in place the save fired only `GetWitTestsForKanbanBoard`;
  a requirement suite picks up new Tested-By cases by itself. The app's
  fallback fires under the same condition (§4.4 already guarantees that: it
  runs only after the public create was needed and refused).
- The controller's body convention, from the sibling call it did make:
  `POST …/_api/_testManagement/GetWitTestsForKanbanBoard?teamId=…&__v=5` with
  `Content-Type: application/json` and body `{"userStoryIds":"[145386]"}` -
  JSON whose array values are JSON **strings**. Expect the add call to follow
  suit (something like `{"requirementId":145386,"testCaseIds":"[157941]"}`);
  the field names are the unknown, and a 400 from a wrong name says so.
So the capture needs a PBI that has no suite yet. Before coding:
1. Chrome → the Gamma Guardians board → a PBI card that has no suite (and on
   which a test case is genuinely wanted) → "…" → Add Test → fill Steps +
   Module → Save, with DevTools' Network tab open.
2. Open the `AddWitTestCasesToRequirementSuite` request → **Payload** and
   **Headers**. Record: the JSON body (expected shape: the requirement id and
   the test case id(s); note the exact field names), `Content-Type`, and any
   `__RequestVerificationToken` / `X-TFS-…` headers.
3. Put the body verbatim into the code comment and into the test fixture.

Assumption to verify in the same session: with a **bearer** token the
anti-forgery header is not required (CSRF checks apply to cookie auth). If the
call with bearer + JSON body answers 200 in the probe (§4.5), the assumption
holds; if it answers 400/403 mentioning the token, the fallback cannot work from
the app and this design stops here — write that down and keep the message path.

### 4.2 Resolving the two ids the URL needs
- `projectId` (GUID): `GET {org}/_apis/projects/{project}` → `id`. Cache it
  with the project (Rust cache, `cache/keys.rs`; one cache per side).
- `teamId`: the team whose area covers the PBI's area path. Ask
  `GET {org}/_apis/projects/{projectId}/teams` and for each team
  `GET {org}/{projectId}/{teamId}/_apis/work/teamsettings/teamfieldvalues`; pick
  the team whose `values[].value` (with `includeChildren`) covers the PBI's
  area, preferring the longest match. Fall back to the project's
  `defaultTeam.id` from the projects call. Cache the area→team answer.
  (For HRM\Gamma Guardians the answer is `71215fd9-…`, above.)

### 4.3 Where it goes
`src-tauri/src/ado_testplan/plans.rs`, next to `create_requirement_suite`:

```rust
/// The route the Boards "Add Test" button takes. INTERNAL portal controller
/// (`_api`, not `_apis`) - undocumented, versioned by `__v`, watched on
/// 2026-09-22 (docs/superpowers/specs/2026-09-22-requirement-suite-boards-fallback-design.md).
/// Used ONLY after the documented create answered 403 (access level: Basic
/// without Test Plans). Adds `case_ids` to the PBI's requirement suite,
/// creating the team's current-sprint plan and the suite when they do not
/// exist. Returns the plan id the reply names; the suite id is found by
/// listing that plan's suites. Every request and reply is logged whole,
/// because the day this stops answering 200 is the day it changed.
pub async fn boards_add_to_requirement_suite(
    &self, org: &str, project_id: &str, team_id: &str, pbi_id: i32, case_ids: &[i32],
) -> Result<i32, AdoError> {
    let url = format!(
        "{}/{}/{}/_api/_testManagement/AddWitTestCasesToRequirementSuite?teamId={}&__v=5",
        self.base_url, percent_encode_segment(org), project_id, team_id
    );
    let body = serde_json::json!({ /* the captured shape, §4.1 */ });
    crate::applog::info(format!("boards suite route: POST {} body {}", tidy(&url), body));
    let data = self.post_json(url, &body).await?;           // 401/403/404 bodies are logged by refused()
    let raw: String = data.to_string().chars().take(2000).collect();
    crate::applog::info(format!("boards suite route answered: {raw}"));
    data["testPlanId"].as_i64().map(|p| p as i32).ok_or_else(|| AdoError::Http {
        status: 0,
        body: format!("the Boards route answered without a testPlanId: {raw}"),
    })
}
```

`post_json` keeps the GET/POST/PATCH-only invariant (no DELETE anywhere) and
the pacing/logging funnel; nothing new on the transport side.

### 4.4 Ordering in the upload (`src-tauri/src/commands/queue.rs`, `submit_queue`)
Today the suite is resolved BEFORE the batch (the plan scan runs first). The
fallback needs created case ids, so:

1. Resolve as today. On `AdoError::Http { status: 403, .. }` from
   `ensure_requirement_suite_cb` (the "every candidate forbids" error — it is
   `Http`, not `Forbidden`, so the sentence reaches the screen), do NOT emit
   `SuiteNotCreated` yet; remember the sentence as `suite_pending`.
2. Run the batch as today.
3. If `suite_pending` and at least one case was created or updated: resolve
   project id + team id (§4.2), call `boards_add_to_requirement_suite` with
   the ids of the cases in this upload (all of them — the endpoint pulls in
   every Tested-By case anyway, and passing all is the honest description of
   what was uploaded).
4. On `Ok(plan_id)`: `find_requirement_suite` scoped to that plan (or list
   `Plans/{plan_id}/suites` and take the `requirementTestSuite` whose
   `requirementId == pbi_id`), `remember_suite(...)`, log
   `requirement suite for #{pbi} created through the Boards route: plan {p}, suite {s}`,
   and surface it the way a created suite is surfaced today.
5. On `Err`: emit `SuiteNotCreated` with the ORIGINAL sentence plus one line:
   "The Boards route did not work either - Settings → Logs has what it said."
   Never retry, never loop.

Run Tests resolves suites through the same `ensure_requirement_suite_cb`; it
has no created ids, so it keeps the message path. (Optional later: let Run
Tests use the route with the PBI's existing Tested-By case ids.)

### 4.5 Probe before wiring (cheap, decisive)
In a dev build, on a PBI that has cases but no suite, call the new function
once from the dev panel (or a temporary command) with one existing case id and
read the log. 200 + `testPlanId` → wire §4.4. 400 naming a field → fix the
body from §4.1. 403 → see the assumption in §4.1.

### 4.6 Tests (`src-tauri/tests/ado_testplan.rs`, wiremock)
- `boards_route_posts_the_captured_body_and_reads_the_plan_id`: mock the
  `_api/_testManagement/AddWitTestCasesToRequirementSuite` path with
  `teamId` and `__v=5` query matchers; assert the body equals the captured
  fixture and the function returns the `testPlanId`.
- `boards_route_without_a_plan_id_is_an_error_not_a_guess`: reply `{}` → Err.
- `submit_falls_back_to_the_boards_route_after_a_403_and_then_finds_the_suite`:
  public create 403 → batch 200 → internal 200 with `testPlanId` → suites list
  for that plan contains the requirement suite → `remember_suite` called,
  `SuiteNotCreated` NOT emitted.
- `submit_reports_both_failures_when_the_boards_route_fails_too`: internal
  404 → `SuiteNotCreated` carries the original sentence and the added line.
- Team resolution: longest-area-match wins; default team when nothing matches.

All under `tests/` (integration tests only — see CLAUDE.md), no `#[cfg(test)]`.

### 4.7 Logging (the contract with the future)
Every call through the route writes, at INFO: the full URL (tidied), the body,
and the reply (2000 chars). A non-2xx already writes ADO's body via
`refused()`. Grep for `boards suite route` in Settings → Logs; a bug report
ships it. Changelog line for the release:
"When Azure DevOps refuses to create a test suite for your access level, the
app now takes the same route the Boards 'Add Test' button does, so the cases
still land in a suite. If that route ever stops working, the log says exactly
what Azure DevOps answered."

## 5. Risks, stated
- Undocumented endpoint: may change shape or vanish with any ADO deployment.
  Mitigation: fallback only, full logging, no retries, message path intact.
- It creates a NEW plan in the team's current sprint rather than reusing an
  older sprint plan — that is what Boards does too; acceptable.
- It pulls EVERY Tested-By case into the suite, not just this upload's. That
  is the requirement-suite semantics anyway.
- `__v=5` may be bumped server-side; keep it a constant with the date it was
  seen, and log the reply so a new required version shows up in the answer.

## 6. Reference facts
- Org/project: PeoplesHR / HRM, project id `73d6b311-d948-40cc-8c86-fc600c1edb87`,
  Gamma Guardians team id `71215fd9-79d5-4742-8756-01821a02e014`.
- #145386's suite now: plan 157942, suite 157944; 69 + 66 uploaded today plus
  two stubs to delete in ADO: #157941 (duplicates #157801) and #157953
  ("Sample - Boards route capture (delete me)").
- #138416: 931 cases, at the 1000-link limit.
- Access level check: Organization settings → Users → the account → Access level.
