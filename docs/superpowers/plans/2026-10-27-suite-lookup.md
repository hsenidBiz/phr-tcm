# Faster requirement-suite lookup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finding a PBI's requirement suite stops walking every test plan when it does not have to, a found suite is remembered across restarts, the same walk never runs twice at once, and a walk that is still needed says what it is doing.

**Why (owner, 2026-10-27, with a screenshot of Run Tests at "Scanning test plans 481 / 482"):** the log showed 482 plans read 8 at a time; 481 finished in about 45 s; plan 55291 has thousands of suites and its suite list was read about 99 pages one after another (1 to 2.5 s each), so the bar sat at 481 / 482 for about 40 s. The same pages were also read twice at once. Owner chose all four fixes: "Go ahead with 1, 2, 3 and 4".

**Findings the plan rests on (controller, from the code):**
- Three things walk plans for one PBI: Run Tests' `ensure_pbi_suite` (`commands/testplan.rs`, non-exhaustive: area-matched plans only when any), App's background `find_pbi_suite` (`src/App.tsx` ~1288, exhaustive: EVERY plan), and the plans tree `list_plans_with_suites` (`src/App.tsx` ~1203 prefetch, `Suites.tsx`, `ManageCases/index.tsx`, `lib/plansAll.ts`), which pages every plan's whole suite list.
- `SuiteScanProgress { done, total }` is emitted by BOTH `ensure_pbi_suite` and `list_plans_with_suites`, and Run Tests (`RunPanel/index.tsx` ~141) and Search Suites both listen to it: Run Tests' bar can be showing the plans-tree prefetch's walk, not its own. 481 / 482 one at a time is the per-plan counter of `list_plans_with_suites_cb` (the ensure scan counts in batches of 8).
- A found suite is remembered for the session only (`ado_testplan::remember_suite`, `mod.rs` ~269).
- Azure DevOps has `GET https://dev.azure.com/{org}/_apis/testplan/suites?testCaseId={id}&api-version=7.1` (org level, "Get Suites By Test Case Id"): every suite holding that test case, each with its plan, `suiteType` and `requirementId`. Confirm the response shape in the reference index (`xerj search --prefix refadonode "getSuitesByTestCaseId"`, optional) and pin it with a wiremock test.

**Tech Stack:** Rust (reqwest, wiremock tests in `src-tauri/tests/suite/`), tauri-specta bindings (regenerate with `cargo test --test bindings`), React.

## Global Constraints

- GET / POST / PATCH only; no DELETE anywhere.
- All ADO calls through `src-tauri/src/ado*`; user-facing errors name no URL (`network_error`).
- Rust tests only in `src-tauri/tests/suite/` (one binary; a new module gets a `mod` line in `suite/main.rs`); process-wide state takes its lock from `suite/serial.rs`; pooled wiremock addresses repeat, so a test empties any cache key holding a server address first.
- Caching: Rust side only through `src-tauri/src/cache/` with the key and TTL in `cache/keys.rs`; never a private cache. Never change an existing key string.
- `src/bindings.ts` is generated; never hand-edit.
- One build or test command at a time. Never set CARGO_TARGET_DIR.
- No em or en dashes in user text; the changelog is end-user words.

## Review Focus

1. A PBI whose cases sit in several suites (a static suite and the requirement suite, or requirement suites of OTHER PBIs): only a `requirementTestSuite` whose `requirementId` is this PBI, in this project, counts.
2. A PBI with no test cases, or whose cases are in no requirement suite: falls back to today's walk, unchanged in result.
3. A remembered suite that has since been deleted or moved: Run Tests' Shift-click re-detect overwrites it; a points read that answers NotFound for the remembered suite forgets it and looks again once.
4. Two lookups for the same PBI started together (Run Tests + the background finder): one walk, both get its answer; a lookup for a different PBI is not blocked by it.
5. The progress a screen shows is its own lookup's, never another screen's walk.

---

### Task 1: Ask the PBI's test cases where their suite is

**Files:** `src-tauri/src/ado_testplan/plans.rs` (and `mod.rs` if a shared helper fits there), a new test module `src-tauri/tests/suite/suite_lookup.rs` (+ `mod` line).

**Produces:** `AdoClient::suite_from_cases(&self, org: &str, project: &str, pbi_id: i32) -> Result<Option<EnsuredSuite>, AdoError>`:
- reads the PBI's Tested By test case ids (relations `Microsoft.VSTS.Common.TestedBy-Forward`; reuse the existing relations read if one exists - Grep `TestedBy`);
- asks Get Suites By Test Case Id for up to the first 3 of them, one after another, stopping at the first hit;
- a hit is a suite with `suiteType == "requirementTestSuite"`, `requirementId == pbi_id`, and a plan in `project` (compare the suite's project / plan project the API returns; if it does not say, confirm the plan with one GET of the plan in `project`);
- returns `EnsuredSuite { plan_id, plan_name, suite_id, created_plan: false }`;
- Forbidden / NotFound on one case's lookup skips that case; other errors propagate.
- `find_pbi_requirement_suite` and `ensure_inner` call it first and only walk plans when it returns None.

- [ ] wiremock tests: hit on the first case (no plan walk requested: assert the plans list endpoint got 0 calls); first case's suites hold only another PBI's requirement suite and a static suite, second case holds the right one; no Tested By links -> walk happens as today; a 403 on one case's lookup skips to the next. Fail, implement, pass: `cd src-tauri && cargo test --test suite suite_lookup::`.
- [ ] Commit `fix(v2): a PBI's suite is found from its own test cases before any plan is walked`.

### Task 2: Remember the found suite across restarts, and walk once at a time

**Files:** `src-tauri/src/ado_testplan/mod.rs` (`remember_suite` and its reader), `src-tauri/src/cache/keys.rs`, the plan-walk entry points, tests in `suite_lookup.rs`.

- [ ] `remember_suite` also writes the suite to the persistent cache (`cache::put`, key built in `keys.rs` from base url + org + project + pbi, TTL 30 days); the reader checks the session memory, then the persistent cache, before any lookup. Shift-click re-detect (Run Tests) still bypasses and overwrites. Add `forget_suite(base_url, org, project, pbi_id)`; Run Tests' points read (`commands/testplan.rs` or wherever points are read for a remembered suite) answering NotFound for the remembered suite calls it and resolves once more.
- [ ] Single flight: concurrent lookups for the same (base url, org, project, pbi) share one in-flight future (a `tokio::sync::Mutex` / `OnceCell` map keyed by that tuple, removed when done), and concurrent `list_plans_with_suites` for the same (org, project) likewise share one walk. Different keys run in parallel.
- [ ] Tests: remembered suite survives a session wipe (clear the session cache, keep the persistent one) and makes no request; NotFound forgets and re-resolves; two concurrent lookups for one PBI hit the suites endpoint once (wiremock `expect(1)`); two PBIs both proceed. Run `cargo test --test suite suite_lookup::` and the existing testplan modules (`cargo test --test suite testplan`). Commit `fix(v2): a found suite is remembered across restarts, and one walk serves every screen that asks`.

### Task 3: Each screen shows its own walk, and a long plan says so

**Files:** `src-tauri/src/events.rs` (`SuiteScanProgress`), `src-tauri/src/commands/testplan.rs`, `src-tauri/src/ado_testplan/plans.rs`, `src/bindings.ts` (regenerate), `src/screens/RunPanel/index.tsx`, `src/screens/Suites.tsx`, their tests, `src/lib/changelog.ts` is NOT touched (the release bump adds the line).

- [ ] `SuiteScanProgress` gains `kind: "suite" | "plans"` (a specta enum or string union) and `pages: Option<u32>`: `ensure_pbi_suite` / the finder emit `suite`, `list_plans_with_suites` emits `plans`. Run Tests listens only to `suite`, Search Suites only to `plans`.
- [ ] When every plan but the slowest has finished and one plan is still paging, the walk reports that plan's page count (`pages`) as it goes (a per-page callback from the suite-list paging). The screens then show "Reading a large plan (page N)" instead of the frozen count; otherwise they show "Scanning test plans" with done / total as today.
- [ ] Regenerate bindings (`cd src-tauri && cargo test --test bindings`); Rust tests for the kind and pages fields; vitest for both screens (a `plans` event does not move Run Tests' bar; a `pages` event shows the sentence). Run `cargo test --test suite suite_lookup::`, `npx vitest run src/screens/RunPanel src/screens/RunPanel.test.tsx src/screens/Suites.test.tsx`, `npx tsc --noEmit`. Commit `fix(v2): each screen shows its own plan scan, and a long plan says which page it is on`.
