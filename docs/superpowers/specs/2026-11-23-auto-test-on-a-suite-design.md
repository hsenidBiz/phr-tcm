# Auto Test on a test suite - design

Date: 2026-11-23. Owner asked for this, then confirmed approach A plus two decisions (the assistant sees the current suite; results go to the suite's own test points). Calls marked "my call" are open to the owner's correction.

## Why

Many regression cases live only in test suites (e.g. HRM Test Suit / Freeze Version / 26R2 / Master (Common) / Regression / Performance & Evaluation / Definition Wizard) with no PBI. Auto Test works only from the PBI in the top bar, so these cases cannot be scripted, run or reported there. Scripts themselves are saved per case id, so the gap is the scope, not the storage.

## Goals

- Open Auto Test on a test suite and do everything a PBI allows: see the cases, script them (by hand or with the assistant), run them watched or unattended, review, send results, and raise bugs.
- Send results to the suite's own test points.
- The assistant knows the current suite, as it knows the current PBI.

## Non-goals

- Child suites. A suite scope means the suite's own cases, exactly as Run Tests' folder mode does today. To run a parent folder's whole tree, open each child. (My call: matches Run Tests and avoids a 1,000-case surprise. Recursion can come later.)
- Writing new test cases into a suite (the writing tools stay PBI-based).
- A suite and a PBI chosen at the same time.

## Approaches considered

- **A. A suite is a scope beside the PBI (chosen).** Auto Test, its runs, reviews, order and the assistant take "a PBI or a suite".
- B. Paste case ids into Auto Test: quick, but no home for runs and history, and no link to the suite's test points.
- C. Link suite cases to a placeholder PBI in Azure DevOps: no app change, but it bends the owner's data.

## The design

### Choosing a suite

- **Top bar** (`ContextBar` / `PbiPicker`). The picker gains a PBI / Suite switch. In Suite, it searches suites by name using the existing suite search (the `search_test_suites` data) and shows plan › path › suite.
  - Picking a suite sets the **current suite**. The chip shows a folder icon, the suite name, and its plan in muted text.
  - Picking a PBI clears the suite, and the reverse.
  - (My call: this is the only way in under Basic mode, which hides Search Suites.)
- **Search Suites.** Each suite's More menu gains **Auto Test**. It sets the suite as current and opens Auto Test. A requirement suite with a PBI keeps "Use as current PBI" as today; Auto Test on it opens the suite scope, not the PBI.
- The current suite lives beside `pbi` in App state, is remembered across restarts like the PBI, and is cleared on sign-out or an org change.

### Auto Test with a suite

- **Cases:** a new Rust command `suite_test_cases_full(org, project, plan_id, suite_id, moduleRef, preconditionsRef) -> Vec<TestCaseFull>`.
  - It reads the suite's own test points (non-recursive, `get_test_points_in`), takes the case ids de-duplicated in suite order, then reuses `get_test_cases_by_ids` plus step attachments.
  - It returns the same shape the PBI path does, so the screen does not branch on it.
- **Scope type.** Everywhere Auto Test now takes `pbi: PbiHit` it takes `scope: Scope`, where `Scope = { kind: "pbi", pbi } | { kind: "suite", suite: { planId, suiteId, title, planName, project } }`, with a stable `scopeKey` (`pbi:<id>` / `suite:<plan>:<suite>`). This covers:
  - open cards;
  - run setup (`openRunSetup`, `BackgroundRun`, `ReviewRequest`);
  - review-request matching;
  - cancelling a setup when the scope changes;
  - App moving to a finished run's scope.
- **Run record.** `LocalRun` gains `suite: Option<SuiteRef { plan_id, suite_id, title }>`; `pbi_id` is 0 for a suite run.
  - Old records read as PBI runs unchanged (`#[serde(default)]`).
  - `autoRunReplay` takes the scope instead of `pbi_id`.
- **Execution order.** The order file is keyed by scope: `orders/<pbi_id>.json` as today, `orders/suite-<plan>-<suite>.json` for a suite. The assistant's `POST /autorun-order` validates against the scope's cases.
- **Past runs.** A suite run says "for suite <title>" where a PBI run says "for PBI #id". The "other scope" dimming compares scope keys.
- **Review and send.**
  - The scope-mismatch check compares scope keys.
  - The run name is `<suite title> - Auto Test`.
  - `autoRunPublish` gets the scope from the run. For a suite run it uses `{plan_id, suite_id}` from the run directly (no PBI area lookup, no requirement-suite search), finds the points with `get_test_points`, and creates, records and completes the test run as today. Every configuration's point is sent, as for a PBI.
  - A case no longer in the suite is skipped with "not in this suite any more".
- **Bugs.** From a suite run, a bug links to the test case only. The existing `create_work_item` already skips id 0. The dialog footer reads "Links to test case #id" with no PBI.
- **Flaky marks, last results, scripts and videos** are already per case id: no change.
- **Divergence notes** that need a PBI (`note_pbi_in`) are skipped for a suite scope.

### The assistant

- The bridge context gains `current_suite: Option<CurrentSuite { plan_id, suite_id, title, plan_name, project }>`, pushed by `useBridgeContextSync` beside the PBI and cleared with it.
- `get_current_pbi` answers with the suite when a suite is current. Its text says that no PBI is selected, names the current suite (plan, suite ids and title), and says to read its cases with `get_suite_test_cases`. (My call: no new tool. The assistant prompts already call `get_current_pbi` first.)
- The guides and prompts that say "the current PBI" (gen-tests, discover, heal; `ai_tools.rs`, `autorun/guide.rs`) gain one line: when a suite is current, the cases come from the suite.
- `/autorun-order` and anything else that reads `current_pbi` accept the suite.

### Basic mode

Auto Test is in Basic mode's rail, and the top bar's Suite switch works there. No other change.

## Left to the builder

The picker's look within the existing chip style, the exact remembered-state key, and the test layout.

## Tests (outline)

- **Rust:**
  - suite cases: own points only, de-duplicated, in order;
  - `LocalRun` old records still read as PBI runs;
  - the order file is keyed by scope;
  - publish for a suite run uses the run's plan and suite, skips a missing case with its sentence, and never searches for a PBI;
  - a bug with PBI 0 links only the case;
  - the bridge context and `get_current_pbi` answer with a current suite;
  - `/autorun-order` validates against suite cases.
- **Webview:**
  - picker Suite mode sets the chip; picking a PBI clears the suite;
  - Search Suites' Auto Test opens Auto Test on the suite;
  - Auto Test lists suite cases, runs, and reviews;
  - Send is enabled for a matching suite;
  - Past runs labels suite runs;
  - the bug footer has no PBI.
