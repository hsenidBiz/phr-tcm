/**
 * DEMO DATA MODE - dev builds only (loaded via a DEV-gated dynamic import
 * in main.tsx, so `tauri build` ships none of it). When enabled, every
 * Azure DevOps command is patched at the bindings layer to serve a fake
 * in-memory org: reads return dummy data, and creates/updates/imports land
 * in the in-memory store - nothing ever reaches a real organization. The
 * store keeps state for the session (submit -> the new cases show up in
 * Edit/View/Run), and resets on reload.
 */
import {
  commands,
  events,
  type AuthStatus,
  type BoardData,
  type CaseHistory,
  type DbDatabase,
  type DetectedTool,
  type EnsuredSuite,
  type NewWorkItem,
  type PbiHit,
  type RunOutcome,
  type SubmitItemResult,
  type TestCase,
  type TestCaseFull,
  type TestPoint,
  type WorkComment,
  type WorkItemDetail,
} from "../bindings";
import { isCaptureMode } from "./capture";

const DEMO_KEY = "tcm-v2-dev-demo";
const PREFS_KEY = "tcm-v2-prefs";
const PREFS_BACKUP_KEY = "tcm-v2-dev-demo-prefs-backup";

export function isDemoMode(): boolean {
  try {
    return localStorage.getItem(DEMO_KEY) === "on";
  } catch {
    return false;
  }
}

/** Flip the flag and reload - patches only apply at boot. Entering demo
 * swaps the persisted context to the fake org with the demo PBI already
 * selected (the real prefs are backed up); leaving restores them, so the
 * user lands exactly where they were. */
export function toggleDemoMode() {
  try {
    const entering = !isDemoMode();
    if (entering) {
      localStorage.setItem(PREFS_BACKUP_KEY, localStorage.getItem(PREFS_KEY) ?? "");
      localStorage.setItem(
        PREFS_KEY,
        JSON.stringify({
          org: "DemoOrg",
          project: "Demo Project",
          section: "manual",
          pbi: PBIS[0],
          workMode: false,
        }),
      );
      // Recents for the demo project so the pick-a-PBI empty states and
      // the picker have something to offer.
      localStorage.setItem("tcm-v2-recent-pbis:DemoOrg/Demo Project", JSON.stringify(PBIS));
    } else {
      const backup = localStorage.getItem(PREFS_BACKUP_KEY);
      if (backup) localStorage.setItem(PREFS_KEY, backup);
      else localStorage.removeItem(PREFS_KEY);
      localStorage.removeItem(PREFS_BACKUP_KEY);
      // Drop demo-scoped caches so nothing fake lingers in real use.
      for (const k of Object.keys(localStorage)) {
        if (k.includes(":DemoOrg")) localStorage.removeItem(k);
      }
    }
    localStorage.setItem(DEMO_KEY, entering ? "on" : "off");
  } catch {
    // storage unavailable
  }
  window.location.reload();
}

const ok = <T,>(data: T) => Promise.resolve({ status: "ok" as const, data });
const err = <E,>(error: E) => Promise.resolve({ status: "error" as const, error });
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
/** Timestamp N seconds ago - keeps the "in progress" demo run looking
 * genuinely live (elapsed times grow) instead of frozen in the past. */
const nowMinus = (seconds: number) => new Date(Date.now() - seconds * 1000).toISOString();

// ---------------------------------------------------------------- dataset

const PBIS: PbiHit[] = [
  { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" },
  { id: 1002, title: "Demo - Checkout redesign", work_item_type: "Product Backlog Item" },
];

const full = (
  id: number,
  title: string,
  tags: string,
  steps: [string, string][],
  preconditions = "",
): TestCaseFull => ({
  id,
  title,
  tags,
  // What ADO would hold; the editor compares against it to decide whether
  // a save needs to write Steps at all.
  steps_xml: "",
  automation_status: "Not Automated",
  steps: steps.map(([action, expected]) => ({ action, expected })),
  step_ids: steps.map((_, i) => String(i + 2)),
  module_value: "",
  preconditions,
});

const casesByPbi = new Map<number, TestCaseFull[]>([
  [
    1001,
    [
      full(5001, "Login - valid credentials", "smoke; demo", [
        ["Open the login page", "The form is shown"],
        ["Enter valid credentials and submit", "The dashboard opens"],
      ], "A demo user exists"),
      full(5002, "Login - wrong password shows an error", "regression; demo", [
        ["Open the login page", "The form is shown"],
        ["Enter a wrong password", "An inline error appears"],
        ["Submit again with the right password", "The dashboard opens"],
      ]),
      full(5003, "Login - locked account is refused", "regression", [
        ["Lock the demo account", "Account flagged"],
        ["Attempt to sign in", "A lockout message appears"],
      ]),
      full(5004, "Session - expires after timeout", "demo", [
        ["Sign in and idle past the timeout", "The session expires"],
        ["Interact with the page", "The user lands back on login"],
      ]),
      full(5005, "Profile - update display name", "smoke", [
        ["Open profile settings", "The current name is shown"],
        ["Change the name and save", "A confirmation toast appears"],
      ]),
    ],
  ],
  [1002, [full(5101, "Checkout - guest can pay", "demo", [["Add an item and check out as guest", "Payment succeeds"]])]],
]);

let nextId = 6000;

/** point/run state per case id: last outcome + a little history. */
const pointState = new Map<number, { outcome: string; runId: number | null }>([
  [5001, { outcome: "passed", runId: 700 }],
  [5002, { outcome: "failed", runId: 700 }],
  [5003, { outcome: "blocked", runId: 699 }],
  [5004, { outcome: "", runId: null }],
  [5005, { outcome: "passed", runId: 698 }],
  [5101, { outcome: "", runId: null }],
]);
const historyByCase = new Map<number, RunOutcome[]>([
  [5001, [
    { outcome: "Passed", completed_date: "2026-07-13T09:00:00Z", run_id: 700, result_id: 7000, run_by: "Demo Tester" },
    { outcome: "Failed", completed_date: "2026-07-12T09:00:00Z", run_id: 695, result_id: 6950, run_by: "Demo Tester" },
  ]],
  [5002, [{ outcome: "Failed", completed_date: "2026-07-13T09:00:00Z", run_id: 700, result_id: 7000, run_by: "Demo Tester" }]],
]);
let nextRunId = 701;

const SUITE_BY_PBI = new Map<number, EnsuredSuite>([
  [1001, { plan_id: 90, plan_name: "Demo Test Plan", suite_id: 91, created_plan: false }],
  [1002, { plan_id: 90, plan_name: "Demo Test Plan", suite_id: 93, created_plan: false }],
]);
const PBI_BY_SUITE = new Map([...SUITE_BY_PBI].map(([pbi, s]) => [s.suite_id, pbi]));

const boardItems: BoardData["items"] = [
  { id: 2001, title: "Demo Task - wire the login flow", work_item_type: "Task", state: "In Progress", state_color: "007acc", column: "In Progress", assigned_to: "Demo User", tags: "demo", priority: 2, changed_date: "2026-07-13T08:00:00Z", parent: { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" } },
  { id: 2002, title: "Demo Task - write test cases", work_item_type: "Task", state: "To Do", state_color: "b2b2b2", column: "To Do", assigned_to: "Demo User", tags: "", priority: 2, changed_date: "2026-07-13T07:00:00Z", parent: { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" } },
  { id: 2003, title: "Demo Bug - session timeout not enforced", work_item_type: "Bug", state: "To Do", state_color: "cc293d", column: "To Do", assigned_to: "Demo User", tags: "demo", priority: 1, changed_date: "2026-07-13T06:00:00Z", parent: null },
  { id: 2004, title: "Demo Task - done example", work_item_type: "Task", state: "Done", state_color: "339933", column: "Done", assigned_to: "Demo User", tags: "", priority: 3, changed_date: "2026-07-12T06:00:00Z", parent: { id: 1002, title: "Demo - Checkout redesign", work_item_type: "Product Backlog Item" } },
];

const comments = new Map<number, WorkComment[]>([
  [2003, [{ id: 1, text: "Repro'd on the demo build.", text_html: "<div>Repro'd on the demo build.</div>", created_by: "Demo User", created_by_id: "demo", created_date: "2026-07-13T06:30:00Z", modified_date: "2026-07-13T06:30:00Z", avatar_url: "" }]],
]);

const detailFor = (b: BoardData["items"][number]): WorkItemDetail => ({
  id: b.id,
  title: b.title,
  work_item_type: b.work_item_type,
  state: b.state,
  assigned_to: b.assigned_to,
  assigned_to_unique: "demo@local",
  activity: "Development",
  tags: b.tags,
  area_path: "Demo Project\\Demo Team",
  iteration_path: "Demo Project\\Sprint 1",
  remaining_work: 2,
  completed_work: 1,
  original_estimate: 3,
  start_date: "2026-07-10T00:00:00Z",
  target_date: "2026-07-15T00:00:00Z",
  description_text: "Demo description. Nothing here is real.",
  description_html: "<div><b>Demo description.</b> Nothing here is real.<ul><li>bullet one</li><li>bullet two</li></ul></div>",
  description_field: b.work_item_type === "Bug" ? "Microsoft.VSTS.TCM.ReproSteps" : "System.Description",
  extra_pages:
    b.work_item_type === "Bug"
      ? [
          {
            name: "RCA",
            fields: [
              { label: "Initial Findings", reference_name: "Demo.InitialFindings", section: 0, kind: "html", allowed: [], value: "<div>### What is the issue?\n**Session timeout** never fires in demo mode.</div>" },
              { label: "Root Cause Category", reference_name: "Demo.RootCauseCategory", section: 1, kind: "pick", allowed: ["Code Defect", "Design/Requirement", "Environment"], value: "Code Defect" },
            ],
          },
          {
            name: "Preventive Measures",
            fields: [
              { label: "Lessons Learned", reference_name: "Demo.LessonsLearned", section: 0, kind: "html", allowed: [], value: "" },
            ],
          },
        ]
      : [],
  extra_pages_error: null,
  inline_images: [],
});

// ------------------------------------------------------- capture-mode names

/** The help site's screenshots are taken on this data (capture mode), and
 * they must never show the word it is named by. Rather than a second
 * dataset, every answer the patched commands give passes through this map
 * in capture mode, so a screen added later is covered without anyone
 * remembering to. Ordered: longer phrases before the words inside them.
 * Whatever a phrase here does not cover falls to the last rule, which
 * keeps the case of the word it replaces ("Demo." -> "Portal."). Normal
 * sample-data mode (capture off) is untouched. */

/** The pull request's linked work item id was borrowed from a real
 *  organisation along with its title. Capture mode answers a sample id
 *  instead (2005 is free: the board uses 2001-2004), so no real work item
 *  number reaches a screenshot. neutralName only rewrites text, so the
 *  number itself is swapped in neutralValue. */
const BORROWED_WORK_ITEM = 143783;
const CAPTURE_WORK_ITEM = 2005;
const CAPTURE_NUMBERS = new Map<number, number>([[BORROWED_WORK_ITEM, CAPTURE_WORK_ITEM]]);

export const CAPTURE_NAMES: [string, string][] = [
  ["Demo - Login & session flow", "Login and session flow"],
  ["Demo - Checkout redesign", "Checkout redesign"],
  ["DemoOrg", "Contoso"],
  ["Demo Project", "Customer Portal"],
  ["Demo Team", "Portal Team"],
  ["Demo Test Plan", "Release 2.4"],
  ["Demo Folder", "Web app"],
  ["Demo Config", "Windows 11, Edge"],
  ["Demo Task - wire the login flow", "Wire the login form to the session service"],
  ["Demo Task - write test cases", "Write test cases for signing in"],
  ["Demo Bug - session timeout not enforced", "Session timeout is not enforced"],
  ["Demo Task - done example", "Add the guest checkout button"],
  ["Demo Task - ", ""],
  ["Demo Bug - ", ""],
  ["Demo User", "Alex Tester"],
  ["Demo Tester", "Priya Raman"],
  ["demo@local", "alex.tester@contoso.com"],
  ["Demo mode: ", ""],
  ["Demo description: what changed and why.", "What changed and why."],
  ["Demo description. Nothing here is real.", "Users stay signed in until they sign out or the session times out."],
  ["<b>Demo description.</b> Nothing here is real.", "<b>Users stay signed in</b> until they sign out or the session times out."],
  ["<li>bullet one</li><li>bullet two</li>", "<li>Sign in, then leave the page idle for 30 minutes.</li><li>Click any link: the sign-in page should open.</li>"],
  ["Demo failure comment", "The error message did not appear."],
  ["Demo: step 3", "Step 3"],
  ["the demo build", "the staging build"],
  ["in demo mode", "on staging"],
  ["A demo user exists", "A test user exists"],
  ["the demo account", "the test account"],
  ["A demo pull request - there is no description in the demo data.", "Adds the PBI scope filter to the board."],
  ["Publishing is off in demo data.", "Publishing is off."],
  // People, a work item and a test project from real life that the sample
  // history, pull request and build log borrowed.
  ["Avin Alwis", "Alex Tester"],
  ["Dilshan Kaviratne", "Sam Doyle"],
  ["Ishani Dasanayake", "Priya Raman"],
  ["Naveen Warnakulasuriya", "Jordan Lee"],
  ["Participants - Selected employees and department inconsistencies", "Sign-in page keeps the old session after a password change"],
  ["Timeline - split weight across sprints", "Session timeout is not enforced"],
  [`demo-wi/${BORROWED_WORK_ITEM}`, `demo-wi/${CAPTURE_WORK_ITEM}`],
  ["PeoplesHR.PMS.Infrastructure.Tests", "Portal.Infrastructure.Tests"],
  ["C:\\demo\\v2.exe", "C:\\Users\\alex.tester\\AppData\\Local\\TestCaseManager\\current\\v2.exe"],
];

/** The capture organisation and project, as the context bar shows them. */
export const CAPTURE_ORG = "Contoso";
export const CAPTURE_PROJECT = "Customer Portal";

/** One string under the capture names. */
export function neutralName(text: string): string {
  let out = text;
  for (const [from, to] of CAPTURE_NAMES) out = out.split(from).join(to);
  return out.replace(/\bdemo\b/gi, (w) => (w === "DEMO" ? "PORTAL" : w[0] === "D" ? "Portal" : "portal"));
}

/** Every string inside a value, under the capture names. */
function neutralValue<T>(value: T): T {
  if (typeof value === "string") return neutralName(value) as T;
  if (typeof value === "number") return (CAPTURE_NUMBERS.get(value) ?? value) as T;
  if (Array.isArray(value)) return value.map(neutralValue) as T;
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, neutralValue(v)])) as T;
  }
  return value;
}

/** The patches, each answering under the capture names. */
function neutralized<T extends Record<string, (...args: never[]) => unknown>>(patches: T): T {
  return Object.fromEntries(
    Object.entries(patches).map(([name, fn]) => [
      name,
      (...args: never[]) => Promise.resolve(fn(...args)).then(neutralValue),
    ]),
  ) as T;
}

/** A name as the screen shows it: neutral in capture mode, as-is otherwise. */
const shown = (text: string) => (isCaptureMode() ? neutralName(text) : text);

// ---------------------------------------------------------------- patches

function applyPatches() {
  // Capture mode shots must never name demo data (house rule): a neutral
  // display name stands in for the usual "(DEMO DATA)" marker.
  const auth: AuthStatus = {
    signed_in: true,
    account: isCaptureMode() ? "Alex Tester" : "demo@local (DEMO DATA)",
  };
  const allCases = () => [...casesByPbi.values()].flat();
  const findCase = (id: number) => allCases().find((c) => c.id === id);

  const patches = {
    authStatus: () => Promise.resolve(auth),
    signIn: () => ok(auth),
    checkUpdate: () => Promise.resolve({ available: null, blocked: null, failed_attempt: null }),
    applyUpdate: () => err("Demo mode: updates are disabled"),

    listOrgs: () => ok([{ name: "DemoOrg", url: "https://example.invalid/demo" }]),
    listProjects: () => ok([{ id: "demo-project", name: "Demo Project" }]),
    searchPbis: (_o: string, _p: string, query: string) =>
      // Matched on the name shown, so a capture-mode search finds what the
      // picker lists.
      ok(PBIS.filter((b) => shown(`${b.id} ${b.title}`).toLowerCase().includes(query.toLowerCase()))),

    listTestCaseFields: () => ok([]),
    testCaseFieldValues: () => ok([]),
    listProjectTags: () => ok(["demo", "smoke", "regression"]),
    classificationPaths: () => ok(["Demo Project", "Demo Project\\Demo Team"]),
    listIterations: () =>
      ok([
        { path: "Demo Project", start_date: null, finish_date: null },
        { path: "Demo Project\\Sprint 1", start_date: "2026-07-06T00:00:00Z", finish_date: "2026-07-17T00:00:00Z" },
        { path: "Demo Project\\Sprint 2", start_date: "2026-07-20T00:00:00Z", finish_date: "2026-07-31T00:00:00Z" },
      ]),
    activityValues: () => ok(["Development", "Testing"]),

    pbiTestCases: (_o: string, pbiId: number) =>
      ok((casesByPbi.get(pbiId) ?? []).map((c) => ({ id: c.id, title: c.title, tags: c.tags, automation_status: c.automation_status }))),
    pbiTestCasesFull: (_o: string, pbiId: number) => ok(casesByPbi.get(pbiId) ?? []),
    testCasesByIds: (_o: string, ids: number[]) =>
      ok(ids.map(findCase).filter((c): c is TestCaseFull => Boolean(c))),

    submitQueue: async (_o: string, _p: string, pbiId: number, queue: TestCase[]) => {
      const list = casesByPbi.get(pbiId) ?? [];
      casesByPbi.set(pbiId, list);
      const results: SubmitItemResult[] = [];
      for (const [i, tc] of queue.entries()) {
        await sleep(120); // let the progress UI breathe like a real run
        const existing = tc.update_id != null ? list.find((c) => c.id === tc.update_id) : undefined;
        if (existing) {
          existing.title = tc.title;
          existing.steps = tc.steps;
          existing.automation_status = tc.automation_status;
          if (tc.tags.trim()) existing.tags = tc.tags;
          if (tc.preconditions.trim()) existing.preconditions = tc.preconditions;
          results.push({ index: i, title: tc.title, action: "updated", id: existing.id, error: null });
        } else {
          const id = nextId++;
          list.push({ ...full(id, tc.title, tc.tags, []), steps: tc.steps, step_ids: tc.steps.map((_, s) => String(s + 2)), preconditions: tc.preconditions, automation_status: tc.automation_status });
          pointState.set(id, { outcome: "", runId: null });
          results.push({ index: i, title: tc.title, action: "created", id, error: null });
        }
      }
      return { status: "ok" as const, data: results };
    },
    // Demo mode never reaches Azure DevOps, so the delete is offered and
    // removes from the in-memory store only. Permission is granted here so
    // the confirmation screen can actually be exercised.
    canDeleteTestCases: () => ok(true),
    deleteTestCases: (_o: string, _p: string, ids: number[]) => {
      for (const [pbi, list] of casesByPbi) {
        casesByPbi.set(pbi, list.filter((c) => !ids.includes(c.id)));
      }
      return ok(ids.map((id) => ({ id, deleted: true, error: "" })));
    },

    updateTestCase: (_o: string, _p: string, tc: TestCase) => {
      const existing = tc.update_id != null ? findCase(tc.update_id) : undefined;
      if (existing) {
        existing.title = tc.title;
        existing.steps = tc.steps;
        existing.automation_status = tc.automation_status;
        if (tc.tags.trim()) existing.tags = tc.tags;
        if (tc.preconditions.trim()) existing.preconditions = tc.preconditions;
      }
      return ok(null);
    },

    ensurePbiSuite: (_o: string, _p: string, pbiId: number) =>
      ok(SUITE_BY_PBI.get(pbiId) ?? { plan_id: 90, plan_name: "Demo Test Plan", suite_id: 91, created_plan: false }),
    findPbiSuite: (_o: string, _p: string, pbiId: number) => ok(SUITE_BY_PBI.get(pbiId) ?? null),
    listPlansWithSuites: () =>
      ok([
        {
          plan: { id: 90, name: "Demo Test Plan", area_path: "Demo Project", root_suite_id: 1 },
          suites: [
            { id: 95, name: "Demo Folder", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            { id: 91, name: "Demo - Login & session flow", suite_type: "requirementTestSuite", requirement_id: 1001, parent_id: 95 },
            { id: 93, name: "Demo - Checkout redesign", suite_type: "requirementTestSuite", requirement_id: 1002, parent_id: 95 },
          ],
        },
      ]),
    listTestPoints: (_o: string, _p: string, _plan: number, suiteId: number) => {
      const pbiId = PBI_BY_SUITE.get(suiteId);
      const cases = pbiId != null ? (casesByPbi.get(pbiId) ?? []) : [];
      const points: TestPoint[] = cases.map((c) => {
        const st = pointState.get(c.id) ?? { outcome: "", runId: null };
        return {
          point_id: c.id + 40000,
          test_case_id: c.id,
          test_case_name: c.title,
          config_name: "Demo Config",
          tester: "Demo User",
          last_outcome: st.outcome,
          last_run_id: st.runId,
          last_result_id: st.runId != null ? st.runId + 100 : null,
        };
      });
      return ok(points);
    },
    runHistory: () =>
      ok([...historyByCase.entries()].map(([test_case_id, outcomes]): CaseHistory => ({ test_case_id, outcomes }))),
    // The incremental protocol the runner speaks now: open lazily, record
    // per case as the tester advances, complete at Finish.
    startTestRun: (_o: string, _p: string, _plan: number, _name: string, pointIds: number[]) => {
      const runId = nextRunId++;
      return ok({
        run_id: runId,
        web_url: "https://example.invalid/demo-run",
        results: pointIds.map((point_id) => ({ point_id, result_id: point_id + 100 })),
        unmatched: [],
      });
    },
    recordResult: (
      _o: string,
      _p: string,
      runId: number,
      _resultId: number,
      outcome: { point_id: number; outcome: string },
    ) => {
      const caseId = outcome.point_id - 40000;
      pointState.set(caseId, { outcome: outcome.outcome.toLowerCase(), runId });
      const hist = historyByCase.get(caseId) ?? [];
      hist.unshift({ outcome: outcome.outcome, completed_date: new Date().toISOString(), run_id: runId, result_id: runId * 10, run_by: "Demo Tester" });
      historyByCase.set(caseId, hist.slice(0, 5));
      return ok([]);
    },
    finishTestRun: () => ok(null),
    getResultDetail: () => ok({ outcome: "Failed", comment: "Demo failure comment" }),
    resultFailureDetail: () => ok({ comment: "Demo: step 3 timed out waiting for the redirect.", bug_ids: [2003] }),
    resultScreenshots: () => ok([]),
    fileBug: (_o: string, _p: string, title: string) => {
      const id = nextId++;
      boardItems.push({ id, title, work_item_type: "Bug", state: "To Do", state_color: "cc293d", column: "To Do", assigned_to: "Demo User", tags: "demo", priority: 2, changed_date: new Date().toISOString(), parent: null });
      return ok({
        id,
        url: "https://example.invalid/demo-bug",
        screenshots_failed: 0,
        screenshots_total: 0,
      });
    },
    viewExecutionReport: () => err("Demo mode: execution reports are disabled"),

    listRepos: () =>
      ok([
        { id: "demo-repo-1", name: "demo-web" },
        { id: "demo-repo-2", name: "demo-api" },
      ]),
    prOverview: () =>
      ok({
        awaiting: [
          {
            id: 501, title: "Add PBI scope to the board", repo: "demo-web", repo_id: "demo-repo-1",
            author: "Sam Rivera", source_branch: "feature/pbi-scope", target_branch: "main",
            created: "2026-07-17T09:00:00Z",
            description: "Demo description: what changed and why.", is_draft: false, has_conflicts: false,
            my_vote: 0,
            reviewers: [
              { display_name: "Demo User", vote: 0 },
              { display_name: "Alex Kim", vote: 10 },
            ],
            web_url: "https://example.invalid/demo-pr/501",
          },
        ],
        mine: [
          {
            id: 502, title: "Electric border for the PBI chip", repo: "demo-web", repo_id: "demo-repo-1",
            author: "Demo User", source_branch: "feature/electric-border", target_branch: "main",
            created: "2026-07-16T14:00:00Z",
            description: "Demo description: what changed and why.", is_draft: true, has_conflicts: true,
            my_vote: 0,
            reviewers: [{ display_name: "Sam Rivera", vote: -5 }],
            web_url: "https://example.invalid/demo-pr/502",
          },
        ],
      }),
    boardPrLinks: () =>
      ok([
        {
          work_item_id: 2001, pr_id: 501, status: "active",
          title: "Add PBI scope to the board", repo: "demo-web", repo_id: "demo-repo-1",
          web_url: "https://example.invalid/demo-pr/501",
        },
        {
          work_item_id: 2001, pr_id: 499, status: "completed",
          title: "Earlier slice", repo: "demo-db", web_url: "https://example.invalid/demo-pr/499",
        },
      ]),
    prWorkItems: () =>
      ok([
        {
          id: BORROWED_WORK_ITEM, work_item_type: "Bug",
          title: "Participants - Selected employees and department inconsistencies",
          state: "In Progress", state_color: "007acc",
          url: `https://example.invalid/demo-wi/${BORROWED_WORK_ITEM}`,
        },
      ]),
    // 501 is mid-build and 502 went red, so both pills show on the default
    // view; 503 passed and is therefore ABSENT, which is what a green run
    // looks like to this endpoint - no entry rather than "succeeded".
    prBuildStates: (_o: string, _p: string, _r: string, ids: number[]) =>
      ok(
        ids
          .map((id) =>
            id === 501 || id === 504 ? { pr_id: id, state: "running" }
            : id === 502 || id === 505 ? { pr_id: id, state: "failed" }
            : null,
          )
          .filter(Boolean),
      ),
    // One unresolved thread on a file, one general thread already resolved
    // - enough to see both states and both buttons without a live org.
    prThreads: () =>
      ok([
        {
          id: 9001,
          status: "active",
          file_path: "/src/components/LoginForm.tsx",
          line: 42,
          last_updated: new Date(Date.now() - 3 * 3600_000).toISOString(),
          comments: [
            {
              id: 1, author: "Priya Raman", author_id: "demo-priya", avatar: "", edited: false,
              published: new Date(Date.now() - 4 * 3600_000).toISOString(),
              content: "This swallows the error - can we surface it instead of `catch {}`?",
            },
            {
              id: 2, author: "Sam Doyle", author_id: "demo-sam", avatar: "", edited: true,
              published: new Date(Date.now() - 3 * 3600_000).toISOString(),
              content: "Good catch. Pushing a fix that toasts the message.",
            },
          ],
        },
        {
          id: 9002,
          status: "fixed",
          file_path: "",
          line: 0,
          last_updated: new Date(Date.now() - 26 * 3600_000).toISOString(),
          comments: [
            {
              id: 3, author: "Priya Raman", author_id: "demo-priya", avatar: "", edited: false,
              published: new Date(Date.now() - 28 * 3600_000).toISOString(),
              content: "Does this need a changelog entry?",
            },
          ],
        },
      ]),
    setPrThreadStatus: (
      _o: string, _p: string, _r: string, _i: number, _t: number, status: string,
    ) => ok(status),
    repoPullRequests: () =>
      ok([
        {
          id: 503, title: "Bump dependencies", repo: "demo-web", repo_id: "demo-repo-1",
          author: "Alex Kim", source_branch: "chore/deps", target_branch: "main",
          created: "2026-07-15T08:00:00Z",
            description: "Demo description: what changed and why.", is_draft: false, has_conflicts: false,
          my_vote: 10,
          reviewers: [{ display_name: "Demo User", vote: 10 }],
          web_url: "https://example.invalid/demo-pr/503",
          status: "active", closed: "", merge_commit: "",
        },
      ]),
    // Three runs so the pipeline views have one of each kind to show: one
    // still building (newest), one green that deployed, and the failed
    // PR-validation run - including the failing step message.
    prPipeline: () =>
      ok([
        {
          id: 902, name: "demo-web", number: "2026.7.25-03", status: "inProgress",
          result: "", is_validation: false,
          started: nowMinus(6 * 60), finished: "",
          web_url: "https://example.invalid/demo-build/902",
          stages: [
            {
              name: "Build", state: "completed", result: "succeeded",
              started: nowMinus(6 * 60), finished: nowMinus(2 * 60),
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "succeeded",
                  started: nowMinus(6 * 60), finished: nowMinus(2 * 60),
                  tasks: [
                    { name: "Restore The Solution", state: "completed", result: "succeeded",
                      started: nowMinus(6 * 60), finished: nowMinus(330), issues: [], log_id: 1 },
                    { name: "Build The Solution", state: "completed", result: "succeeded",
                      started: nowMinus(330), finished: nowMinus(180), issues: [], log_id: 2 },
                    { name: "Run Unit Test", state: "completed", result: "succeeded",
                      started: nowMinus(180), finished: nowMinus(120), issues: [], log_id: 3 },
                  ],
                },
              ],
            },
            {
              name: "Package", state: "inProgress", result: "",
              started: nowMinus(120), finished: "",
              jobs: [
                {
                  name: "Build Docker App", state: "inProgress", result: "",
                  started: nowMinus(120), finished: "",
                  tasks: [
                    { name: "Initialize job", state: "completed", result: "succeeded",
                      started: nowMinus(120), finished: nowMinus(110), issues: [], log_id: 4 },
                    { name: "Build The Image", state: "inProgress", result: "",
                      started: nowMinus(110), finished: "", issues: [], log_id: 5 },
                    { name: "Push The Image", state: "pending", result: "",
                      started: "", finished: "", issues: [], log_id: 6 },
                  ],
                },
              ],
            },
            { name: "Deploy", state: "pending", result: "", started: "", finished: "", jobs: [] },
          ],
          deployments: [],
        },
        {
          id: 901, name: "demo-web", number: "2026.7.24-12", status: "completed",
          result: "succeeded", is_validation: false,
          started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:06:00Z",
          web_url: "https://example.invalid/demo-build/901",
          stages: [
            {
              name: "Build", state: "completed", result: "succeeded",
              started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:04:00Z",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "succeeded",
                  started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:04:00Z",
                  tasks: [
                    { name: "Restore The Solution", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:00:20Z", issues: [], log_id: 7 },
                    { name: "Build The Solution", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:00:20Z", finished: "2026-07-24T09:03:00Z", issues: [], log_id: 8 },
                    { name: "Run Unit Test", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:03:00Z", finished: "2026-07-24T09:04:00Z", issues: [], log_id: 9 },
                  ],
                },
              ],
            },
            {
              name: "Package", state: "completed", result: "succeeded",
              started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:06:00Z",
              jobs: [
                {
                  name: "Build Docker App", state: "completed", result: "succeeded",
                  started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:06:00Z",
                  tasks: [
                    { name: "Build The Image", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:05:40Z", issues: [], log_id: 10 },
                    { name: "Push The Image", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:05:40Z", finished: "2026-07-24T09:06:00Z", issues: [], log_id: 11 },
                  ],
                },
              ],
            },
          ],
          deployments: [
            {
              release: "Release-482", environment: "QA", status: "succeeded",
              on: "2026-07-24T10:00:00Z", web_url: "https://example.invalid/demo-release/482",
            },
            {
              release: "Release-482", environment: "Production", status: "notStarted",
              on: "", web_url: "https://example.invalid/demo-release/482",
            },
          ],
        },
        {
          id: 900, name: "demo-web (Build)", number: "20260724.1", status: "completed",
          result: "failed", is_validation: true,
          started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:04:00Z",
          web_url: "https://example.invalid/demo-build/900",
          stages: [
            {
              name: "Build", state: "completed", result: "failed",
              started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:04:00Z",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "failed",
                  started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:04:00Z",
                  tasks: [
                    { name: "Restore The Solution", state: "completed", result: "succeeded",
                      started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:00:30Z", issues: [], log_id: 12 },
                    { name: "Run Unit Test", state: "completed", result: "failed",
                      started: "2026-07-24T08:00:30Z", finished: "2026-07-24T08:04:00Z",
                      issues: ["3 tests failed in PeoplesHR.PMS.Infrastructure.Tests"], log_id: 99 },
                    { name: "Publish Test Results", state: "completed", result: "skipped",
                      started: "", finished: "", issues: [], log_id: 13 },
                  ],
                },
              ],
            },
          ],
          deployments: [],
        },
      ]),

    fetchBoard: () =>
      ok({
        items: boardItems,
        states_by_type: {
          Task: [
            { name: "To Do", color: "b2b2b2", category: "Proposed" },
            { name: "In Progress", color: "007acc", category: "InProgress" },
            { name: "Done", color: "339933", category: "Completed" },
          ],
          Bug: [
            { name: "To Do", color: "cc293d", category: "Proposed" },
            { name: "In Progress", color: "007acc", category: "InProgress" },
            { name: "Done", color: "339933", category: "Completed" },
          ],
        },
      }),
    moveBoardItem: (_o: string, _p: string, itemId: number, _t: string, column: string) => {
      const item = boardItems.find((b) => b.id === itemId);
      const state = column === "Done" ? "Done" : column;
      if (item) {
        item.column = column;
        item.state = state;
        item.changed_date = new Date().toISOString();
      }
      return ok(state);
    },
    workItemDetail: (_o: string, _p: string, id: number) => {
      const item = boardItems.find((b) => b.id === id);
      return item ? ok(detailFor(item)) : err({ kind: "notFound" });
    },
    updateWorkItem: (_o: string, _p: string, id: number, patches: { reference_name: string; value: string }[]) => {
      const item = boardItems.find((b) => b.id === id);
      for (const p of patches) {
        if (!item) break;
        if (p.reference_name === "System.Title") item.title = p.value;
        if (p.reference_name === "System.State") item.state = p.value;
      }
      return ok(null);
    },
    workItemComments: (_o: string, _p: string, id: number) => ok(comments.get(id) ?? []),

    // A believably messy history: the item bounced New <-> In Progress
    // twice, which is exactly the case the state summary exists to make
    // readable. Dates are relative so the timeline's day grouping and
    // "3 days ago" labels stay sensible whenever the demo is opened.
    workItemHistory: (_o: string, _p: string, _id: number) => {
      const day = 86_400_000;
      // Days back, then a fixed hour of that day - and never in the
      // future, which adding hours to "now" would produce.
      const at = (daysAgo: number, hour = 10) => {
        const d = new Date(Date.now() - daysAgo * day);
        d.setHours(hour, 0, 0, 0);
        return new Date(Math.min(d.getTime(), Date.now() - 60_000)).toISOString();
      };
      const who = (by: string) => ({ by, avatar_url: "" });
      const rev = (
        n: number,
        by: string,
        atIso: string,
        fields: { reference_name: string; label: string; old: string; new: string }[],
        extra: Partial<{
          links_added: string[];
          links_removed: string[];
          comment_added: boolean;
        }> = {},
      ) => {
        const state = fields.find((f) => f.reference_name === "System.State");
        return {
          rev: n,
          ...who(by),
          at: atIso,
          fields,
          links_added: extra.links_added ?? [],
          links_removed: extra.links_removed ?? [],
          state_from: state?.old ?? "",
          state_to: state?.new ?? "",
          comment_added: extra.comment_added ?? false,
        };
      };
      const f = (reference_name: string, label: string, o: string, n: string) => ({
        reference_name,
        label,
        old: o,
        new: n,
      });
      // Newest first, as the real command returns.
      return ok([
        rev(9, "Avin Alwis", at(0, 9), [
          f("System.State", "State", "Resolved", "QA Ready"),
          f("System.Reason", "Reason", "Moved out of state Resolved", "Moved out of state In Progress"),
          f("Microsoft.VSTS.Scheduling.TargetDate", "Target Date", "2026-07-24T07:58:55Z", "2026-07-24T09:28:31Z"),
        ]),
        rev(8, "Avin Alwis", at(0, 8), [], { links_added: ["Related link"] }),
        rev(7, "Dilshan Kaviratne", at(1, 16), [], { links_added: ["Commit link"] }),
        rev(6, "Dilshan Kaviratne", at(1, 15), [f("System.State", "State", "In Progress", "Resolved")], {
          comment_added: true,
        }),
        rev(5, "Ishani Dasanayake", at(2, 11), [
          f("Microsoft.VSTS.Common.Priority", "Priority", "3", "2"),
          f("System.Tags", "Tags", "regression", "regression; smoke"),
        ]),
        rev(4, "Avin Alwis", at(3, 14), [
          f("Microsoft.VSTS.Scheduling.RemainingWork", "Remaining Work", "6.8", "0"),
        ]),
        rev(3, "Avin Alwis", at(4, 10), [f("System.State", "State", "New", "In Progress")]),
        rev(2, "Avin Alwis", at(6, 15), [f("System.State", "State", "In Progress", "New")]),
        rev(1, "Naveen Warnakulasuriya", at(9, 9), [
          f("System.State", "State", "", "New"),
          f("System.Title", "Title", "", "Timeline - split weight across sprints"),
          f("System.AssignedTo", "Assigned To", "", "Avin Alwis"),
        ]),
      ]);
    },
    // Comments arrive as the HTML the panel renders from markdown, the
    // way ADO stores them; the flattened `text` is the pre-HTML fallback.
    addComment: (_o: string, _p: string, id: number, text: string) => {
      const list = comments.get(id) ?? [];
      const now = new Date().toISOString();
      list.unshift({ id: Date.now(), text: text.replace(/<[^>]+>/g, ""), text_html: text, created_by: "Demo User", created_by_id: "demo", created_date: now, modified_date: now, avatar_url: "" });
      comments.set(id, list);
      return ok(null);
    },
    updateComment: (_o: string, _p: string, id: number, commentId: number, text: string) => {
      const c = (comments.get(id) ?? []).find((x) => x.id === commentId);
      if (c) {
        c.text_html = text;
        c.text = text.replace(/<[^>]+>/g, "");
        c.modified_date = new Date().toISOString();
      }
      return ok(null);
    },
    // The demo user owns every demo comment, so Edit shows on all of them.
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
    listTeamMembers: () => ok([{ display_name: "Demo User", unique_name: "demo@local" }]),
    listTeams: () => ok(["Demo Team"]),
    createWorkItem: (_o: string, _p: string, item: NewWorkItem) => {
      const id = nextId++;
      boardItems.push({ id, title: item.title, work_item_type: item.wi_type, state: "To Do", state_color: "b2b2b2", column: "To Do", assigned_to: item.assigned_to ?? "", tags: item.tags ?? "", priority: item.priority ?? 2, changed_date: new Date().toISOString(), parent: null });
      return ok({ id, url: "https://example.invalid/demo-wi" });
    },
    bridgeStatus: () => ok({ port: 51999, mcp_exe: "C:\\demo\\v2.exe" }),
    setBridgeContext: () => ok(null),
    watchAssignedWork: () => ok(null),
    detectAiTools: () =>
      Promise.resolve([
        { id: "claude-code", name: "Claude Code", installed: true, registered: true },
        { id: "vscode", name: "VS Code", installed: true, registered: false },
      ]),
    registerAiTool: () => ok(null),
    unregisterAiTool: () => ok(null),
    setAdoRateLevel: () => Promise.resolve(200),
    prDeployments: () => ok([]),
    shareQueue: () => ok("tcm-share:DemoOrg/Demo Project/1001/demo-aaaa-1111"),
    fetchSharedQueue: () =>
      ok({
        pbi_id: 1002, pbi_title: "Demo - Checkout redesign",
        pbi_work_item_type: "Product Backlog Item",
        organization: "DemoOrg", project: "Demo Project",
        cases: [
          {
            update_id: null, title: "Shared - reviewer sanity check", tags: "demo; shared",
            automation_status: "Not Automated", module_value: "",
            preconditions: "A demo user exists",
            comment: "Shared by a teammate for review.",
            // The one flow reviewer notes are written FOR: a draft sent to
            // somebody else to review. Shaped the way the guide asks - what
            // the case checks in plain words, then where the requirement
            // lives - and deliberately silent about where the SET came
            // from, which the developer settled at intake.
            reviewer_notes:
              "Checks that a cycle which has already been published cannot be " +
              "copied from again, so its settings cannot be overwritten by " +
              "accident.\n\n" +
              "Spec: **Step10-ManagePerformanceCycle.md** 7.7 (AC-3)\n\n" +
              "Code: `IndexModel.CanCopyFromPreviousCycle`",
            steps: [
              { action: "Open the app", expected: "It opens" },
              { action: "Open the shared draft", expected: "The case is listed" },
            ],
          },
        ],
        warnings: [],
      }),

    // Commands added after the demo store. Unpatched, each one went to the
    // real backend, which has no sign-in in demo mode, came back
    // Unauthorized and raised "Session expired" on the screens that load
    // them (Run Tests, Suite Management, the drawer, comments, pull
    // requests). demoCoverage.test.ts fails on the next one left out.
    workItemTypeStates: () =>
      ok([
        { name: "To Do", color: "b2b2b2", category: "Proposed" },
        { name: "In Progress", color: "007acc", category: "InProgress" },
        { name: "Done", color: "339933", category: "Completed" },
      ]),
    commentImages: () => ok([]),
    prDescription: () => ok("A demo pull request - there is no description in the demo data."),
    getRunOrder: () => ok({ state: "none" as const }),
    saveRunOrder: (_o: string, _p: string, _pbi: number, cases: { id: number; group?: string | null }[]) =>
      ok({
        format: "tcm-run-order",
        version: 1,
        saved_by: auth.account,
        saved_at: new Date().toISOString(),
        cases: cases.map((c) => ({ id: c.id, group: c.group ?? null })),
      }),
    // Capture mode lists each requirement suite's cases, so Suite
    // Management has an order to show; normal sample data keeps the empty
    // answer it always gave.
    listSuiteEntries: (_o: string, _p: string, suiteId: number) => {
      const pbiId = PBI_BY_SUITE.get(suiteId);
      const cases = isCaptureMode() && pbiId != null ? (casesByPbi.get(pbiId) ?? []) : [];
      return ok(cases.map((c, i) => ({ id: c.id, sequence_number: i, entry_type: "testCase" })));
    },
    reorderSuiteCases: (_o: string, _p: string, _s: number, caseIds: number[]) => ok(caseIds),
    canCreateTestSuites: () => ok(true),
    createStaticSuite: (_o: string, _p: string, _plan: number, parent: number, name: string) =>
      ok({ id: 9000 + Math.floor(Math.random() * 1000), name, suite_type: "staticTestSuite", requirement_id: null, parent_id: parent }),
    addCasesToSuite: (_o: string, _p: string, _plan: number, _s: number, caseIds: number[]) => ok(caseIds),
    relinkTestCases: (_o: string, _p: string, ids: number[]) => ok(ids.map((id) => ({ id, moved: true, error: null }))),
    resetTestPoints: () => ok(null),
    autoRunPublish: () => ok({ status: "refused" as const, why: "Publishing is off in demo data." }),

    buildLog: () =>
      ok(
        [
          "Starting: Run Unit Test",
          "==============================================================================",
          "Task         : .NET Core",
          "Determining projects to restore...",
          "Restored /home/vsts/work/1/s/src/Demo.Application.csproj (in 344 ms)",
          "##[warning]Package 'SQLitePCLRaw' 2.1.11 has a known vulnerability",
          "Test run for /home/vsts/work/1/s/Demo.Tests.dll (.NETCoreApp,Version=v10.0)",
          "Passed!  - Failed:     0, Passed:   203, Skipped:     0, Total:   203",
          "##[error]3 tests failed in PeoplesHR.PMS.Infrastructure.Tests",
          "Finishing: Run Unit Test",
        ].join(String.fromCharCode(10)),
      ),
    appLogDir: () => Promise.resolve("C:\\demo\\logs"),
    appLogs: () =>
      Promise.resolve([
        { at: "2026-07-26 09:00:01", level: "info", message: "Test Case Manager 1.11.2 started" },
        { at: "2026-07-26 09:00:04", level: "info", message: "Signed in to Azure DevOps" },
        { at: "2026-07-26 09:02:10", level: "info", message: "Submitting 3 test case(s) to DemoOrg/Demo Project PBI #1001" },
        { at: "2026-07-26 09:02:14", level: "warn", message: "Retrying after rate limit (5s)" },
        { at: "2026-07-26 09:02:20", level: "error", message: "Submit failed for 'Checkout - guest can pay': field 'Module' is required" },
        { at: "2026-07-26 09:02:21", level: "info", message: "Submit finished: 3 of 3 processed, 1 failed" },
      ]),
  };
  // Capture mode only (normal sample data is unchanged): what the AI Bridge
  // tab and a pull request's comments show in the help site's shots. The
  // database list and the PHR X defaults come from this machine otherwise -
  // real server names and logins - and the sample tools lack the fields the
  // tab reads to say where each one is registered.
  const captureOnly = {
    detectAiTools: () => Promise.resolve(CAPTURE_AI_TOOLS),
    dbDatabases: () => Promise.resolve(CAPTURE_DATABASES),
    dbServerDefaults: () => Promise.resolve({ exe_path: "", db_type: "mssql", schema_filter: "" }),
    // The open thread's reply carries a pasted screenshot.
    prThreads: () =>
      patches.prThreads().then((r) => ({
        ...r,
        data: r.data.map((t, i) =>
          i === 0
            ? { ...t, comments: t.comments.map((c, j) => (j === 1 ? { ...c, content: `${c.content}\n\n${CAPTURE_SHOT_MD}` } : c)) }
            : t,
        ),
      })),
    commentImages: (_o: string, texts: string[]) =>
      ok(texts.some((t) => t.includes(CAPTURE_SHOT_URL)) ? [{ url: CAPTURE_SHOT_URL, data: captureShotData() }] : []),
  };
  // Capture mode: every answer goes out under the neutral names, so no
  // shot shows the word the sample data is named by (see CAPTURE_NAMES).
  Object.assign(commands, isCaptureMode() ? neutralized({ ...patches, ...captureOnly }) : patches);
  muteRealSessionEvents();
}

/** Events the Rust side pushes from a REAL Azure DevOps session, which the
 * patched commands above cannot intercept. The assigned-work poller is a
 * Rust background task that keeps running against the real org if this dev
 * app was signed in before demo mode went on: without this, real work-item
 * titles would reach the bell, the Board badge and a toast - on screen, and
 * in a capture-mode shot. The pacer's "slow down" toast likewise only ever
 * comes from real traffic.
 *
 * Left alone on purpose: file-watch, draft-comment, report-note and intake
 * events. They come from files and pages the person opened on this
 * machine, not from Azure DevOps, and muting them would drop their edits. */
export const MUTED_EVENTS = ["workAssigned", "slowdownRequested"] as const;

function muteRealSessionEvents() {
  const none = () => Promise.resolve(() => {});
  for (const name of MUTED_EVENTS) Object.assign(events[name], { listen: none, once: none });
}

/** The queue the help site's screenshots are taken of (capture mode only).
 * The queue is a local draft, not Azure DevOps data, so nothing above
 * supplies one, and the capture script cannot pick a file or type a case.
 * Three rows show every state the queue documents: a new case with an
 * in-app comment and reviewer notes, an update whose title differs from
 * #5002 (a diff to open), and a new case titled like #5001 (the duplicate
 * check). Both orders are set, so the Order bar shows. */
export const CAPTURE_QUEUE: TestCase[] = [
  {
    title: "Login - remember me keeps you signed in",
    steps: [
      { action: "Open the login page", expected: "The form is shown" },
      { action: "Tick Remember me and sign in", expected: "The dashboard opens" },
      { action: "Close and reopen the browser", expected: "You are still signed in" },
    ],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "A user account exists",
    update_id: null,
    comment: "Asked the team how long the sign-in should last.",
    reviewer_notes: "Covers the **Remember me** option on the sign-in form.",
    tester_order: 1,
    spec_order: 2,
  },
  {
    title: "Login - wrong password shows an inline error",
    steps: [
      { action: "Open the login page", expected: "The form is shown" },
      { action: "Enter a wrong password", expected: "An inline error appears" },
      { action: "Submit again with the right password", expected: "The dashboard opens" },
    ],
    tags: "regression",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: 5002,
    tester_order: 2,
    spec_order: 1,
  },
  {
    title: "Login - valid credentials",
    steps: [
      { action: "Open the login page", expected: "The form is shown" },
      { action: "Enter valid credentials and submit", expected: "The dashboard opens" },
    ],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    tester_order: 3,
    spec_order: 3,
  },
];

/** The local comments View Test Cases shows in capture mode, by case id. */
export const CAPTURE_NOTES: Record<string, string> = {
  "5002": "Check the error wording once the new copy lands.",
};

/** Remembered view choices cleared on every capture-mode boot: exact keys,
 * or (ending in ":") every key with that prefix. */
export const CAPTURE_RESET = [
  "tcm-v2-group-cases",
  "tcm-v2-group-view",
  "tcm-v2-group-manage",
  "tcm-v2-group-mode",
  "tcm-v2-group-points",
  "tcm-v2-edit-collapsed-groups",
  "tcm-v2-view-collapsed-groups",
  "tcm-v2-run-collapsed-groups",
  "tcm-v2-manage-collapsed-groups",
  "tcm-v2-runner-pinned",
  "tcm-v2-run-order:",
  "tcm-v2-run-order-view:",
  // The AI Bridge tab and the Settings switches that change it.
  "tcm-v2-working-dir",
  "tcm-v2-ai-global-allowed",
  "tcm-v2-ai-scope",
  "tcm-v2-ai-show-phrx",
  "tcm-v2-mcp-disabled",
  "tcm-v2-db-mcp",
  "tcm-v2-db-writes",
  // The board's view options and the pull request filters.
  "tcm-v2-board-swimlanes",
  "tcm-v2-board-lanes-collapsed:",
  "tcm-v2-hidden-cols",
  "tcm-v2-type-filter",
  "tcm-v2-this-sprint",
  "tcm-v2-pr-yours:",
  "tcm-v2-pr-status",
];

/** The working repository the AI Bridge tab shows in capture mode. */
export const CAPTURE_REPO = "C:\\Projects\\customer-portal";

/** The AI tools the AI Bridge tab lists in capture mode: one registered in
 * the repository, one not yet, with a machine-wide copy left over. */
export const CAPTURE_AI_TOOLS: DetectedTool[] = [
  {
    id: "claude-code",
    name: "Claude Code",
    installed: true,
    registered_servers: ["tcm-testcases"],
    scope: "project",
    global_registered_servers: [],
  },
  {
    id: "vscode",
    name: "VS Code",
    installed: true,
    registered_servers: [],
    scope: "project",
    global_registered_servers: ["tcm-testcases"],
  },
];

/** The databases the Company database card offers in capture mode, in
 * place of this machine's own list and logins. */
export const CAPTURE_DATABASES: DbDatabase[] = [
  {
    id: "qa",
    label: "QA - read only",
    shipped: true,
    server: "sql-qa.contoso.local",
    port: 1433,
    database: "CustomerPortal_QA",
    user: "portal_reader",
    trust_cert: false,
    has_password: true,
    customised: false,
  },
  {
    id: "dev",
    label: "Dev - dev login",
    shipped: true,
    server: "sql-dev.contoso.local",
    port: 1433,
    database: "CustomerPortal_Dev",
    user: "portal_devlogin",
    trust_cert: false,
    has_password: true,
    customised: true,
  },
];

/** A screenshot pasted into a pull request comment (capture mode). */
export const CAPTURE_SHOT_URL =
  "https://dev.azure.com/contoso/_apis/git/repositories/web/pullRequests/501/attachments/sign-in-error.png";
const CAPTURE_SHOT_MD = `![sign-in error](${CAPTURE_SHOT_URL})`;
const CAPTURE_SHOT_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="360" height="132" viewBox="0 0 360 132">' +
  '<rect width="360" height="132" rx="8" fill="#f8fafc" stroke="#cbd5e1"/>' +
  '<rect x="20" y="18" width="320" height="28" rx="4" fill="#fee2e2" stroke="#fca5a5"/>' +
  '<text x="32" y="37" font-family="Segoe UI, sans-serif" font-size="13" fill="#b91c1c">Something went wrong. Please try again.</text>' +
  '<rect x="20" y="58" width="320" height="22" rx="4" fill="#ffffff" stroke="#cbd5e1"/>' +
  '<rect x="20" y="88" width="320" height="22" rx="4" fill="#ffffff" stroke="#cbd5e1"/>' +
  '<rect x="260" y="116" width="80" height="10" rx="3" fill="#15803d"/>' +
  "</svg>";
/** The screenshot as the data: URI `commentImages` answers with. */
function captureShotData(): string {
  return `data:image/svg+xml;base64,${btoa(CAPTURE_SHOT_SVG)}`;
}

/** Capture mode: every boot of the main window starts from the same scene -
 * the sample org, project and first PBI selected, the test-case screens
 * (not Work Manager) in front, and that PBI's queue set to CAPTURE_QUEUE.
 * Each shot's route begins with a reload, so a route that clears the PBI,
 * switches to Work Manager or uploads the queue cannot leave the next shot
 * somewhere else. The runner window boots this same bundle mid-route and
 * is left alone. The capture script puts the person's storage back when it
 * finishes, so these keys go with it. */
export function seedCaptureScene() {
  if (!isCaptureMode() || window.location.hash.startsWith("#runner")) return;
  try {
    // Under the capture names, the same ones listOrgs/listProjects/searchPbis
    // answer with, so the context bar's lists hold the selected values.
    const pbis = neutralValue(PBIS);
    localStorage.setItem(
      PREFS_KEY,
      JSON.stringify({ org: CAPTURE_ORG, project: CAPTURE_PROJECT, section: "manual", pbi: pbis[0], workMode: false }),
    );
    localStorage.setItem(`tcm-v2-recent-pbis:${CAPTURE_ORG}/${CAPTURE_PROJECT}`, JSON.stringify(pbis));
    localStorage.setItem(`tcm-v2-draft:${CAPTURE_ORG}/${PBIS[0].id}`, JSON.stringify(CAPTURE_QUEUE));
    // One local comment, so View Test Cases has a Comment chip to show.
    localStorage.setItem(`tcm-v2-case-notes:${CAPTURE_ORG}`, JSON.stringify(CAPTURE_NOTES));
    // A working repository with its AI tools on, and the dev database
    // chosen, so the AI Bridge tab shows every card.
    localStorage.setItem("tcm-v2-repositories", JSON.stringify([{ path: CAPTURE_REPO, enabled: true }]));
    localStorage.setItem("tcm-v2-current-repo", CAPTURE_REPO);
    localStorage.setItem("tcm-v2-db-selected", "dev");
    // One repository's pull requests tracked beside your own (the sample
    // repository's id, as the capture names answer it).
    localStorage.setItem(`tcm-v2-pr-repos:${CAPTURE_ORG}/${CAPTURE_PROJECT}`, JSON.stringify([neutralName("demo-repo-1")]));
    // View choices a shot can switch on (grouping, folded groups, a run
    // order, the runner's pin) are remembered across boots; each shot
    // starts from the defaults instead of what the shot before it chose.
    for (const key of Object.keys(localStorage)) {
      if (CAPTURE_RESET.some((k) => (k.endsWith(":") ? key.startsWith(k) : key === k))) localStorage.removeItem(key);
    }
  } catch {
    // storage unavailable
  }
}

/** Called from main.tsx (DEV only) before the app renders. */
export function maybeEnableDemoMode() {
  if (!isDemoMode()) return;
  applyPatches();
  seedCaptureScene();
}
