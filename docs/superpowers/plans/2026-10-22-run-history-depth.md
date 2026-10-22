# Run History Depth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the History column on Run Tests (and the runner's history) shows up to 5 outcomes for every case that has been run, not only for cases whose results fall in the plan's 15 newest runs or on the first page of a run's results.

**Architecture:** `AdoClient::run_history` (`src-tauri/src/ado_testplan/history.rs`) gets the suite's case ids, pages the plan's run list and each run's results with one shared `$top/$skip` helper, walks the runs newest first in batches of `SUITE_SCAN_CONCURRENCY` until every asked case has 5 outcomes (or the runs run out, or 150 runs were walked), and keeps each **completed** run's per-case outcomes in the Rust cache's session tier, so a repeat open reads only the run list plus new or unfinished runs. One `applog::info` summary line per sweep says which defect bit. Return type (`Vec<CaseHistory>` / `RunOutcome`) is unchanged; the UI's HistoryDots is untouched.

**Tech Stack:** Rust (tokio, futures `buffer_unordered`, wiremock in tests), React 19 + TS (TanStack Query, `persistentQuery`), vitest, tauri-specta bindings.

**Spec (the bug, no separate spec file):** On Run Tests (`src/screens/RunPanel/index.tsx`) a 25-case suite shows "Last outcome" for every case (from the suite's test points) but History dots for only 3 cases, one dot each; 22 show an em dash. Two causes, both real in the code, either or both hit the user (no token to check the real plan):
1. `run_history` GETs the plan's runs, sorts newest first and keeps 15 (`RUN_HISTORY_RUNS`) - a plan-wide cap shared by every suite in the plan; Auto Run uploads make many runs per plan, so a case whose results are older gets nothing.
2. Each run's results are read with `GET test/Runs/{id}/results?api-version=7.1` and no `$top/$skip`. That Test Management endpoint pages with `$top/$skip` and sends no continuation header (see the comment on `get_run_results`, `src-tauri/src/ado_testplan/runs.rs` ~159-163), so an unpaged read gets only the first page.
Also found while planning: the run list itself (`GET test/runs?planId=`) is documented with `$skip/$top` and returns runs in ascending id order; if the server caps an unpaged list, the "newest" runs are never seen at all. It is paged too.

## Global Constraints

- ADO client issues GET/POST/PATCH only - no DELETE anywhere. This change adds GETs only.
- One cache per side. Rust: `src-tauri/src/cache/` only, key + TTL in `src-tauri/src/cache/keys.rs`. Webview: `src/lib/cache.ts` only; never change an existing `cacheKeys` string - add a new key.
- Rust tests only in `src-tauri/tests/suite/<module>.rs` (here: the existing `ado_testplan.rs`, already in `suite/main.rs`). Never a `#[cfg(test)]` in `src/`, never a new file under `tests/`.
- `src/bindings.ts` is generated: `cd src-tauri && cargo test --test bindings`. Never hand-edit it.
- User-facing errors name no URL. The new log line carries no URL either (plan id and counts only).
- One build/test command at a time; never set `CARGO_TARGET_DIR` / `--target-dir`.
- Commits via Bash heredoc: `git commit -F - <<'EOF' ... EOF`, ending with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; confirm with `git log -1`. Do not push, do not release, do not bump the version.
- `src/ui-consistency.test.ts` is never weakened.

## Review Focus

1. **A plan with hundreds of runs hammering ADO.** A suite with even one never-run case (or a case with under 5 results ever) never reaches "all have 5", so every first open walks the full 150-run cap. That is why completed runs are kept in the session cache: the second open costs the run-list pages plus only new/unfinished runs. Check the cap is counted over runs walked (cached ones included), batches are `SUITE_SCAN_CONCURRENCY` (8) wide and awaited batch by batch (never all 150 futures in one `buffer_unordered`), and the run-list paging stops on a short page or `MAX_PAGES`.
2. **The global pacer (`ado::throttle`).** Every GET is paced process-wide; at Gentle (800 ms) a cold 150-run sweep is about 2 minutes with History showing the seed/dashes meanwhile. Acceptable for a background column, but the sweep must never block the points query (it is a separate query) and must never run unbounded.
3. **Cache correctness.** Only runs whose list `state` is `Completed` (case-insensitive) are kept; InProgress / NotStarted / Waiting / NeedsInvestigation / Aborted are always re-read. A 403/404 run is skipped and not kept. Keys carry `base_url` (as `keys::suite` does) so parallel tests on different mock servers never share entries; the session tier is wiped by `claim_for` when a different account signs in. The kept value is every verdict row of the run (all cases, not only the asked ones), so another suite of the same plan can reuse it.
4. **Row shape quirks.** Results with no verdict (`is_no_verdict`) or no parsable `testCase.id` (`id_i32`) are dropped; several rows for one case in one run (multiple configurations) each count as an outcome, in API order - as today (the existing test pins 3 rows of case 201 in run 2).
5. **History from another suite of the same plan.** History is per test case across the whole plan, as today (results are not filtered by test point). What changes: the returned list contains only the asked cases, and the webview key now includes the suite, so suite A's history is never painted on suite B from the disk seed.

---

### Task 1: Paged, adaptive, cached sweep (Rust)

**Files:**
- Modify: `src-tauri/src/ado_testplan/mod.rs` (add `SKIP_PAGE` + `get_skip_paged` next to `MAX_PAGES` / `may_continue`)
- Modify: `src-tauri/src/ado_testplan/runs.rs` (`get_run_results` ~153-185 uses `get_skip_paged`; drop its local `PAGE` loop, keep its comment)
- Modify: `src-tauri/src/ado_testplan/history.rs` (rewrite `run_history`)
- Modify: `src-tauri/src/cache/keys.rs` (add `RUN_OUTCOMES_TTL`, `run_outcomes`)
- Modify: `src-tauri/src/commands/runs.rs:294` - only the call: `.run_history(&organization, &project, plan_id, &[])` so the crate compiles (the real argument comes in Task 2)
- Test: `src-tauri/tests/suite/ado_testplan.rs`

**Interfaces:**
- Consumes: `AdoClient::get_json`, `AdoClient::tp_base`, `self.base_url` (`pub(crate)`), `super::{id_i32, is_no_verdict, CaseHistory, RunOutcome, SUITE_SCAN_CONCURRENCY, MAX_PAGES}`, `crate::cache::{session_fresh, session_put}`, `crate::applog::{info, warn}`.
- Produces:
  - `src-tauri/src/ado_testplan/mod.rs`:
    ```rust
    /// Rows per page for the Test Management API's $top/$skip lists (runs,
    /// run results). That API sends no continuation header.
    pub(crate) const SKIP_PAGE: usize = 200;

    impl AdoClient {
        /// Every row of a Test Management list that pages with $top/$skip.
        /// `url` already carries its query (it contains `?`, ends with
        /// `api-version=7.1`); this appends `&$top={SKIP_PAGE}&$skip={rows so far}`.
        /// Stops on a page shorter than SKIP_PAGE, or at MAX_PAGES with
        /// `applog::warn("stopped paging {what} at the {MAX_PAGES}-page cap - results may be incomplete")`.
        /// Returns the rows and how many pages were read. Errors propagate.
        pub(crate) async fn get_skip_paged(
            &self,
            url: String,
            what: &'static str,
        ) -> Result<(Vec<serde_json::Value>, usize), AdoError>
    }
    ```
    (Owned `url` and `&'static str` so the future can be built eagerly in the batch, like the current code - closure-captured refs break the tauri command macro.)
  - `src-tauri/src/cache/keys.rs`:
    ```rust
    /// How long a completed test run's per-case outcomes are kept (session
    /// tier only). A completed run's results do not change, so this is in
    /// effect the session; a different account signing in clears it
    /// (`claim_for`). Session, not durable: a deep sweep keeps up to 150
    /// runs per plan and the durable file is rewritten whole on every change.
    pub const RUN_OUTCOMES_TTL: Duration = Duration::from_secs(24 * 60 * 60);

    /// One completed run's verdict rows, for Run Tests' history sweep
    /// (session tier only). The base_url is in the key for the same reason as `suite`.
    pub fn run_outcomes(base_url: &str, org: &str, project: &str, run_id: i32) -> String {
        format!("run-outcomes:{base_url}|{org}|{project}|{run_id}")
    }
    ```
  - `src-tauri/src/ado_testplan/history.rs`:
    ```rust
    pub async fn run_history(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        test_case_ids: &[i32],
    ) -> Result<Vec<CaseHistory>, AdoError>
    ```
    Private items in the same file: `const RUN_HISTORY_PER_CASE: usize = 5;`, `const RUN_HISTORY_UNTARGETED_RUNS: usize = 15;`, `const RUN_HISTORY_MAX_RUNS: usize = 150;`, `#[derive(Clone)] struct CaseOutcome { test_case_id: i32, outcome: RunOutcome }` (the session value is `Vec<CaseOutcome>`), `struct PlanRun { id: i32, date: String, completed: bool }`, `fn run_case_outcomes(run_id: i32, rows: &[serde_json::Value]) -> Vec<CaseOutcome>` (the current per-row parsing: skip `is_no_verdict`, skip no `id_i32(&r["testCase"]["id"])`, fill `RunOutcome` exactly as today).

**Behaviour of `run_history` (exact):**
1. Run list: `get_skip_paged(format!("{}/test/runs?planId={}&api-version=7.1", self.tp_base(org, project), plan_id), "test runs")`. Errors propagate as today. Each row → `PlanRun { id: r["id"].as_i64()? as i32, date: completedDate or else startedDate or "", completed: r["state"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("Completed")) }`. Sort by `date` descending, then `id` descending (undated last).
2. Targets: `HashSet<i32>` of `test_case_ids`. Empty → untargeted: walk at most `RUN_HISTORY_UNTARGETED_RUNS` runs, never stop early, return every case seen (today's behaviour, used by callers with no case list). Non-empty → walk at most `RUN_HISTORY_MAX_RUNS`.
3. Walk `runs[..limit]` in `chunks(SUITE_SCAN_CONCURRENCY)`. Per chunk: for each run, `crate::cache::session_fresh::<Vec<CaseOutcome>>(&keys::run_outcomes(&self.base_url, org, project, id), keys::RUN_OUTCOMES_TTL)` - a hit counts as "from cache"; the misses are fetched as eager futures through `stream::iter(futs).buffer_unordered(SUITE_SCAN_CONCURRENCY)`, each `get_skip_paged(format!("{}/test/Runs/{}/results?api-version=7.1", ...), "run results")`. Then, in the chunk's newest-first order: `Err(Forbidden | NotFound)` → skip (count "skipped", never cached); other `Err` → return it; `Ok((rows, pages))` → `run_case_outcomes`, count `pages > 1` as "multi-page", and if `run.completed` → `session_put(key, outcomes.clone())`.
4. Aggregate each run's outcomes (in order) into `HashMap<i32, Vec<RunOutcome>>`, only for cases in the targets when targeted, capped at `RUN_HISTORY_PER_CASE`.
5. After each chunk, when targeted and every target has `>= RUN_HISTORY_PER_CASE` → stop ("every asked case has 5").
6. Log once, then return `CaseHistory` per aggregated case (targeted: only targets with at least one outcome):
   `applog::info(format!("run history: plan {plan_id}: {listed} runs listed ({list_pages} page(s)), {walked} walked ({fetched} read, {cached} from cache, {skipped} skipped, {multi_page} with more than one page of results), {covered}/{asked} asked cases with history, {full} with 5, stopped: {reason}, {ms} ms"))`, `reason` one of `"every asked case has 5"`, `"runs exhausted"`, `"cap of 150 runs"`, `"untargeted (newest 15)"`; `asked` = targets.len() (0 when untargeted). `info`, not `debug`: it is one line per sweep and the Logs viewer hides debug by default. `walked` > 15 with history found there means cause 1; `multi_page` > 0 means cause 2.
7. Update the doc comment on `run_history` to describe all of the above in 6-8 lines.

- [ ] **Step 1: failing tests** in `src-tauri/tests/suite/ado_testplan.rs` (add `use wiremock::matchers::path_regex;`; count requests with `server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "...").count()`; each test its own `MockServer` and `AdoClient::with_base_url("t".into(), server.uri())`; give the per-run catch-all `Mock::given(method("GET")).and(path_regex(r"^/o/p/_apis/test/Runs/\d+/results$"))` and mount specific run mocks with `.with_priority(1)`; a full-page list mock must match `query_param("$skip", "0")` so the follow-up page does not loop to `MAX_PAGES`. Tests that make 100+ requests take `let _pacer = crate::serial::pacer();` first so no other test's pacer level slows them):
  - existing `run_history_aggregates_newest_first_and_caps_at_five`: call becomes `client.run_history("o", "p", 9, &[]).await` - assertions unchanged.
  - `run_history_reaches_a_case_whose_only_results_are_in_the_twentieth_newest_run`: 25 runs, ids 1..=25, `"state": "Completed"`, `completedDate` `2026-07-{id:02}T10:00:00Z`; run 6 (the 20th newest) has `{"id": 60, "outcome": "Passed", "completedDate": "2026-07-06T10:05:00Z", "testCase": {"id": "501"}}`; the catch-all returns one Passed row for case `"900"`. Ask `&[501]`. Assert the result is exactly one `CaseHistory` with `test_case_id == 501`, one outcome, `run_id == 6`, `result_id == 60` (and so case 900 is not returned).
  - `run_history_reads_every_page_of_a_large_run`: one Completed run id 1; `Runs/1/results` with `query_param("$skip","0")` → 200 rows `Passed` for case `"601"`; with `query_param("$skip","200")` → 1 row `{"outcome": "Failed", "testCase": {"id": "602"}}`. Ask `&[601, 602]`. Assert 601 has 5 outcomes and 602 has 1 outcome `"Failed"` (the second page was read).
  - `run_history_reads_every_page_of_the_run_list`: `test/runs` with `query_param("$skip","0")` → 200 runs ids 1..=200 dated `2026-01-01T10:00:00Z`; with `query_param("$skip","200")` → run 201 dated `2026-09-01T10:00:00Z`; `Runs/201/results` (priority 1) → Passed for case `"801"`; catch-all → `{"value": []}`. Ask `&[801]`. Assert 801's first outcome has `run_id == 201`. (Pacer lock.)
  - `run_history_stops_once_every_asked_case_has_five`: 40 Completed runs ids 1..=40 (date by id); catch-all → one Passed row for case `"301"`. Ask `&[301]`. Assert results requests across all runs == 8 (one batch), and 301's run ids are `[40, 39, 38, 37, 36]`.
  - `run_history_stops_at_the_run_cap`: 160 Completed runs (one list page); catch-all → `{"value": []}`. Ask `&[302]`. Assert results requests == 150 and the history is empty. (Pacer lock.)
  - `run_history_skips_runs_it_may_not_read`: runs 1, 2, 3; `Runs/2/results` → 403, `Runs/3/results` → 404, run 1 → Passed for `"303"`. Ask `&[303]`. Assert `Ok`, 303 has one outcome with `run_id == 1`. Call again on the same client and assert runs 2 and 3 were each requested twice (not kept).
  - `run_history_does_not_keep_an_in_progress_run`: run 1 `"state": "InProgress"`, `startedDate` only, results Passed for `"304"`. Call `run_history(.., &[304])` twice on one client. Assert `Runs/1/results` was requested 2 times and both answers hold the outcome.
  - `run_history_reads_a_completed_run_once`: runs 1, 2 Completed with Passed rows for `"305"`. Call twice on one client. Assert `Runs/1/results` and `Runs/2/results` each requested exactly 1 time, `test/runs` requested 2 times, and the second answer's run ids equal the first's (`[2, 1]`).
  - `run_history_logs_one_summary_line`: take `let _log = crate::serial::log_tail();`; 2 Completed runs, case `"306"`; call `client.run_history("o", "p", 9306, &[306])` (plan id unique to this test; the runs mock matches path only, so the id does not matter to it). Assert exactly one line in `v2_lib::applog::recent(500)` whose `message` starts with `"run history: plan 9306:"`, that it contains `"runs listed"`, `"1/1 asked cases with history"` and `"stopped: runs exhausted"`, and that it does not contain `"http"`.
  - `run_results_are_read_across_every_page` (existing) must keep passing against the refactored `get_run_results`.
- [ ] **Step 2:** `cd src-tauri && timeout 1800 cargo test --test suite -- ado_testplan::` → FAIL (compile error on the new argument first; after a stub, the depth/paging/cache tests fail).
- [ ] **Step 3:** implement `SKIP_PAGE` + `get_skip_paged`, refactor `get_run_results` onto it, the keys, the new `run_history`, and the `&[]` at `commands/runs.rs:294`.
- [ ] **Step 4:** same command → PASS; then `cd src-tauri && timeout 1800 cargo test --test suite -- cache::` → PASS (the cache tripwire: no private map was added).
- [ ] **Step 5:** commit:
  ```
  fix(v2): run history reads every page and walks back until each case has five outcomes

  The sweep kept the plan's 15 newest runs and read one page of each run's
  results, so cases whose results were older or on a later page showed no
  history while Last outcome did. It now pages the run list and the results,
  walks back in batches of 8 until every asked case has 5 outcomes (150-run
  cap), keeps completed runs' outcomes for the session, and logs one summary
  line per sweep.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  ```

### Task 2: Command, bindings and webview callers

**Files:**
- Modify: `src-tauri/src/commands/runs.rs` (~283-296)
- Regenerate: `src/bindings.ts` (`cd src-tauri && cargo test --test bindings`)
- Modify: `src/lib/cache.ts` (~71), `src/lib/cache.test.ts` (~192)
- Modify: `src/screens/RunPanel/index.tsx` (~187-202; the history query moves below the `points` query ~207)
- Modify: `src/screens/RunnerWindow.tsx` (~193-200)
- Test: `src/screens/RunPanel.test.tsx`, `src/screens/RunnerWindow.test.tsx` (only if an assertion breaks)
- Leave alone: `src/dev/demo.ts:1085` and `src/tour/tourBackend.ts:69` (`runHistory: () => ...` ignores arguments, still type-checks).

**Interfaces:**
- Consumes: Task 1's `run_history(org, project, plan_id, &[i32])`.
- Produces:
  - Command:
    ```rust
    /// Recent outcome history (last 5, newest first) for the given test
    /// cases of a plan - the suite's cases, so the sweep knows when it can
    /// stop. An empty list returns every case in the plan's 15 newest runs.
    #[tauri::command]
    #[specta::specta]
    pub async fn run_history(
        app: tauri::AppHandle,
        organization: String,
        project: String,
        plan_id: i32,
        test_case_ids: Vec<i32>,
    ) -> Result<Vec<ado_testplan::CaseHistory>, ado::AdoError>
    ```
    body passes `&test_case_ids`.
  - Generated binding: `runHistory: (organization: string, project: string, planId: number, testCaseIds: number[]) => ...`.
  - `src/lib/cache.ts` `cacheKeys`: add right after `runHistory` (which stays, unchanged and pinned - the old string stays reserved):
    ```ts
    // Per suite: the history is asked for that suite's cases only.
    suiteRunHistory: (org: string, project: string, planId: number | undefined, suiteId: number | undefined) =>
      `run-history:${org}/${project}/${planId}/${suiteId}`,
    ```
    and pin it in `cache.test.ts` next to the `runHistory` pin: `expect(cacheKeys.suiteRunHistory("acme", "Web", 7, 71)).toBe("run-history:acme/Web/7/71");`. If a gate in `cache.test.ts` requires every key to be read somewhere, remove `runHistory` and its pin instead and say so in the commit body.
  - RunPanel (after the `points` query):
    ```ts
    const historyCaseIds = useMemo(
      () => [...new Set((points.data ?? []).flatMap((p) => (p.test_case_id == null ? [] : [p.test_case_id])))].sort((a, b) => a - b),
      [points.data],
    );
    const history = useQuery({
      queryKey: ["run-history", org, project, suite.data?.plan_id, suite.data?.suite_id, historyCaseIds.join(",")],
      ...persistentQuery({
        key: cacheKeys.suiteRunHistory(org, project, suite.data?.plan_id, suite.data?.suite_id),
        fetcher: () => unwrap(commands.runHistory(org, project, suite.data!.plan_id, historyCaseIds)),
        ...CACHE.outcomes,
      }),
      enabled: Boolean(suite.data && points.data),
      gcTime: 30 * 60_000,
      retry: false,
    });
    ```
    `historyByCase` follows it unchanged. `refreshPoints`' `invalidateQueries({ queryKey: ["run-history"] })` still matches (prefix).
  - RunnerWindow: same derivation from its `points.data` (the `runner-points` query); `queryKey: ["run-history", session?.org, session?.project, session?.planId, session?.suiteId, historyCaseIds.join(",")]`, `queryFn: () => unwrap(commands.runHistory(session!.org, session!.project, session!.planId, historyCaseIds))`, `enabled: Boolean(session && points.data)`, rest unchanged.

- [ ] **Step 1: failing test** in `src/screens/RunPanel.test.tsx`: `run history is asked for the suite's own cases` - `mockIPC((cmd, args) => ...)` as `mockAll` does, recording `args` for `run_history`; after `await screen.findByText("Valid login")`, `await waitFor(() => expect(seen).toEqual({ organization: "acme", project: "Web", planId: 9, testCaseIds: [201, 202] }))` (import `waitFor`).
- [ ] **Step 2:** `npx vitest run src/screens/RunPanel.test.tsx` → FAIL.
- [ ] **Step 3:** implement the command, then `cd src-tauri && timeout 1800 cargo test --test bindings` (regenerates `src/bindings.ts`; check its diff is only the `runHistory` line), then the cache key and the two screens. If the first RunPanel test's synchronous `getByTitle("Failed · 2026-07-12 (run #7)")` now runs before history arrives (history waits for points), change those two to `await screen.findByTitle(...)` - nothing else.
- [ ] **Step 4:** `npx tsc --noEmit`, then `npx vitest run src/screens/RunPanel.test.tsx src/screens/RunnerWindow.test.tsx src/lib/cache.test.ts src/ui-consistency.test.ts` → PASS.
- [ ] **Step 5:** commit:
  ```
  fix(v2): Run Tests and the runner ask run history for their own suite's cases

  The sweep stops once those cases have five outcomes each, and the
  webview keeps the history per suite, so one suite's dots are never
  painted on another suite of the same plan.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  ```

### Task 3: Wrap-up - the full gates

**Files:** none expected.

- [ ] **Step 1:** `cd src-tauri && timeout 3000 cargo test --tests` → PASS (one run; nothing else running).
- [ ] **Step 2:** `npm test` → PASS. `src/App.test.tsx` has a known load flake: one failure that passes on a re-run is that; two different ones are not.
- [ ] **Step 3:** `npm run build` → PASS.
- [ ] **Step 4:** `git grep -n "DELETE\|\.delete(" -- src-tauri/src/ado_testplan` shows nothing new; `git diff main --stat` lists only the files named above.
- [ ] **Step 5:** commit only if a gate made you change something, `fix(v2): <what>` with the Co-Authored-By line. Report to Avin: the manual check is to open the reported suite in `npm run tauri dev`, wait for History, then Settings → Logs (Info) and read the `run history: plan N:` line - `walked` above 15 with dots now filled means cause 1, `with more than one page of results` above 0 means cause 2. Release is his call (not part of this plan).
